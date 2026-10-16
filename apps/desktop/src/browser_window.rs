//! One top-level browser window: platform window, presentation surface, the
//! tab strip, the toolbar, and the tabs it shows.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use rustlite_browser_core::{Initiator, NavState, Page, PageKey, Tab, load_id_space};
use rustlite_diagnostics::FrameStats;
use rustlite_net::{LoadResult, Loader, Url};
use rustlite_paint::{DisplayList, Point, Size};
use rustlite_renderer::{Canvas, CanvasError, rasterize};
use softbuffer::{Context, SoftBufferError, Surface};
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use crate::address_bar::{AddressBar, Edit};
use crate::commands::{self, Command};
use crate::findbar::{
    FindBarLayout, FindBarView, FindButton, FindHit, FindState, paint_highlights, scroll_to_reveal,
};
use crate::frame::{self, WINDOW_BACKGROUND};
use crate::tabstrip::{TabHit, TabLabel, TabStripLayout, TabStripView};
use crate::toolbar::{
    ToolbarButton, ToolbarHit, ToolbarLayout, ToolbarView, UI_FONT_SIZE, shape_ui_text,
};

/// Frame-time samples kept for the exit summary (~4 s at 60 FPS).
const FRAME_SAMPLES: usize = 240;
/// Frame interval for `requestAnimationFrame`-driven redraws (~60 Hz).
const ANIMATION_FRAME: std::time::Duration = std::time::Duration::from_micros(16_667);

/// Tab titles are cut to this many characters before shaping.
const TAB_TITLE_CHARS: usize = 60;

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

/// A tab plus the per-tab browser UI state.
#[derive(Debug)]
struct TabSlot {
    tab: Tab,
    address: AddressBar,
    find: Option<FindState>,
    /// Scroll the current find match into view on the next frame.
    reveal_match: bool,
}

/// The chrome control under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hover {
    Tab(TabHit),
    Toolbar(ToolbarButton),
    Find(FindButton),
}

pub struct BrowserWindow {
    // Field order matters: the surface must drop before the context and window.
    surface: Surface<Rc<Window>, Rc<Window>>,
    _context: Context<Rc<Window>>,
    window: Rc<Window>,
    surface_size: Option<(NonZeroU32, NonZeroU32)>,
    /// Never empty. Only the active tab's page holds the window's font
    /// context; it moves with the selection (see [`Page::take_fonts`]).
    tabs: Vec<TabSlot>,
    active: usize,
    loader: Option<Arc<Loader>>,
    /// Load-id space for the next tab, so results route to their tab.
    next_space: u32,
    strip: TabStripLayout,
    toolbar: ToolbarLayout,
    /// Where the find bar was last drawn (for hit testing), if open.
    find_bar: Option<FindBarLayout>,
    hovered: Option<Hover>,
    over_link: bool,
    debug_overlay: bool,
    modifiers: ModifiersState,
    cursor: Option<Point>,
    frame_list: DisplayList,
    stats: FrameStats,
    exit_after_frames: Option<u64>,
    /// New tabs run page scripts.
    javascript: bool,
    /// When to draw the next animation frame (the active page asked for one).
    next_frame: Option<Instant>,
    /// Opened on first use; `None` until then or if the platform has none.
    clipboard: Option<arboard::Clipboard>,
}

impl std::fmt::Debug for BrowserWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserWindow")
            .field("id", &self.window.id())
            .field("tabs", &self.tabs.len())
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

/// The URL the address bar shows for `tab` (empty for blank tabs).
fn tab_url_text(tab: &Tab) -> String {
    let url = match tab.state() {
        NavState::Loading { url } => Some(url),
        NavState::Idle => tab.url().cloned(),
    };
    url.map(|u| u.to_string())
        .filter(|u| u != "about:blank")
        .unwrap_or_default()
}

/// The text a tab shows: its title, else its URL, else "New Tab".
fn tab_title(tab: &Tab) -> String {
    let title = tab.page().title().filter(|t| !t.is_empty());
    let url = tab_url_text(tab);
    let text = title
        .or_else(|| (!url.is_empty()).then_some(url))
        .unwrap_or_else(|| "New Tab".to_owned());
    text.chars().take(TAB_TITLE_CHARS).collect()
}

