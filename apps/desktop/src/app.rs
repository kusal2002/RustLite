//! Application entry points: the windowed event loop and headless rendering.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustlite_browser_core::{Content, NavState, Page, Tab};
use rustlite_diagnostics::{FrameStats, FrameSummary};
use rustlite_net::{FetchConfig, LoadResult, Loader};
use rustlite_paint::DisplayList;
use rustlite_renderer::{CanvasError, Framebuffer, PngError, encode_png, rasterize};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

use crate::browser_window::{BrowserWindow, Flow, WindowError};
use crate::frame::{self, WINDOW_BACKGROUND};
use crate::options::Options;
use crate::toolbar::{ToolbarLayout, ToolbarView, UI_FONT_SIZE, shape_ui_text};

/// Network worker threads per process.
const LOADER_THREADS: usize = 4;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("event loop error")]
    EventLoop(#[from] winit::error::EventLoopError),
    #[error(transparent)]
    Window(#[from] WindowError),
    #[error("cannot allocate framebuffer")]
    Framebuffer(#[from] CanvasError),
    #[error("cannot read {path}")]
    Input {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is {size} bytes; the limit is {limit} bytes")]
    InputTooLarge { path: String, size: u64, limit: u64 },
    #[error("cannot write {path}")]
    Output {
        path: String,
        #[source]
        source: PngError,
    },
}

/// Events delivered to the UI thread from elsewhere.
#[derive(Debug)]
pub enum AppEvent {
    /// A network load finished on a loader thread.
    Load(LoadResult),
}

/// Result of a windowed run, for logging and smoke tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunReport {
    pub frames: Option<FrameSummary>,
}

struct App {
    options: Options,
    loader: Option<Arc<Loader>>,
    window: Option<BrowserWindow>,
    final_stats: Option<FrameStats>,
    error: Option<AppError>,
}

impl App {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: AppError) {
        tracing::error!(error = %error, "fatal error");
        self.error.get_or_insert(error);
        self.close(event_loop);
    }

    fn close(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(mut window) = self.window.take() {
            window.log_all_memory();
            self.final_stats = Some(window.stats().clone());
        }
        event_loop.exit();
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let initial = self
            .options
            .open
            .as_deref()
            .map(crate::options::command_line_input);
        match BrowserWindow::new(
            event_loop,
            self.options.size,
            self.options.debug_overlay,
            self.options.exit_after_frames,
            self.loader.clone(),
            initial,
            self.options.javascript,
        ) {
            Ok(window) => self.window = Some(window),
            Err(e) => self.fail(event_loop, e.into()),
        }
    }

    fn user_event(&mut self, _: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Load(result) => {
                if let Some(window) = self.window.as_mut() {
                    window.on_load_result(result);
                }
            }
        }
    }

    /// Runs page timers and schedules the next wake-up: the event loop
    /// sleeps until the earliest timer or animation frame, or an event.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(window) = self.window.as_mut() else {
            return;
        };
        window.tick(std::time::Instant::now());
        event_loop.set_control_flow(match window.next_deadline() {
            Some(deadline) => ControlFlow::WaitUntil(deadline),
            None => ControlFlow::Wait,
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.as_mut().filter(|w| w.id() == id) else {
            return;
        };
        match window.handle_event(event) {
            Ok(Flow::Continue) => {}
            Ok(Flow::Close) => self.close(event_loop),
            Err(e) => self.fail(event_loop, e.into()),
        }
    }
}

