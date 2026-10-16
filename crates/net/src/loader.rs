//! A bounded background loader: a fixed pool of worker threads fetching
//! requests from a bounded queue, so network I/O never blocks the UI thread
//! and a page cannot queue unbounded work.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::{FetchConfig, Fetcher, NetError, Request, Response};

/// Shared cancellation flag. Cancelling is idempotent and cheap.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// The outcome of a background load, tagged with the caller's id.
#[derive(Debug)]
pub struct LoadResult {
    pub id: u64,
    pub request: Request,
    pub result: Result<Response, NetError>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoaderError {
    #[error("the load queue is full")]
    QueueFull,
    #[error("the loader has shut down")]
    ShutDown,
}

struct Job {
    id: u64,
    request: Request,
    cancel: CancelToken,
}

/// Delivers results; called on a worker thread.
pub type Notify = Arc<dyn Fn(LoadResult) + Send + Sync>;

/// Pending jobs beyond this are refused rather than queued.
pub const QUEUE_CAPACITY: usize = 64;

pub struct Loader {
    queue: Option<SyncSender<Job>>,
    workers: Vec<JoinHandle<()>>,
}

impl std::fmt::Debug for Loader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loader")
            .field("workers", &self.workers.len())
            .finish_non_exhaustive()
    }
}

impl Loader {
    /// Starts `threads` workers (at least one). `notify` receives every
    /// result, including cancellations, on a worker thread.
    pub fn new(threads: usize, config: &FetchConfig, notify: Notify) -> Self {
        let (tx, rx) = sync_channel::<Job>(QUEUE_CAPACITY);
        let rx = Arc::new(Mutex::new(rx));
        let fetcher = Fetcher::new(config);
        let workers = (0..threads.max(1))
            .map(|i| {
                let rx: Arc<Mutex<Receiver<Job>>> = rx.clone();
                let fetcher = fetcher.clone();
                let notify = notify.clone();
                std::thread::Builder::new()
                    .name(format!("rustlite-net-{i}"))
                    .spawn(move || worker(&rx, &fetcher, &notify))
                    .expect("failed to spawn network worker thread")
            })
            .collect();
        Self {
            queue: Some(tx),
            workers,
        }
    }

    /// Queues `request`. The result arrives through `notify` with `id`.
    pub fn load(&self, id: u64, request: Request, cancel: CancelToken) -> Result<(), LoaderError> {
        let queue = self.queue.as_ref().ok_or(LoaderError::ShutDown)?;
        queue
            .try_send(Job {
                id,
                request,
                cancel,
            })
            .map_err(|e| match e {
                TrySendError::Full(_) => LoaderError::QueueFull,
                TrySendError::Disconnected(_) => LoaderError::ShutDown,
            })
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        // Closing the queue makes idle workers exit; busy ones finish their
        // current request (bounded by the fetch timeout). Workers are not
        // joined, so dropping never blocks the UI thread.
        self.queue = None;
        self.workers.clear();
    }
}

fn worker(rx: &Mutex<Receiver<Job>>, fetcher: &Fetcher, notify: &Notify) {
    loop {
        // Hold the lock only while receiving.
        let job = match rx.lock() {
            Ok(guard) => guard.recv(),
            Err(_) => return,
        };
        let Ok(job) = job else { return };
        let result = if job.cancel.is_cancelled() {
            Err(NetError::Cancelled)
        } else {
            fetcher.fetch(&job.request, &job.cancel)
        };
        tracing::debug!(id = job.id, url = %job.request.url, ok = result.is_ok(), "load finished");
        notify(LoadResult {
            id: job.id,
            request: job.request,
            result,
        });
    }
}
