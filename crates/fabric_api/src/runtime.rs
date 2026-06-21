//! Dedicated Tokio runtime for reqwest (GPUI uses its own scheduler, not Tokio).

use std::future::Future;
use std::sync::OnceLock;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

pub fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .thread_name("fabric-network")
            .build()
            .expect("tokio runtime for fabric-api")
    })
}

/// Run network I/O on the dedicated Tokio runtime (safe to call from any thread).
pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    runtime().spawn(future);
}
