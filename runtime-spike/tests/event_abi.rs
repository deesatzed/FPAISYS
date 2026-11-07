use sese_runtime_spike::event_abi::{evaluate_event, Decision, Event, Window};
fn input() -> (Window, Event) {
    (
        Window {
            count: 2,
            mean: 3.5,
            p99_us: 9,
            last_error: 4,
        },
        Event {
            timestamp_ns: 8,
            entity_id: 42,
            metric: 6.25,
            flags: 5,
        },
    )
}
#[test]
fn guest_receives_typed_snapshot_and_returns_valid_verdict() {
    let code=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 local.get 6 local.get 7))").unwrap();
    let (window, event) = input();
    let verdict = evaluate_event(&code, &window, &event).unwrap();
    assert_eq!(verdict.decision, Decision::Throttle);
    assert_eq!(verdict.score, 6.25);
    assert_eq!(verdict.reason, 5);
}

#[test]
fn fixed_window_probe_uses_prior_count_mean_p99_and_error() {
    let code = sese_runtime_spike::builtin_event::artifact("window-probe").unwrap();
    let (window, event) = input();
    let result = evaluate_event(&code, &window, &event).unwrap();
    assert_eq!(result.decision, Decision::Allow);
    assert_eq!(result.score, 14.5);
    assert_eq!(result.reason, 4);
    let different = Window {
        count: 4,
        mean: 2.0,
        p99_us: 3,
        last_error: 8,
    };
    let result = evaluate_event(&code, &different, &event).unwrap();
    assert_eq!(result.score, 9.0);
    assert_eq!(result.reason, 8);
}
#[test]
fn invalid_values_and_verdicts_fail_closed() {
    let (window, mut event) = input();
    let code=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 9 f64.const 0 i32.const 0))").unwrap();
    assert!(evaluate_event(&code, &window, &event).is_err());
    event.metric = f64::NAN;
    assert!(evaluate_event(&code, &window, &event).is_err());
    let nan=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 f64.const nan i32.const 0))").unwrap();
    event.metric = 1.0;
    assert!(evaluate_event(&nan, &window, &event).is_err());
}
#[test]
fn unsigned_fields_preserve_full_width() {
    let code=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) local.get 0 i64.const -1 i64.eq local.get 2 i64.const -1 i64.eq i32.and local.get 3 i32.const -1 i32.eq i32.and local.get 4 i64.const -1 i64.eq i32.and local.get 5 i64.const -1 i64.eq i32.and local.get 7 i32.const -1 i32.eq i32.and if (result i32) i32.const 3 else i32.const 9 end local.get 1 local.get 3))").unwrap();
    let window = Window {
        count: u64::MAX,
        mean: 1.25,
        p99_us: u64::MAX,
        last_error: u32::MAX,
    };
    let event = Event {
        timestamp_ns: u64::MAX,
        entity_id: u64::MAX,
        metric: 0.0,
        flags: u32::MAX,
    };
    let verdict = evaluate_event(&code, &window, &event).unwrap();
    assert_eq!(verdict.decision, Decision::Escalate);
    assert_eq!(verdict.score, 1.25);
    assert_eq!(verdict.reason, u32::MAX);
}
#[test]
fn event_guest_cannot_import_or_loop_unbounded() {
    let (window, event) = input();
    let imported=wat::parse_str("(module (import \"host\" \"io\" (func)) (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 0 f64.const 0 i32.const 0))").unwrap();
    let looping=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) (loop br 0) i32.const 0 f64.const 0 i32.const 0))").unwrap();
    assert!(evaluate_event(&imported, &window, &event).is_err());
    assert!(evaluate_event(&looping, &window, &event).is_err());
}
#[test]
fn supervised_event_worker_round_trips_and_rejects_invalid_guest() {
    let (window, event) = input();
    let code=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 1 local.get 6 local.get 7))").unwrap();
    let result = sese_runtime_spike::event_abi::supervise_event(
        env!("CARGO_BIN_EXE_event-worker"),
        &code,
        &window,
        &event,
        std::time::Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(result.decision, Decision::Throttle);
    assert_eq!(result.score, 6.25);
    assert_eq!(result.reason, 5);
    let bad=wat::parse_str("(module (func (export \"evaluate_event_v1\") (param i64 f64 i64 i32 i64 i64 f64 i32) (result i32 f64 i32) i32.const 9 f64.const 0 i32.const 0))").unwrap();
    assert!(sese_runtime_spike::event_abi::supervise_event(
        env!("CARGO_BIN_EXE_event-worker"),
        &bad,
        &window,
        &event,
        std::time::Duration::from_secs(2)
    )
    .is_err());
}
#[test]
fn event_protocol_rejects_missing_response_and_truncated_request() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let (window, event) = input();
    assert!(sese_runtime_spike::event_abi::supervise_event(
        "/usr/bin/true",
        b"",
        &window,
        &event,
        std::time::Duration::from_secs(2)
    )
    .is_err());
    let mut child = Command::new(env!("CARGO_BIN_EXE_event-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&[0; 55]).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
