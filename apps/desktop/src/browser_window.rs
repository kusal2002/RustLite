//! One top-level browser window: platform window, presentation surface, the
//! toolbar, and the tab it shows.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use rustlite_browser_core::{Initiator, NavState, Page, Tab};
use rustlite_diagnostics::FrameStats;
use rustlite_net::{LoadResult, Loader};
use rustlite_paint::{DisplayList, Point, Size};
use rustlite_renderer::{Canvas, CanvasError, rasterize};
use softbuffer::{Context, SoftBufferError, Surface};
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::{CursorIcon, Window, WindowId};

use crate::address_bar::{AddressBar, Edit};
use crate::commands::{self, Command};
use crate::frame::{self, WINDOW_BACKGROUND};
use crate::toolbar::{
    ToolbarButton, ToolbarHit, ToolbarLayout, ToolbarView, UI_FONT_SIZE, shape_ui_text,
};

/// Frame-time samples kept for the exit summary (~4 s at 60 FPS).
const FRAME_SAMPLES: usize = 240;

#[derive(Debug, thiserror::Error)]
pub enum WindowError {
    #[error("failed to create window")]
    Create(#[from] winit::error::OsError),
    #[error("presentation surface error")]
    Surface(#[from] SoftBufferError),
    #[error("presentation buffer does not match window size")]
    Canvas(#[from] CanvasError),
}

/// What the event loop should do after a window handled an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Close,
}

pub struct BrowserWindow {
    // Field order matters: the surface must drop before the context and window.
    surface: Surface<Rc<Window>, Rc<Window>>,
    _context: Context<Rc<Window>>,
    window: Rc<Window>,
    surface_size: Option<(NonZeroU32, NonZeroU32)>,
    tab: Tab,
    toolbar: ToolbarLayout,
    address: AddressBar,
    hovered: Option<ToolbarButton>,
    over_link: bool,
    debug_overlay: bool,
    modifiers: ModifiersState,
    cursor: Option<Point>,
    frame_list: DisplayList,
    stats: FrameStats,
    exit_after_frames: Option<u64>,
}

impl std::fmt::Debug for BrowserWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserWindow")
            .field("id", &self.window.id())
            .field("tab", &self.tab)
            .finish_non_exhaustive()
    }
}