/// Opens the browser window and runs until it is closed.
pub fn run(options: Options) -> Result<RunReport, AppError> {
    let event_loop = EventLoop::<AppEvent>::with_user_event().build()?;
    // Sleep until the OS (or a loader thread) delivers events; redraws are
    // requested only on change.
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = Mutex::new(event_loop.create_proxy());
    let loader = Loader::new(
        LOADER_THREADS,
        &FetchConfig::default(),
        Arc::new(move |result| {
            if let Ok(proxy) = proxy.lock() {
                // Fails only after the event loop has exited; nothing to do then.
                let _ = proxy.send_event(AppEvent::Load(result));
            }
        }),
    );
    let mut app = App {
        options,
        loader: Some(Arc::new(loader)),
        window: None,
        final_stats: None,
        error: None,
    };
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.error {
        return Err(error);
    }
    let frames = app.final_stats.as_ref().and_then(FrameStats::summary);
    if let Some(s) = frames {
        tracing::info!(
            frames = s.total_frames,
            mean_ms = ms(s.mean),
            p95_ms = ms(s.p95),
            max_ms = ms(s.max),
            "frame times (last {} frames)",
            s.samples
        );
    }
    Ok(RunReport { frames })
}

/// Result of a headless render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadlessReport {
    pub display_items: usize,
    pub raster_time: Duration,
}

/// Headless renders let page timers due this soon after load run first
/// (`setTimeout(f, 0)`, short animations' first steps), as a user would see.
const HEADLESS_TIMER_WINDOW: Duration = Duration::from_millis(500);

/// Runs page timers due within [`HEADLESS_TIMER_WINDOW`] and one animation
/// frame, delivering any loads the scripts start meanwhile.
fn settle_timers(tab: &mut Tab, rx: &std::sync::mpsc::Receiver<rustlite_net::LoadResult>) {
    let window_end = Instant::now() + HEADLESS_TIMER_WINDOW;
    loop {
        tab.run_timers(Instant::now());
        while let Ok(r) = rx.try_recv() {
            tab.on_load_result(r);
        }
        let next = tab.next_timer().filter(|&t| t <= window_end);
        let now = Instant::now();
        if (next.is_none() && tab.pending_subresources() == 0) || now >= window_end {
            break;
        }
        let wait = next
            .map_or(Duration::from_millis(50), |t| {
                t.saturating_duration_since(now)
            })
            .min(Duration::from_millis(50));
        if let Ok(r) = rx.recv_timeout(wait) {
            tab.on_load_result(r);
        }
    }
    if tab.wants_animation_frame() {
        tab.run_animation_frame(Instant::now());
    }
}

/// How long headless rendering waits for a page and its subresources.
const HEADLESS_LOAD_TIMEOUT: Duration = Duration::from_secs(20);