impl BrowserWindow {
    pub fn new(
        event_loop: &ActiveEventLoop,
        size: Size,
        debug_overlay: bool,
        exit_after_frames: Option<u64>,
        loader: Option<Arc<Loader>>,
        initial: Option<String>,
        javascript: bool,
    ) -> Result<Self, WindowError> {
        let attributes = Window::default_attributes()
            .with_title("RustLite")
            .with_inner_size(PhysicalSize::new(size.width, size.height))
            .with_min_inner_size(PhysicalSize::new(200, 120));
        let window = Rc::new(event_loop.create_window(attributes)?);
        let context = Context::new(window.clone())?;
        let surface = Surface::new(&context, window.clone())?;

        let inner = window.inner_size();
        let scale = window.scale_factor();
        let strip = TabStripLayout::new(inner.width, scale, 1);
        let toolbar = ToolbarLayout::at(strip.height(), inner.width, scale);
        tracing::info!(
            width = inner.width,
            height = inner.height,
            scale_factor = scale,
            "window created"
        );

        let mut this = Self {
            surface,
            _context: context,
            window,
            surface_size: None,
            tabs: Vec::new(),
            active: 0,
            loader,
            next_space: 0,
            strip,
            toolbar,
            find_bar: None,
            hovered: None,
            over_link: false,
            debug_overlay,
            modifiers: ModifiersState::empty(),
            cursor: None,
            frame_list: DisplayList::new(),
            stats: FrameStats::new(FRAME_SAMPLES),
            exit_after_frames,
            javascript,
            next_frame: None,
            clipboard: None,
        };
        let slot = this.new_slot();
        this.tabs.push(slot);
        match initial {
            Some(input) => {
                if let Err(e) = this.tabs[0].tab.navigate(&input) {
                    tracing::warn!(error = %e, "initial navigation failed");
                }
            }
            None => this.tabs[0].address.focus(),
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

    fn slot(&self) -> &TabSlot {
        &self.tabs[self.active]
    }

    fn slot_mut(&mut self) -> &mut TabSlot {
        &mut self.tabs[self.active]
    }

    fn page_mut(&mut self) -> &mut Page {
        self.tabs[self.active].tab.page_mut()
    }

    /// A blank tab with its own load-id space, sized like the current one.
    fn new_slot(&mut self) -> TabSlot {
        let inner = self.window.inner_size();
        let viewport = frame::content_size(Size::new(inner.width, inner.height), &self.toolbar);
        let mut page = Page::new(viewport);
        page.set_device_scale(self.window.scale_factor() as f32);
        page.set_javascript_enabled(self.javascript);
        let space = self.next_space;
        self.next_space = self.next_space.wrapping_add(1);
        TabSlot {
            tab: Tab::with_id_space(page, self.loader.clone(), space),
            address: AddressBar::default(),
            find: None,
            reveal_match: false,
        }
    }

    /// Makes tab `index` the active one, handing it the font context.
    fn select(&mut self, index: usize) {
        if index >= self.tabs.len() || index == self.active {
            return;
        }
        if let Some(fonts) = self.page_mut().take_fonts() {
            self.tabs[index].tab.page_mut().set_fonts(fonts);
        }
        self.active = index;
        self.sync_chrome();
    }

    /// Opens a tab after the active one; `url` loads in it, or its address
    /// bar is focused when `None`. `foreground` selects it.
    fn open_tab(&mut self, url: Option<Url>, foreground: bool) {
        let mut slot = self.new_slot();
        match url {
            Some(url) => {
                if let Err(e) = slot.tab.navigate_to(url, Initiator::Page) {
                    tracing::warn!(error = %e, "new tab navigation rejected");
                }
                slot.address.show_url(&tab_url_text(&slot.tab));
            }
            None => slot.address.focus(),
        }
        let index = self.active + 1;
        self.tabs.insert(index, slot);
        if foreground {
            self.select(index);
        }
        self.window.request_redraw();
    }

    /// Closes tab `index`. Returns `Flow::Close` when it was the last one.
    fn close_tab(&mut self, index: usize) -> Flow {
        if index >= self.tabs.len() {
            return Flow::Continue;
        }
        if self.tabs.len() == 1 {
            return Flow::Close;
        }
        let fonts = if index == self.active {
            self.page_mut().take_fonts()
        } else {
            None
        };
        // Dropping the tab cancels its loads.
        drop(self.tabs.remove(index));
        if index < self.active || self.active >= self.tabs.len() {
            self.active -= 1;
        }
        if let Some(fonts) = fonts {
            self.page_mut().set_fonts(fonts);
        }
        self.sync_chrome();
        Flow::Continue
    }

    /// Delivers a finished network load to the tab that started it.
    pub fn on_load_result(&mut self, result: LoadResult) {
        let space = load_id_space(result.id);
        let Some(index) = self.tabs.iter().position(|s| s.tab.id_space() == space) else {
            tracing::debug!(id = result.id, "load result for a closed tab ignored");
            return;
        };
        let slot = &mut self.tabs[index];
        if slot.tab.on_load_result(result) {
            let text = tab_url_text(&slot.tab);
            slot.address.show_url(&text);
            if slot.tab.state() == NavState::Idle && slot.tab.pending_subresources() == 0 {
                self.log_page_memory(index);
                self.log_cache();
            }
            if index == self.active {
                self.sync_chrome();
            }
            self.window.request_redraw();
        }
    }

    /// Runs page timers due at `now` in every tab and starts animation
    /// frames that are due. Called by the event loop when it wakes.
    pub fn tick(&mut self, now: Instant) {
        let mut changed = false;
        for index in 0..self.tabs.len() {
            if self.tabs[index].tab.run_timers(now) {
                changed = true;
                if index == self.active {
                    self.sync_chrome();
                }
            }
        }
        if self.next_frame.is_some_and(|t| t <= now) {
            self.next_frame = None;
            changed = true;
        }
        if changed {
            self.window.request_redraw();
        }
    }

    /// When the event loop must wake next: the earliest page timer or the
    /// next animation frame. `None` sleeps until an event arrives.
    pub fn next_deadline(&mut self) -> Option<Instant> {
        let timers = self.tabs.iter_mut().filter_map(|s| s.tab.next_timer());
        timers.chain(self.next_frame).min()
    }

    /// Logs every tab's memory and the shared caches (on exit).
    pub fn log_all_memory(&mut self) {
        for index in 0..self.tabs.len() {
            self.log_page_memory(index);
        }
        self.log_cache();
    }

    /// Logs a tab's memory (the active tab also holds the font caches).
    fn log_page_memory(&mut self, index: usize) {
        let page = self.tabs[index].tab.page_mut();
        // Lay out and paint first so the report covers the whole pipeline.
        page.display_list();
        tracing::info!(tab = index, memory = %page.memory_report(), "page memory");
    }

    fn log_cache(&self) {
        if let Some(loader) = &self.loader {
            let c = loader.cache_stats();
            tracing::info!(
                entries = c.entries,
                kib = c.bytes / 1024,
                hits = c.hits,
                misses = c.misses,
                evictions = c.evictions,
                "http cache"
            );
        }
    }

    /// Updates the window title and address bar from the active tab's state.
    fn sync_chrome(&mut self) {
        let slot = &mut self.tabs[self.active];
        let url_text = tab_url_text(&slot.tab);
        slot.address.show_url(&url_text);
        let title = match slot.tab.page().title().filter(|t| !t.is_empty()) {
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
                for slot in &mut self.tabs {
                    slot.tab.page_mut().set_device_scale(scale_factor as f32);
                }
                self.window.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let page = self.page_mut();
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
                // A focused text field gets keys first (including repeats).
                let key = &event.logical_key;
                let text = event.text.as_deref();
                if self.slot().address.is_focused() {
                    let mods = self.modifiers;
                    let edit = self.slot_mut().address.key(key, text, mods);
                    if self.apply_address_edit(edit) {
                        return Ok(Flow::Continue);
                    }
                } else if self.find_focused() && self.find_key(key, text) {
                    return Ok(Flow::Continue);
                }
                let command = commands::shortcut(key, self.modifiers);
                // Tab and window shortcuts stay with the browser, as in
                // other browsers; everything else goes to the page first.
                let reserved = matches!(
                    command,
                    Some(
                        Command::NewTab
                            | Command::CloseTab
                            | Command::NextTab
                            | Command::PreviousTab
                            | Command::SelectTab(_)
                            | Command::Quit
                            | Command::FocusAddress
                    )
                );
                if !reserved
                    && !self.slot().address.is_focused()
                    && !self.find_focused()
                    && self.page_key(key, text)
                {
                    return Ok(Flow::Continue);
                }
                if let Some(command) = command {
                    let repeatable = matches!(
                        command,
                        Command::ScrollLines(_)
                            | Command::ScrollPages(_)
                            | Command::ScrollToTop
                            | Command::ScrollToBottom
                            | Command::FindNext
                            | Command::FindPrevious
                    );
                    if !event.repeat || repeatable {
                        return Ok(self.execute(command));
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Released
                    && !self.slot().address.is_focused()
                    && !self.find_focused() =>
            {
                if let Some(key) = self.to_page_key(&event.logical_key, event.text.as_deref()) {
                    self.slot_mut().tab.key_up(&key);
                    self.window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                // Saturating float-to-int casts; positions outside i32 are meaningless anyway.
                self.set_cursor(Some(Point::new(position.x as i32, position.y as i32)));
            }
            WindowEvent::CursorLeft { .. } => self.set_cursor(None),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => match button {
                MouseButton::Left => return Ok(self.click(false)),
                MouseButton::Middle => return Ok(self.click(true)),
                MouseButton::Back => return Ok(self.execute(Command::Back)),
                MouseButton::Forward => return Ok(self.execute(Command::Forward)),
                _ => {}
            },
            _ => {}
        }
        Ok(Flow::Continue)
    }

    /// The page's view of a key press. Shortcut chords (Ctrl/Cmd, and Alt
    /// outside macOS) arrive as named keys, never as typed text.
    fn to_page_key(&self, key: &Key, text: Option<&str>) -> Option<PageKey> {
        let mods = self.modifiers;
        let chord = mods.control_key()
            || mods.super_key()
            || (mods.alt_key() && !cfg!(target_os = "macos"));
        Some(match key {
            Key::Named(NamedKey::Enter) => PageKey::Enter,
            Key::Named(NamedKey::Backspace) => PageKey::Backspace,
            Key::Named(NamedKey::Delete) => PageKey::Delete,
            Key::Named(NamedKey::ArrowLeft) => PageKey::Left,
            Key::Named(NamedKey::ArrowRight) => PageKey::Right,
            Key::Named(NamedKey::Home) => PageKey::Home,
            Key::Named(NamedKey::End) => PageKey::End,
            Key::Named(NamedKey::Tab) if !chord => PageKey::Tab {
                shift: mods.shift_key(),
            },
            Key::Named(NamedKey::Escape) => PageKey::Escape,
            Key::Named(NamedKey::Space) if !chord => PageKey::Text(" ".into()),
            Key::Named(named) => PageKey::Other(format!("{named:?}")),
            Key::Character(c) if chord => PageKey::Other(c.to_string()),
            Key::Character(c) => PageKey::Text(text.unwrap_or(c).to_owned()),
            _ => return None,
        })
    }

    /// Offers a key press to the page. Returns true if the page used it.
    fn page_key(&mut self, key: &Key, text: Option<&str>) -> bool {
        let primary = if cfg!(target_os = "macos") {
            self.modifiers.super_key()
        } else {
            self.modifiers.control_key()
        };
        // Paste into a focused page text field.
        let paste = primary && matches!(key, Key::Character(c) if c.eq_ignore_ascii_case("v"));
        let page_key = if paste && self.page_mut().focused().is_some() {
            match self.clipboard_text() {
                Some(t) => PageKey::Text(t.replace(['\r', '\n'], " ")),
                None => return false,
            }
        } else {
            match self.to_page_key(key, text) {
                Some(k) => k,
                None => return false,
            }
        };
        let out = self.slot_mut().tab.key_down(&page_key);
        if out.consumed || out.navigate.is_some() {
            self.sync_chrome();
            self.window.request_redraw();
            return true;
        }
        false
    }

    /// Applies an address-bar edit. Returns true if the key was consumed.
    fn apply_address_edit(&mut self, edit: Edit) -> bool {
        match edit {
            Edit::Changed | Edit::Cancel => self.window.request_redraw(),
            Edit::Submit(input) => {
                if let Err(e) = self.slot_mut().tab.navigate(&input) {
                    tracing::warn!(error = %e, "navigation rejected");
                }
                self.sync_chrome();
            }
            Edit::Copy(text) => self.set_clipboard(text),
            Edit::Cut(text) => {
                self.set_clipboard(text);
                self.window.request_redraw();
            }
            Edit::Paste => {
                if let Some(text) = self.clipboard_text() {
                    self.slot_mut().address.insert(&text);
                    self.window.request_redraw();
                }
            }
            Edit::Ignored => return false,
        }
        true
    }

    fn find_focused(&self) -> bool {
        self.slot()
            .find
            .as_ref()
            .is_some_and(|f| f.input.is_focused())
    }

    /// Handles a key in the focused find field. Returns true if consumed.
    fn find_key(&mut self, key: &Key, text: Option<&str>) -> bool {
        match key {
            Key::Named(NamedKey::Enter) => {
                self.find_step(!self.modifiers.shift_key());
                return true;
            }
            Key::Named(NamedKey::Escape) => {
                self.close_find();
                return true;
            }
            _ => {}
        }
        let mods = self.modifiers;
        let Some(find) = self.slot_mut().find.as_mut() else {
            return false;
        };
        match find.input.key(key, text, mods) {
            Edit::Changed => {
                self.slot_mut().reveal_match = true;
                self.window.request_redraw();
            }
            Edit::Copy(text) => self.set_clipboard(text),
            Edit::Cut(text) => {
                self.set_clipboard(text);
                self.window.request_redraw();
            }
            Edit::Paste => {
                if let Some(text) = self.clipboard_text() {
                    let slot = self.slot_mut();
                    if let Some(find) = slot.find.as_mut() {
                        find.input.insert(&text);
                        slot.reveal_match = true;
                    }
                    self.window.request_redraw();
                }
            }
            // Enter and Escape were handled above.
            Edit::Submit(_) | Edit::Cancel => {}
            Edit::Ignored => return false,
        }
        true
    }

    fn open_find(&mut self) {
        let slot = self.slot_mut();
        slot.address.blur();
        match &mut slot.find {
            Some(find) => find.input.focus(),
            None => slot.find = Some(FindState::new()),
        }
        slot.reveal_match = true;
        self.window.request_redraw();
    }

    fn close_find(&mut self) {
        self.slot_mut().find = None;
        self.find_bar = None;
        self.window.request_redraw();
    }

    fn find_step(&mut self, forward: bool) {
        let slot = self.slot_mut();
        match &mut slot.find {
            Some(find) => {
                find.step(forward);
                slot.reveal_match = true;
                self.window.request_redraw();
            }
            None => self.open_find(),
        }
    }

    fn clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            match arboard::Clipboard::new() {
                Ok(c) => self.clipboard = Some(c),
                Err(e) => tracing::warn!(error = %e, "clipboard unavailable"),
            }
        }
        self.clipboard.as_mut()
    }

    fn clipboard_text(&mut self) -> Option<String> {
        self.clipboard().and_then(|c| c.get_text().ok())
    }

    fn set_clipboard(&mut self, text: String) {
        if let Some(Err(e)) = self.clipboard().map(|c| c.set_text(text)) {
            tracing::warn!(error = %e, "clipboard write failed");
        }
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
            Command::ScrollLines(n) => self.page_mut().scroll_by(n as f32 * 40.0),
            Command::ScrollPages(n) => {
                let page = self.page_mut();
                // Keep a little overlap between pages, as browsers do.
                let page_height = page.css_viewport().1 * 0.875;
                page.scroll_by(n as f32 * page_height)
            }
            Command::ScrollToTop => self.page_mut().scroll_to(0.0),
            Command::ScrollToBottom => self.page_mut().scroll_to(f32::MAX),
            Command::FocusAddress => {
                let slot = self.slot_mut();
                if let Some(find) = slot.find.as_mut() {
                    find.input.blur();
                }
                slot.address.focus();
                false
            }
            Command::Back => self.slot_mut().tab.back(),
            Command::Forward => self.slot_mut().tab.forward(),
            Command::Reload => self.slot_mut().tab.reload(),
            Command::Stop => {
                // Escape closes an open find bar unless a load can be stopped.
                let loading = matches!(self.slot().tab.state(), NavState::Loading { .. });
                if self.slot().find.is_some() && !loading {
                    self.close_find();
                } else {
                    self.slot_mut().tab.stop();
                }
                true
            }
            Command::NewTab => {
                self.open_tab(None, true);
                false
            }
            Command::CloseTab => return self.close_tab(self.active),
            Command::NextTab => {
                self.select((self.active + 1) % self.tabs.len());
                false
            }
            Command::PreviousTab => {
                self.select((self.active + self.tabs.len() - 1) % self.tabs.len());
                false
            }
            Command::SelectTab(i) => {
                self.select(i.min(self.tabs.len() - 1));
                false
            }
            Command::Find => {
                self.open_find();
                false
            }
            Command::FindNext => {
                self.find_step(true);
                false
            }
            Command::FindPrevious => {
                self.find_step(false);
                false
            }
        };
        if navigated {
            self.sync_chrome();
        }
        self.window.request_redraw();
        Flow::Continue
    }

    fn content_point(&self, point: Point) -> Option<(f32, f32)> {
        let top = self.toolbar.bottom() as i32;
        (point.y >= top).then(|| (point.x as f32, (point.y - top) as f32))
    }

    fn hover_at(&self, point: Point) -> Option<Hover> {
        if let Some(hit) = self.strip.hit_test(point) {
            return (hit != TabHit::Background).then_some(Hover::Tab(hit));
        }
        if let Some(ToolbarHit::Button(button)) = self.toolbar.hit_test(point) {
            return Some(Hover::Toolbar(button));
        }
        match self.find_bar.as_ref().and_then(|bar| bar.hit_test(point)) {
            Some(FindHit::Button(button)) => Some(Hover::Find(button)),
            _ => None,
        }
    }

    fn over_chrome(&self, point: Point) -> bool {
        self.strip.hit_test(point).is_some()
            || self.toolbar.hit_test(point).is_some()
            || self
                .find_bar
                .as_ref()
                .is_some_and(|b| b.hit_test(point).is_some())
    }

    fn set_cursor(&mut self, cursor: Option<Point>) {
        self.cursor = cursor;
        let hovered = cursor.and_then(|p| self.hover_at(p));
        if hovered != self.hovered {
            self.hovered = hovered;
            self.window.request_redraw();
        }
        let content = cursor
            .filter(|&p| !self.over_chrome(p))
            .and_then(|p| self.content_point(p));
        let over_link = content.is_some_and(|(x, y)| self.page_mut().link_at(x, y).is_some());
        if over_link != self.over_link {
            self.over_link = over_link;
            self.window.set_cursor(if over_link {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            });
        }
    }

    /// A left (or middle, `middle == true`) click at the cursor.
    fn click(&mut self, middle: bool) -> Flow {
        let Some(point) = self.cursor else {
            return Flow::Continue;
        };
        if let Some(hit) = self.strip.hit_test(point) {
            return match hit {
                TabHit::Close(i) => self.close_tab(i),
                TabHit::Tab(i) if middle => self.close_tab(i),
                TabHit::Tab(i) => {
                    self.select(i);
                    Flow::Continue
                }
                TabHit::NewTab if !middle => self.execute(Command::NewTab),
                TabHit::NewTab | TabHit::Background => Flow::Continue,
            };
        }
        if self.over_chrome(point) && middle {
            return Flow::Continue;
        }
        if middle {
            // Middle-click opens links in a background tab.
            return self.click_link(point, true);
        }
        match self.toolbar.hit_test(point) {
            Some(ToolbarHit::Button(button)) => {
                let command = match button {
                    ToolbarButton::Back => Command::Back,
                    ToolbarButton::Forward => Command::Forward,
                    ToolbarButton::Reload
                        if matches!(self.slot().tab.state(), NavState::Loading { .. }) =>
                    {
                        Command::Stop
                    }
                    ToolbarButton::Reload => Command::Reload,
                };
                return self.execute(command);
            }
            Some(ToolbarHit::AddressField) => {
                let slot = self.slot_mut();
                if let Some(find) = slot.find.as_mut() {
                    find.input.blur();
                }
                slot.address.focus();
                self.window.request_redraw();
                return Flow::Continue;
            }
            Some(ToolbarHit::Background) => return Flow::Continue,
            None => {}
        }
        let find_hit = self.find_bar.as_ref().and_then(|bar| bar.hit_test(point));
        if let Some(hit) = find_hit {
            match hit {
                FindHit::Button(FindButton::Previous) => self.find_step(false),
                FindHit::Button(FindButton::Next) => self.find_step(true),
                FindHit::Button(FindButton::Close) => self.close_find(),
                FindHit::Field | FindHit::Background => {
                    let slot = self.slot_mut();
                    slot.address.blur();
                    if let Some(find) = slot.find.as_mut()
                        && !find.input.is_focused()
                    {
                        find.input.focus();
                    }
                    self.window.request_redraw();
                }
            }
            return Flow::Continue;
        }
        // A click in the page takes focus from the browser's text fields.
        let slot = self.slot_mut();
        let mut blurred = false;
        if slot.address.is_focused() {
            slot.address.blur();
            blurred = true;
        }
        if let Some(find) = slot.find.as_mut()
            && find.input.is_focused()
        {
            find.input.blur();
            blurred = true;
        }
        if blurred {
            self.window.request_redraw();
        }
        let primary = if cfg!(target_os = "macos") {
            self.modifiers.super_key()
        } else {
            self.modifiers.control_key()
        };
        self.click_link(point, primary)
    }

    /// Follows the link under `point`, in a new background tab if `new_tab`.
    fn click_link(&mut self, point: Point, new_tab: bool) -> Flow {
        let Some((x, y)) = self.content_point(point) else {
            return Flow::Continue;
        };
        // Page scripts see plain left clicks first and may cancel them
        // (e.g. links handled by script) or change the page.
        if !new_tab {
            let out = self.slot_mut().tab.click(x, y);
            // Scripts or controls may have changed the title or content, or
            // a form submission started a navigation.
            self.sync_chrome();
            self.window.request_redraw();
            if out.consumed || out.navigate.is_some() {
                return Flow::Continue;
            }
        }
        let link = self.page_mut().link_at(x, y);
        let Some(url) = link else {
            return Flow::Continue;
        };
        tracing::info!(%url, new_tab, "link clicked");
        if new_tab {
            self.open_tab(Some(url), false);
        } else if self
            .slot_mut()
            .tab
            .navigate_to(url, Initiator::Page)
            .is_ok()
        {
            self.sync_chrome();
        }
        Flow::Continue
    }

    fn toolbar_view(&mut self) -> ToolbarView {
        let hovered = match self.hovered {
            Some(Hover::Toolbar(b)) => Some(b),
            _ => None,
        };
        let slot = &mut self.tabs[self.active];
        let loading = matches!(slot.tab.state(), NavState::Loading { .. });
        let (can_back, can_forward) = (slot.tab.can_go_back(), slot.tab.can_go_forward());
        let address = &slot.address;
        let text = address.text().to_owned();
        let caret = address.is_focused().then(|| address.cursor());
        let all_selected = address.is_all_selected();
        let fonts = slot.tab.page_mut().fonts();
        let reload_icon = if loading { "\u{2715}" } else { "\u{21BB}" };
        ToolbarView {
            hovered,
            can_back,
            can_forward,
            loading,
            address: shape_ui_text(fonts, &text, UI_FONT_SIZE),
            caret,
            all_selected,
            icons: [
                shape_ui_text(fonts, "\u{2190}", UI_FONT_SIZE + 2.0),
                shape_ui_text(fonts, "\u{2192}", UI_FONT_SIZE + 2.0),
                shape_ui_text(fonts, reload_icon, UI_FONT_SIZE + 2.0),
            ],
        }
    }

    fn tab_strip_view(&mut self) -> TabStripView {
        let labels: Vec<(String, bool)> = self
            .tabs
            .iter()
            .map(|s| {
                (
                    tab_title(&s.tab),
                    matches!(s.tab.state(), NavState::Loading { .. }),
                )
            })
            .collect();
        let hovered = match self.hovered {
            Some(Hover::Tab(hit)) => Some(hit),
            _ => None,
        };
        let active = self.active;
        let fonts = self.page_mut().fonts();
        TabStripView {
            tabs: labels
                .into_iter()
                .map(|(title, loading)| TabLabel {
                    title: shape_ui_text(fonts, &title, UI_FONT_SIZE - 1.0),
                    loading,
                })
                .collect(),
            active,
            hovered,
            close_icon: shape_ui_text(fonts, "\u{2715}", UI_FONT_SIZE - 3.0),
            new_icon: shape_ui_text(fonts, "+", UI_FONT_SIZE + 4.0),
        }
    }

    /// Runs (or reuses) the active tab's find search and reveals the current
    /// match if asked. Expects the page viewport to be current.
    fn update_find(&mut self) -> Option<FindBarView> {
        let hovered = match self.hovered {
            Some(Hover::Find(b)) => Some(b),
            _ => None,
        };
        let slot = &mut self.tabs[self.active];
        let find = slot.find.as_mut()?;
        let page = slot.tab.page_mut();
        page.layout();
        let generation = page.layout_count();
        if find.needs_search(generation) {
            let start = Instant::now();
            let matches = page.find(find.input.text());
            tracing::debug!(
                matches = matches.len(),
                ms = start.elapsed().as_secs_f64() * 1000.0,
                "find"
            );
            find.set_matches(matches, generation, page.scroll_y());
        }
        if std::mem::take(&mut slot.reveal_match)
            && let Some(rect) = find.current_rect()
            && let Some(y) = scroll_to_reveal(rect, page.scroll_y(), page.css_viewport().1)
        {
            page.scroll_to(y);
        }
        let query = find.input.text().to_owned();
        let count = find.count_label();
        let caret = find.input.is_focused().then(|| find.input.cursor());
        let all_selected = find.input.is_all_selected();
        let no_match = !query.is_empty() && find.matches().is_empty();
        let fonts = page.fonts();
        Some(FindBarView {
            query: shape_ui_text(fonts, &query, UI_FONT_SIZE),
            caret,
            all_selected,
            count: shape_ui_text(fonts, &count, UI_FONT_SIZE - 1.0),
            no_match,
            hovered,
            icons: [
                shape_ui_text(fonts, "\u{2191}", UI_FONT_SIZE + 1.0),
                shape_ui_text(fonts, "\u{2193}", UI_FONT_SIZE + 1.0),
                shape_ui_text(fonts, "\u{2715}", UI_FONT_SIZE - 1.0),
            ],
        })
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
        // Animation frame callbacks run right before the frame is painted.
        if self.slot_mut().tab.wants_animation_frame()
            && self.slot_mut().tab.run_animation_frame(start)
        {
            self.sync_chrome();
        }

        if self.surface_size != Some((width, height)) {
            self.surface.resize(width, height)?;
            self.surface_size = Some((width, height));
        }
        // Layout is a handful of integer ops; recomputing beats tracking width/DPI changes.
        let scale = self.window.scale_factor();
        self.strip = TabStripLayout::new(inner.width, scale, self.tabs.len());
        self.toolbar = ToolbarLayout::at(self.strip.height(), inner.width, scale);
        let window_size = Size::new(inner.width, inner.height);
        let content = frame::content_rect(window_size, self.toolbar.bottom());
        self.page_mut()
            .set_viewport(Size::new(content.width, content.height));

        let find_view = self.update_find();
        self.find_bar = find_view
            .as_ref()
            .map(|_| FindBarLayout::new(content, scale));
        let toolbar_view = self.toolbar_view();
        let strip_view = self.tab_strip_view();

        let slot = &mut self.tabs[self.active];
        let (strip, toolbar, find_bar) = (&self.strip, &self.toolbar, &self.find_bar);
        let find = slot.find.as_ref();
        frame::compose(
            &mut self.frame_list,
            window_size,
            toolbar.bottom(),
            slot.tab.page_mut(),
            self.debug_overlay,
            |out, page, content| {
                if let Some(find) = find {
                    paint_highlights(out, find, content, page.device_scale(), page.scroll_y());
                }
                strip.paint(&strip_view, out);
                toolbar.paint(&toolbar_view, out);
                if let (Some(bar), Some(view)) = (find_bar, &find_view) {
                    bar.paint(view, out);
                }
            },
        );

        let mut buffer = self.surface.buffer_mut()?;
        let mut canvas = Canvas::new(&mut buffer[..], inner.width, inner.height)?;
        let raster = rasterize(
            &mut canvas,
            &self.frame_list,
            WINDOW_BACKGROUND,
            self.tabs[self.active].tab.page_mut().glyph_rasterizer(),
        );
        self.window.pre_present_notify();
        buffer.present()?;

        // Pace further animation frames instead of redrawing flat out.
        self.next_frame = self
            .slot_mut()
            .tab
            .wants_animation_frame()
            .then(|| start + ANIMATION_FRAME);

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
