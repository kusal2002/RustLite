//! One top-level browser window: platform window, presentation surface, and
//! the page it shows.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;

use rustlite_browser_core::Page;
use rustlite_diagnostics::FrameStats;
use rustlite_paint::{DisplayList, Point, Size};
use rustlite_renderer::{Canvas, CanvasError, rasterize};
use softbuffer::{Context, SoftBufferError, Surface};
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

use crate::commands::{self, Command};
use crate::frame::{self, ChromeState, WINDOW_BACKGROUND};
use crate::toolbar::{ToolbarHit, ToolbarLayout};

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
    page: Page,
    toolbar: ToolbarLayout,
    chrome: ChromeState,
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
            .field("page", &self.page)
            .field("chrome", &self.chrome)
            .finish_non_exhaustive()
    }
}

impl BrowserWindow {
    pub fn new(
        event_loop: &ActiveEventLoop,
        size: Size,
        debug_overlay: bool,
        exit_after_frames: Option<u64>,
        initial: Option<(String, String)>,
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
        if let Some((url, source)) = initial {
            page.load_html(url, &source);
        }
        let title = match page.title().filter(|t| !t.is_empty()) {
            Some(t) => format!("{t} — RustLite"),
            None => "RustLite".to_owned(),
        };
        window.set_title(&title);

        Ok(Self {
            surface,
            _context: context,
            window,
            surface_size: None,
            page,
            toolbar,
            chrome: ChromeState {
                debug_overlay,
                ..ChromeState::default()
            },
            modifiers: ModifiersState::empty(),
            cursor: None,
            frame_list: DisplayList::new(),
            stats: FrameStats::new(FRAME_SAMPLES),
            exit_after_frames,
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn stats(&self) -> &FrameStats {
        &self.stats
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
                self.page.set_device_scale(scale_factor as f32);
                self.window.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    // One notch scrolls 100 CSS px, like Chromium on Windows.
                    MouseScrollDelta::LineDelta(_, y) => -y * 100.0,
                    MouseScrollDelta::PixelDelta(p) => -(p.y as f32) / self.page.device_scale(),
                };
                if self.page.scroll_by(dy) {
                    self.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => return self.redraw(),
            WindowEvent::ModifiersChanged(mods) => self.modifiers = mods.state(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                if let Some(command) = commands::shortcut(&event.logical_key, self.modifiers) {
                    return Ok(self.execute(command));
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
            _ => {}
        }
        Ok(Flow::Continue)
    }

    fn execute(&mut self, command: Command) -> Flow {
        tracing::debug!(?command, "command");
        match command {
            Command::Quit => Flow::Close,
            Command::ToggleDebugOverlay => {
                self.chrome.debug_overlay = !self.chrome.debug_overlay;
                tracing::info!(enabled = self.chrome.debug_overlay, "debug overlay");
                self.window.request_redraw();
                Flow::Continue
            }
            Command::ScrollLines(n) => self.scroll(|page| page.scroll_by(n as f32 * 40.0)),
            Command::ScrollPages(n) => self.scroll(|page| {
                // Keep a little overlap between pages, as browsers do.
                let page_height = page.css_viewport().1 * 0.875;
                page.scroll_by(n as f32 * page_height)
            }),
            Command::ScrollToTop => self.scroll(|page| page.scroll_to(0.0)),
            Command::ScrollToBottom => self.scroll(|page| page.scroll_to(f32::MAX)),
        }
    }

    fn scroll(&mut self, f: impl FnOnce(&mut Page) -> bool) -> Flow {
        if f(&mut self.page) {
            self.window.request_redraw();
        }
        Flow::Continue
    }

    fn set_cursor(&mut self, cursor: Option<Point>) {
        self.cursor = cursor;
        let hovered = cursor
            .and_then(|p| self.toolbar.hit_test(p))
            .and_then(|hit| match hit {
                ToolbarHit::Button(button) => Some(button),
                _ => None,
            });
        if hovered != self.chrome.hovered_button {
            self.chrome.hovered_button = hovered;
            self.window.request_redraw();
        }
    }

    fn click(&mut self) {
        let Some(point) = self.cursor else { return };
        match self.toolbar.hit_test(point) {
            Some(ToolbarHit::Button(button)) => {
                // Navigation arrives in roadmap phase 7; report rather than pretend.
                tracing::info!(
                    ?button,
                    "toolbar button clicked (navigation not implemented yet)"
                );
            }
            Some(ToolbarHit::AddressField) => {
                tracing::info!("address field clicked (text input not implemented yet)");
            }
            Some(ToolbarHit::Background) | None => {}
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

        let window_size = Size::new(inner.width, inner.height);
        frame::compose(
            &mut self.frame_list,
            window_size,
            &self.toolbar,
            &mut self.page,
            self.chrome,
        );

        let mut buffer = self.surface.buffer_mut()?;
        let mut canvas = Canvas::new(&mut buffer[..], inner.width, inner.height)?;
        let raster = rasterize(
            &mut canvas,
            &self.frame_list,
            WINDOW_BACKGROUND,
            self.page.glyph_rasterizer(),
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
