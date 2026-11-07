use sese_runtime_spike::{shadow::compare, storage::Storage};
use std::time::Duration;

#[test]
fn shadow_divergence_does_not_change_champion_output() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let champ = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
            )
            .unwrap(),
        )
        .unwrap();
    let shadow = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) i32.const 99))",
            )
            .unwrap(),
        )
        .unwrap();
    let result = compare(
        &mut db,
        env!("CARGO_BIN_EXE_worker"),
        &champ,
        &shadow,
        4,
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(result.champion, Some(4));
    assert_eq!(result.shadow, Some(99));
    assert_eq!(result.diverged, Some(true));
}

#[test]
fn shadow_failure_does_not_replace_champion() {
    let mut db = Storage::from_connection(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
    let champ = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) local.get 0))",
            )
            .unwrap(),
        )
        .unwrap();
    let shadow = db
        .register_synthetic_scalar(
            &wat::parse_str(
                "(module (func (export \"evaluate\") (param i32) (result i32) unreachable))",
            )
            .unwrap(),
        )
        .unwrap();
    let result = compare(
        &mut db,
        env!("CARGO_BIN_EXE_worker"),
        &champ,
        &shadow,
        4,
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(result.champion, Some(4));
    assert!(result.shadow.is_none());
    assert!(result.shadow_error.is_some());
    assert_eq!(result.diverged, None);
}
