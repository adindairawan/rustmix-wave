//! Reusable short-lived worker boundary.
//!
//! Heavy operations receive named stack budgets and return compact heap-owned
//! results to the main hardware-orchestration loop. Panel SPI ownership stays
//! on the main task.

use core::fmt::{self, Display};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub enum NamedWorkerError<E> {
    Start(std::io::Error),
    Panicked,
    Operation(E),
}

impl<E: Display> Display for NamedWorkerError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start(error) => write!(formatter, "worker start failed: {error}"),
            Self::Panicked => formatter.write_str("worker panicked"),
            Self::Operation(error) => Display::fmt(error, formatter),
        }
    }
}

impl<E: Display + fmt::Debug> std::error::Error for NamedWorkerError<E> {}

pub fn run_named_worker<T, E, F>(
    name: &'static str,
    stack_bytes: usize,
    task: F,
) -> Result<T, NamedWorkerError<E>>
where
    T: Send + 'static,
    E: Display + Send + 'static,
    F: FnOnce() -> Result<T, E> + Send + 'static,
{
    log::info!(
        "rustmix-wave=worker-boundary name={name} status=starting stack-bytes={stack_bytes}"
    );
    crate::runtime_memory::log_runtime_memory(&format!("before-worker-{name}"));

    let task_slot = Arc::new(Mutex::new(Some(task)));
    let task_for_worker = Arc::clone(&task_slot);

    let worker = std::thread::Builder::new()
        .name(name.into())
        .stack_size(stack_bytes)
        .spawn(move || {
            let task_fn = task_for_worker
                .lock()
                .unwrap()
                .take()
                .expect("worker task must be available in spawned thread");
            task_fn()
        });

    let result = match worker {
        Ok(handle) => handle.join().map_err(|_| {
            log::warn!("rustmix-wave=worker-boundary name={name} status=panicked");
            NamedWorkerError::Panicked
        })?,
        Err(error) => {
            log::warn!(
                "rustmix-wave=worker-boundary name={name} status=start-failed error={error}"
            );
            if stack_bytes > 16 * 1024 {
                return Err(NamedWorkerError::Start(error));
            }
            if let Some(task_fn) = task_slot.lock().unwrap().take() {
                log::warn!(
                    "rustmix-wave=worker-boundary name={name} action=fallback-inline"
                );
                task_fn()
            } else {
                return Err(NamedWorkerError::Start(error));
            }
        }
    };

    crate::runtime_memory::log_runtime_memory(&format!("after-worker-{name}"));
    match result {
        Ok(value) => {
            log::info!("rustmix-wave=worker-boundary name={name} status=completed");
            Ok(value)
        }
        Err(error) => {
            log::warn!("rustmix-wave=worker-boundary name={name} status=failed error={error}");
            Err(NamedWorkerError::Operation(error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::run_named_worker;

    #[test]
    fn returns_compact_result_from_named_short_lived_worker() {
        let result = run_named_worker("unit-worker", 16 * 1024, || Ok::<_, String>(42)).unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn fails_gracefully_if_large_stack_spawn_fails() {
        let result =
            run_named_worker("impossible-worker", usize::MAX / 2, || Ok::<_, String>(999));
        assert!(matches!(result, Err(super::NamedWorkerError::Start(_))));
    }
}
