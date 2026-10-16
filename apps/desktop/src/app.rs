//! Application entry points: the windowed event loop and headless rendering.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use rustlite_browser_core::Page;
use rustlite_diagnostics::{FrameStats, FrameSummary};
use rustlite_paint::DisplayList;
use rustlite_renderer::{CanvasError, Framebuffer, PngError, encode_png, rasterize};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

use crate::browser_window::{BrowserWindow, Flow, WindowError};
use crate::frame::{self, ChromeState, WINDOW_BACKGROUND};
use crate::options::Options;
use crate::toolbar::ToolbarLayout;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("event loop error")]
    EventLoop(#[from] winit::error::EventLoopError),
    #[error(transparent)]
    Window(#[from] WindowError),
    #[error("cannot allocate framebuffer")]
    Framebuffer(#[from] CanvasError),
    #[error("cannot write {path}")]
    Output {
        path: String,
        #[source]
        source: PngError,
    },
}

/// Result of a windowed run, for logging and smoke tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunReport {
    pub frames: Option<FrameSummary>,
}

struct App {
    options: Options,
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
        if let Some(window) = self.window.take() {
            self.final_stats = Some(window.stats().clone());
        }
        event_loop.exit();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        match BrowserWindow::new(
            event_loop,
            self.options.size,
            self.options.debug_overlay,
            self.options.exit_after_frames,
        ) {
            Ok(window) => self.window = Some(window),
            Err(e) => self.fail(event_loop, e.into()),
        }
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
    let event_loop = EventLoop::new()?;
    // Sleep until the OS delivers events; redraws are requested only on change.
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App {
        options,
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

/// Renders one frame at `options.size` (scale factor 1.0) without a window and
/// writes it to `path` as PNG.
pub fn render_headless(options: &Options, path: &Path) -> Result<HeadlessReport, AppError> {
    let size = options.size;
    let toolbar = ToolbarLayout::new(size.width, 1.0);
    let mut page = Page::new(frame::content_size(size, &toolbar));
    let mut list = DisplayList::new();
    let chrome = ChromeState {
        debug_overlay: options.debug_overlay,
        ..ChromeState::default()
    };

    let start = Instant::now();
    frame::compose(&mut list, size, &toolbar, &mut page, chrome);
    let mut fb = Framebuffer::new(size.width, size.height)?;
    rasterize(&mut fb.canvas(), &list, WINDOW_BACKGROUND);
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
    Ok(HeadlessReport {
        display_items: list.len(),
        raster_time,
    })
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
