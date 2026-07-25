use std::any::Any;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Duration;

use rayon::prelude::*;

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, thiserror::Error)]
pub(crate) enum OnlineExecutionError {
    #[error("online worker pool could not be created: {0}")]
    WorkerPool(#[from] rayon::ThreadPoolBuildError),
    #[error("online HTTP client could not be created: {0}")]
    HttpClient(#[from] reqwest::Error),
}

pub(crate) struct OnlineExecutor {
    pool: rayon::ThreadPool,
    http_client: reqwest::blocking::Client,
}

impl OnlineExecutor {
    pub(crate) fn new(max_concurrency: usize) -> Result<Self, OnlineExecutionError> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(max_concurrency.max(1))
            .thread_name(|index| format!("fika-online-{index}"))
            .build()?;
        let http_client = reqwest::blocking::Client::builder()
            .connect_timeout(DEFAULT_CONNECT_TIMEOUT)
            .build()?;
        Ok(Self { pool, http_client })
    }

    pub(crate) fn map<Input, Output, Operation>(
        &self,
        inputs: Vec<Input>,
        operation: Operation,
    ) -> Vec<Result<Output, String>>
    where
        Input: Send,
        Output: Send,
        Operation: Fn(Input) -> Output + Send + Sync,
    {
        self.pool.install(|| {
            inputs
                .into_par_iter()
                .map(|input| {
                    catch_unwind(AssertUnwindSafe(|| operation(input))).map_err(panic_message)
                })
                .collect()
        })
    }

    pub(crate) fn spawn<Output, Operation, Complete>(
        &self,
        operation: Operation,
        complete: Complete,
    ) where
        Output: Send + 'static,
        Operation: FnOnce() -> Output + Send + 'static,
        Complete: FnOnce(Result<Output, String>) + Send + 'static,
    {
        self.pool.spawn(move || {
            complete(catch_unwind(AssertUnwindSafe(operation)).map_err(panic_message));
        });
    }

    pub(crate) fn http_client(&self) -> reqwest::blocking::Client {
        self.http_client.clone()
    }
}

fn panic_message(payload: Box<dyn Any + Send>) -> String {
    let detail = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic");
    format!("online worker panicked: {detail}")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;

    use super::*;

    #[test]
    fn map_limits_parallel_operations_to_the_configured_worker_count() {
        let executor = OnlineExecutor::new(2).expect("executor should initialize");
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let active_worker = Arc::clone(&active);
        let maximum_worker = Arc::clone(&maximum);

        let results = executor.map((0..8).collect(), move |value| {
            let current = active_worker.fetch_add(1, Ordering::SeqCst) + 1;
            maximum_worker.fetch_max(current, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(5));
            active_worker.fetch_sub(1, Ordering::SeqCst);
            value
        });

        assert!(results.iter().all(Result::is_ok));
        assert!(maximum.load(Ordering::SeqCst) <= 2);
    }

    #[test]
    fn map_reports_worker_panics_without_dropping_result_slots() {
        let executor = OnlineExecutor::new(2).expect("executor should initialize");

        let results = executor.map(vec![1, 2, 3], |value| {
            assert_ne!(value, 2, "forced worker failure");
            value
        });

        assert_eq!(results.len(), 3);
        assert!(results[1]
            .as_ref()
            .is_err_and(|message| message.contains("forced worker failure")));
    }
}
