use sese_runtime_spike::reward_rule::SyntheticRewardRule;
use sha2::{Digest, Sha256};

fn rule(value: &serde_json::Value) -> Result<SyntheticRewardRule, String> {
    let bytes = serde_json::to_vec(value).unwrap();
    SyntheticRewardRule::from_pinned_json(&bytes, &Sha256::digest(&bytes).into())
}

fn fixture() -> serde_json::Value {
    serde_json::json!({"schema":"sese-synthetic-reward-rule-v1", "version":"outcome-sign-v1",
        "validated":1,"falsified":-1,"actor":"synthetic operator","source":"local experiment"})
}

#[test]
fn rule_is_pinned_and_maps_only_resolved_statuses() {
    let value = fixture();
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(SyntheticRewardRule::from_pinned_json(&bytes, &[0; 32]).is_err());
    let rule = rule(&value).unwrap();
    assert_eq!(rule.reward("validated").unwrap(), 1);
    assert_eq!(rule.reward("falsified").unwrap(), -1);
    assert!(rule.reward("pending").is_err());
    assert!(rule.reward("unknown").is_err());
    let expected: [u8; 32] = Sha256::digest(&bytes).into();
    assert_eq!(rule.digest(), expected);
}

#[test]
fn rule_rejects_unsupported_versions_mappings_and_provenance() {
    let good = fixture();
    for (key, value) in [
        ("version", serde_json::json!("unknown")),
        ("validated", serde_json::json!(-1)),
        ("falsified", serde_json::json!(1)),
        ("actor", serde_json::json!(" ")),
        ("source", serde_json::json!("")),
        ("schema", serde_json::json!("other")),
        ("unknown", serde_json::json!(true)),
    ] {
        let mut changed = good.clone();
        changed[key] = value;
        assert!(rule(&changed).is_err(), "{key}");
    }
    assert!(SyntheticRewardRule::from_pinned_json(&vec![0; 65_537], &[0; 32]).is_err());
}
