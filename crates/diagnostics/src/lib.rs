//! Structured logging and lightweight performance counters.

mod frame_stats;

pub use frame_stats::{FrameStats, FrameSummary};

use tracing_subscriber::EnvFilter;

/// Default filter when `RUST_LOG` is unset: our crates at `info`, dependencies at `warn`.
pub const DEFAULT_FILTER: &str =
    "warn,rustlite=info,rustlite_desktop=info,rustlite_browser_core=info,rustlite_renderer=info";

#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    #[error("invalid RUST_LOG filter: {0}")]
    Filter(String),
    #[error("a global tracing subscriber is already installed")]
    AlreadyInstalled,
}

/// Installs a global `tracing` subscriber writing to stderr.
///
/// The filter comes from `RUST_LOG` when set, otherwise [`DEFAULT_FILTER`].
pub fn init_logging() -> Result<(), LoggingError> {
    let filter = match std::env::var("RUST_LOG") {
        Ok(spec) => EnvFilter::try_new(&spec).map_err(|e| LoggingError::Filter(e.to_string()))?,
        Err(_) => EnvFilter::new(DEFAULT_FILTER),
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(true)
        .try_init()
        .map_err(|_| LoggingError::AlreadyInstalled)
}
