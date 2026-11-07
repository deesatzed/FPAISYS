use sese_runtime_spike::event_state::EventState;

#[test]
fn metric_mean_and_measured_latency_are_separate_and_bounded() {
    let mut state = EventState::new(2).unwrap();
    state.record(-10.0, 100, 0).unwrap();
    state.record(20.0, 200, 7).unwrap();
    let snap = state.snapshot();
    assert_eq!(snap.count, 2);
    assert_eq!(snap.mean, 5.0);
    assert_eq!(snap.p99_us, 200);
    assert_eq!(snap.last_error, 7);
    state.record(30.0, 50, 0).unwrap();
    let snap = state.snapshot();
    assert_eq!(snap.count, 3);
    assert_eq!(snap.mean, 25.0);
    assert_eq!(snap.p99_us, 200);
    assert_eq!(snap.last_error, 7);
}
#[test]
fn encoded_state_round_trips_and_rejects_corruption() {
    let mut state = EventState::new(2).unwrap();
    state.record(-0.0, 11, 4).unwrap();
    state.record(2.5, 22, 0).unwrap();
    let bytes = state.encode();
    let copy = EventState::decode(&bytes).unwrap();
    assert_eq!(copy.encode(), bytes);
    let mut truncated = bytes.clone();
    truncated.pop();
    assert!(EventState::decode(&truncated).is_err());
    let mut changed = bytes.clone();
    changed[0] = 0;
    assert!(EventState::decode(&changed).is_err());
    assert!(EventState::new(0).is_err());
    assert!(state.record(f64::NAN, 5, 0).is_err());
    assert_eq!(state.encode(), bytes);
}
