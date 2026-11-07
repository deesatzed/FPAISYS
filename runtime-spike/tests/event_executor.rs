use sese_runtime_spike::{
    event_abi::{Event, Window},
    event_executor::LocalEventExecutor,
};
use std::time::Duration;

#[test]
fn local_executor_runs_the_actual_worker_and_detects_changed_worker() {
    let directory = std::env::temp_dir().join(format!(
        "sese-executor-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let worker = directory.join("worker");
    std::fs::copy(env!("CARGO_BIN_EXE_event-worker"), &worker).unwrap();
    let executor = LocalEventExecutor::capture(worker.to_str().unwrap()).unwrap();
    let artifact = sese_runtime_spike::builtin_event::artifact("throttle").unwrap();
    let verdict = executor
        .execute(
            &artifact,
            &Window {
                count: 0,
                mean: 0.0,
                p99_us: 0,
                last_error: 0,
            },
            &Event {
                timestamp_ns: 1,
                entity_id: 42,
                metric: 3.0,
                flags: 4,
            },
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(verdict.score, 3.0);
    assert!(executor.verify_unchanged().is_ok());
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&worker)
        .unwrap()
        .write_all(b"modified")
        .unwrap();
    assert!(executor.verify_unchanged().is_err());
    assert!(
        executor
            .execute(
                &artifact,
                &Window {
                    count: 0,
                    mean: 0.0,
                    p99_us: 0,
                    last_error: 0
                },
                &Event {
                    timestamp_ns: 1,
                    entity_id: 42,
                    metric: 3.0,
                    flags: 4
                },
                Duration::from_secs(2)
            )
            .is_err(),
        "a changed worker must not run under the captured identity"
    );
    std::fs::remove_dir_all(directory).unwrap();
}