impl BrowserWindow {
    pub fn new(
        event_loop: &ActiveEventLoop,
        size: Size,
        debug_overlay: bool,
        exit_after_frames: Option<u64>,
        loader: Option<Arc<Loader>>,
        initial: Option<String>,
    ) -> Result<Self, WindowError> {
        let attributes = Window::default_attributes()
            .with_title("RustLite")
            .with_inner_size(PhysicalSize::new(size.width, size.height))
            .with_min_inner_size(PhysicalSize::new(200, 120));
        let window = Rc::new(event_loop.create_window(attributes)?);
        let context = Context::new(window.clone())?;
        let surface = Surface::new(&context, window.clone())?;

        let inner = window.inner_size();
        let toolbar = ToolbarLayout::new(inner.width, window.scale_factor());
        let viewport = frame::content_size(Size::new(inner.width, inner.height), &toolbar);
        tracing::info!(
            width = inner.width,
            height = inner.height,
            scale_factor = window.scale_factor(),
            "window created"
        );

        let mut page = Page::new(viewport);
        page.set_device_scale(window.scale_factor() as f32);
        let mut this = Self {
            surface,
            _context: context,
            window,
            surface_size: None,
            tab: Tab::new(page, loader),
            toolbar,
            address: AddressBar::default(),
            hovered: None,
            over_link: false,
            debug_overlay,
            modifiers: ModifiersState::empty(),
            cursor: None,
            frame_list: DisplayList::new(),
            stats: FrameStats::new(FRAME_SAMPLES),
            exit_after_frames,
        };
        match initial {
            Some(input) => {
                if let Err(e) = this.tab.navigate(&input) {
                    tracing::warn!(error = %e, "initial navigation failed");
                }
            }
            None => this.address.focus(),
        }
        this.sync_chrome();
        Ok(this)
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn stats(&self) -> &FrameStats {
        &self.stats
    }

    /// Delivers a finished network load (from a loader thread, via the event loop).
    pub fn on_load_result(&mut self, result: LoadResult) {
        if self.tab.on_load_result(result) {
            self.sync_chrome();
            self.window.request_redraw();
        }
    }

    /// Updates the title and address bar from the tab's state.
    fn sync_chrome(&mut self) {
        let url = match self.tab.state() {
            NavState::Loading { url } => Some(url),
            NavState::Idle => self.tab.url().cloned(),
        };
        let url_text = url.as_ref().map_or("", |u| u.as_str());
        let url_text = if url_text == "about:blank" {
            ""
        } else {
            url_text
        };
        self.address.show_url(url_text);
        let title = match self.tab.page().title().filter(|t| !t.is_empty()) {
            Some(t) => format!("{t} — RustLite"),
            None if !url_text.is_empty() => format!("{url_text} — RustLite"),
            None => "RustLite".to_owned(),
        };
        self.window.set_title(&title);
        self.window.request_redraw();
    }

    pub fn handle_event(&mut self, event: WindowEvent) -> Result<Flow, WindowError> {
        match event {
            WindowEvent::CloseRequested => return Ok(Flow::Close),
            WindowEvent::Resized(size) => {
                tracing::debug!(width = size.width, height = size.height, "resized");
                self.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                tracing::info!(scale_factor, "scale factor changed");
                self.tab.page_mut().set_device_scale(scale_factor as f32);
                self.window.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let page = self.tab.page_mut();
                let dy = match delta {
                    // One notch scrolls 100 CSS px, like Chromium on Windows.
                    MouseScrollDelta::LineDelta(_, y) => -y * 100.0,
                    MouseScrollDelta::PixelDelta(p) => -(p.y as f32) / page.device_scale(),
                };
                if page.scroll_by(dy) {
                    self.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => return self.redraw(),
            WindowEvent::ModifiersChanged(mods) => self.modifiers = mods.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                // The focused address bar gets keys first (including repeats).
                match self
                    .address
                    .key(&event.logical_key, event.text.as_deref(), self.modifiers)
                {
                    Edit::Changed | Edit::Cancel => {
                        self.window.request_redraw();
                        return Ok(Flow::Continue);
                    }
                    Edit::Submit(input) => {
                        if let Err(e) = self.tab.navigate(&input) {
                            tracing::warn!(error = %e, "navigation rejected");
                        }
                        self.sync_chrome();
                        return Ok(Flow::Continue);
                    }
                    Edit::Ignored => {}
                }
                if let Some(command) = commands::shortcut(&event.logical_key, self.modifiers) {
                    let scroll_key = matches!(
                        command,
                        Command::ScrollLines(_)
                            | Command::ScrollPages(_)
                            | Command::ScrollToTop
                            | Command::ScrollToBottom
                    );
                    if !event.repeat || scroll_key {
                        return Ok(self.execute(command));
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                // Saturating float-to-int casts; positions outside i32 are meaningless anyway.
                self.set_cursor(Some(Point::new(position.x as i32, position.y as i32)));
            }
            WindowEvent::CursorLeft { .. } => self.set_cursor(None),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.click(),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Back,
                ..
            } => {
                return Ok(self.execute(Command::Back));
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Forward,
                ..
            } => {
                return Ok(self.execute(Command::Forward));
            }
            _ => {}
        }
        Ok(Flow::Continue)
    }

    fn execute(&mut self, command: Command) -> Flow {
        tracing::debug!(?command, "command");
        let navigated = match command {
            Command::Quit => return Flow::Close,
            Command::ToggleDebugOverlay => {
                self.debug_overlay = !self.debug_overlay;
                tracing::info!(enabled = self.debug_overlay, "debug overlay");
                false
            }
            Command::ScrollLines(n) => self.tab.page_mut().scroll_by(n as f32 * 40.0),
            Command::ScrollPages(n) => {
                let page = self.tab.page_mut();
                // Keep a little overlap between pages, as browsers do.
                let page_height = page.css_viewport().1 * 0.875;
                page.scroll_by(n as f32 * page_height)
            }
            Command::ScrollToTop => self.tab.page_mut().scroll_to(0.0),
            Command::ScrollToBottom => self.tab.page_mut().scroll_to(f32::MAX),
            Command::FocusAddress => {
                self.address.focus();
                false
            }
            Command::Back => self.tab.back(),
            Command::Forward => self.tab.forward(),
            Command::Reload => self.tab.reload(),
            Command::Stop => {
                self.tab.stop();
                true
            }
        };
        if navigated {
            self.sync_chrome();
        }
        self.window.request_redraw();
        Flow::Continue
    }

    fn content_point(&self, point: Point) -> Option<(f32, f32)> {
        let top = self.toolbar.height() as i32;
        (point.y >= top).then(|| (point.x as f32, (point.y - top) as f32))
    }

    fn set_cursor(&mut self, cursor: Option<Point>) {
        self.cursor = cursor;
        let hovered = cursor
            .and_then(|p| self.toolbar.hit_test(p))
            .and_then(|hit| match hit {
                ToolbarHit::Button(button) => Some(button),
                _ => None,
            });
        if hovered != self.hovered {
            self.hovered = hovered;
            self.window.request_redraw();
        }
        let over_link = cursor
            .and_then(|p| self.content_point(p))
            .is_some_and(|(x, y)| self.tab.page_mut().link_at(x, y).is_some());
        if over_link != self.over_link {
            self.over_link = over_link;
            self.window.set_cursor(if over_link {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            });
        }
    }

    fn click(&mut self) {
        let Some(point) = self.cursor else { return };
        match self.toolbar.hit_test(point) {
            Some(ToolbarHit::Button(button)) => {
                let command = match button {
                    ToolbarButton::Back => Command::Back,
                    ToolbarButton::Forward => Command::Forward,
                    ToolbarButton::Reload
                        if matches!(self.tab.state(), NavState::Loading { .. }) =>
                    {
                        Command::Stop
                    }
                    ToolbarButton::Reload => Command::Reload,
                };
                self.execute(command);
            }
            Some(ToolbarHit::AddressField) => {
                self.address.focus();
                self.window.request_redraw();
            }
            Some(ToolbarHit::Background) => {}
            None => {
                if self.address.is_focused() {
                    self.address.blur();
                    self.window.request_redraw();
                }
                let link = self
                    .content_point(point)
                    .and_then(|(x, y)| self.tab.page_mut().link_at(x, y));
                if let Some(url) = link {
                    tracing::info!(%url, "link clicked");
                    if self.tab.navigate_to(url, Initiator::Page).is_ok() {
                        self.sync_chrome();
                    }
                }
            }
        }
    }

    fn toolbar_view(&mut self) -> ToolbarView {
        let loading = matches!(self.tab.state(), NavState::Loading { .. });
        let (can_back, can_forward) = (self.tab.can_go_back(), self.tab.can_go_forward());
        let text = self.address.text().to_owned();
        let fonts = self.tab.page_mut().fonts();
        let reload_icon = if loading { "\u{2715}" } else { "\u{21BB}" };
        ToolbarView {
            hovered: self.hovered,
            can_back,
            can_forward,
            loading,
            address: shape_ui_text(fonts, &text, UI_FONT_SIZE),
            caret: self.address.is_focused().then(|| self.address.cursor()),
            all_selected: self.address.is_all_selected(),
            icons: [
                shape_ui_text(fonts, "\u{2190}", UI_FONT_SIZE + 2.0),
                shape_ui_text(fonts, "\u{2192}", UI_FONT_SIZE + 2.0),
                shape_ui_text(fonts, reload_icon, UI_FONT_SIZE + 2.0),
            ],
        }
    }

    fn redraw(&mut self) -> Result<Flow, WindowError> {
        let inner = self.window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(inner.width), NonZeroU32::new(inner.height))
        else {
            // Minimized: nothing to present.
            return Ok(Flow::Continue);
        };
        let start = Instant::now();

        if self.surface_size != Some((width, height)) {
            self.surface.resize(width, height)?;
            self.surface_size = Some((width, height));
        }
        // Layout is a handful of integer ops; recomputing beats tracking width/DPI changes.
        self.toolbar = ToolbarLayout::new(inner.width, self.window.scale_factor());
        let view = self.toolbar_view();

        let window_size = Size::new(inner.width, inner.height);
        frame::compose(
            &mut self.frame_list,
            window_size,
            &self.toolbar,
            &view,
            self.tab.page_mut(),
            self.debug_overlay,
        );

        let mut buffer = self.surface.buffer_mut()?;
        let mut canvas = Canvas::new(&mut buffer[..], inner.width, inner.height)?;
        let raster = rasterize(
            &mut canvas,
            &self.frame_list,
            WINDOW_BACKGROUND,
            self.tab.page_mut().glyph_rasterizer(),
        );
        self.window.pre_present_notify();
        buffer.present()?;

        let elapsed = start.elapsed();
        self.stats.record(elapsed);
        tracing::trace!(
            ?elapsed,
            items = raster.items,
            pixels = raster.pixels_written,
            "frame"
        );

        if let Some(limit) = self.exit_after_frames {
            if self.stats.total_frames() >= limit {
                return Ok(Flow::Close);
            }
            self.window.request_redraw();
        }
        Ok(Flow::Continue)
    }
}