/// Renders one frame at `options.size` (scale factor 1.0) without a window and
/// writes it to `path` as PNG. Loads `options.open` (a file path or URL) first,
/// waiting for the document and its subresources.
pub fn render_headless(options: &Options, path: &Path) -> Result<HeadlessReport, AppError> {
    let size = options.size;
    let toolbar = ToolbarLayout::new(size.width, 1.0);
    let mut page = Page::new(frame::content_size(size, &toolbar));
    page.set_javascript_enabled(options.javascript);
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let loader = Loader::new(
        LOADER_THREADS,
        &FetchConfig::default(),
        Arc::new(move |r| {
            if let Ok(tx) = tx.lock() {
                let _ = tx.send(r);
            }
        }),
    );
    let mut tab = Tab::new(page, Some(Arc::new(loader)));
    if let Some(input) = &options.open {
        if let Err(e) = tab.navigate(&crate::options::command_line_input(input)) {
            tracing::warn!(error = %e, "navigation rejected");
        }
        let deadline = Instant::now() + HEADLESS_LOAD_TIMEOUT;
        while (tab.state() != NavState::Idle || tab.pending_subresources() > 0)
            && Instant::now() < deadline
        {
            if let Ok(r) = rx.recv_timeout(Duration::from_millis(50)) {
                tab.on_load_result(r);
            }
        }
        if tab.state() != NavState::Idle {
            tracing::warn!("page still loading at the headless deadline; rendering what arrived");
        }
        settle_timers(&mut tab, &rx);
    }

    let url = tab
        .url()
        .map(|u| u.to_string())
        .filter(|u| u != "about:blank")
        .unwrap_or_default();
    let fonts = tab.page_mut().fonts();
    let view = ToolbarView {
        can_back: false,
        address: shape_ui_text(fonts, &url, UI_FONT_SIZE),
        icons: [
            shape_ui_text(fonts, "\u{2190}", UI_FONT_SIZE + 2.0),
            shape_ui_text(fonts, "\u{2192}", UI_FONT_SIZE + 2.0),
            shape_ui_text(fonts, "\u{21BB}", UI_FONT_SIZE + 2.0),
        ],
        ..ToolbarView::default()
    };

    let mut list = DisplayList::new();
    let start = Instant::now();
    frame::compose(
        &mut list,
        size,
        toolbar.bottom(),
        tab.page_mut(),
        options.debug_overlay,
        |out, _, _| toolbar.paint(&view, out),
    );
    let mut fb = Framebuffer::new(size.width, size.height)?;
    rasterize(
        &mut fb.canvas(),
        &list,
        WINDOW_BACKGROUND,
        tab.page_mut().glyph_rasterizer(),
    );
    let raster_time = start.elapsed();

    let output_error = |source: PngError| AppError::Output {
        path: path.display().to_string(),
        source,
    };
    let file = File::create(path).map_err(|e| output_error(e.into()))?;
    let mut writer = BufWriter::new(file);
    encode_png(&fb, &mut writer).map_err(output_error)?;
    writer.flush().map_err(|e| output_error(e.into()))?;

    tracing::info!(
        path = %path.display(),
        items = list.len(),
        raster_ms = ms(raster_time),
        "headless frame written"
    );
    tracing::info!(memory = %tab.page().memory_report(), "page memory");
    Ok(HeadlessReport {
        display_items: list.len(),
        raster_time,
    })
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Largest local HTML file the `--dump-*` options will read.
const MAX_DUMP_INPUT: u64 = 64 * 1024 * 1024;

/// Parses a local HTML file (named explicitly on the command line) and returns
/// its DOM in html5lib tree format.
pub fn dump_dom(path: &Path) -> Result<String, AppError> {
    let page = load_local_file(path, rustlite_paint::Size::new(0, 0))?;
    let Content::Html(doc) = page.content() else {
        unreachable!("load_html always produces HTML content")
    };
    Ok(rustlite_dom::dump_tree(&doc.dom, doc.dom.root()))
}

/// Loads a local HTML file and returns the computed styles of every element
/// (the same format as the style oracle), using `viewport` for viewport units.
pub fn dump_style(path: &Path, viewport: rustlite_paint::Size) -> Result<String, AppError> {
    let mut page = load_local_file(path, viewport)?;
    let (dom, styles) = page
        .computed_styles()
        .expect("load_html always produces HTML content");
    Ok(rustlite_style::dump_styles(
        dom,
        styles,
        rustlite_style::DEFAULT_DUMP_PROPERTIES,
    ))
}

/// Loads a local HTML file and returns every element's border box from layout
/// (the geometry oracle's format), in a `viewport`-sized viewport.
pub fn dump_layout(path: &Path, viewport: rustlite_paint::Size) -> Result<String, AppError> {
    let mut page = load_local_file(path, viewport)?;
    let (dom, tree) = page
        .layout()
        .expect("load_html always produces HTML content");
    Ok(rustlite_layout::dump_geometry(dom, tree))
}

/// Reads a local HTML file named explicitly on the command line into a page,
/// with its stylesheets and images (same directory policy as navigation).
fn load_local_file(path: &Path, viewport: rustlite_paint::Size) -> Result<Page, AppError> {
    let display = path.display().to_string();
    let input_error = |source| AppError::Input {
        path: display.clone(),
        source,
    };
    let size = std::fs::metadata(path).map_err(input_error)?.len();
    if size > MAX_DUMP_INPUT {
        return Err(AppError::InputTooLarge {
            path: display,
            size,
            limit: MAX_DUMP_INPUT,
        });
    }
    let bytes = std::fs::read(path).map_err(input_error)?;
    let source = rustlite_net::decode_html(&bytes, None).text;
    let base_dir = std::fs::canonicalize(path)
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let mut page = Page::new(viewport);
    page.load_html_with_base(format!("file:{display}"), &source, base_dir.as_deref());
    Ok(page)
}
