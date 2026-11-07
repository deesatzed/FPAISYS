use ed25519_dalek::SigningKey;
use sese_runtime_spike::attestation::{AuthoritySet, SyntheticTrustPolicy};
use sha2::{Digest, Sha256};

fn fixture() -> (AuthoritySet, serde_json::Value) {
    let keys: Vec<String> = [11, 12, 13]
        .iter()
        .map(|n| {
            SigningKey::from_bytes(&[*n; 32])
                .verifying_key()
                .to_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect()
        })
        .collect();
    let config = serde_json::json!({"schema":"sese-authority-set-v1", "authorities":[
        {"id":"source","role":"trace-source","public_key":keys[0],"revoked":false},
        {"id":"observer","role":"outcome-observer","public_key":keys[1],"revoked":false},
        {"id":"reviewer","role":"preference-reviewer","public_key":keys[2],"revoked":false}]});
    let bytes = serde_json::to_vec(&config).unwrap();
    let authorities =
        AuthoritySet::from_pinned_json(&bytes, &Sha256::digest(&bytes).into()).unwrap();
    let digest: String = authorities
        .digest()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let policy = serde_json::json!({"schema":"sese-synthetic-trust-policy-v1", "authority_config_sha256":digest,
        "declarations":[{"trace_source":"source","outcome_observer":"observer","preference_reviewer":"reviewer",
        "operator":"synthetic operator","observer_relationship_basis":"separate synthetic observer",
        "reviewer_relationship_basis":"separate synthetic reviewer"}]});
    (authorities, policy)
}

fn parse(
    value: &serde_json::Value,
    authorities: &AuthoritySet,
) -> Result<SyntheticTrustPolicy, String> {
    let bytes = serde_json::to_vec(value).unwrap();
    SyntheticTrustPolicy::from_pinned_json(&bytes, &Sha256::digest(&bytes).into(), authorities)
}

#[test]
fn pinned_policy_records_explicit_role_relationships_without_proving_them() {
    let (authorities, policy) = fixture();
    let bytes = serde_json::to_vec(&policy).unwrap();
    assert!(SyntheticTrustPolicy::from_pinned_json(&bytes, &[0; 32], &authorities).is_err());
    let policy = parse(&policy, &authorities).unwrap();
    assert!(!policy.training_export_allowed());
    assert!(policy.declares("source", "observer", None));
    assert!(policy.declares("source", "observer", Some("reviewer")));
    assert!(!policy.declares("source", "observer", Some("other")));
    assert!(!policy.declares("source", "other", None));
}

#[test]
fn policy_rejects_ambiguous_unbound_or_unreviewed_declarations() {
    let (authorities, policy) = fixture();
    let mut invalid_flag = policy.clone();
    invalid_flag["training_export_allowed"] = serde_json::json!("true");
    assert!(parse(&invalid_flag, &authorities).is_err());
    for (path, value) in [
        (
            "authority_config_sha256",
            serde_json::json!("00".repeat(32)),
        ),
        ("schema", serde_json::json!("other")),
    ] {
        let mut invalid = policy.clone();
        invalid[path] = value;
        assert!(parse(&invalid, &authorities).is_err());
    }
    for (field, value) in [
        ("trace_source", serde_json::json!("observer")),
        ("outcome_observer", serde_json::json!("missing")),
        ("preference_reviewer", serde_json::json!("source")),
        ("observer_relationship_basis", serde_json::json!(" ")),
        ("reviewer_relationship_basis", serde_json::json!(null)),
        ("operator", serde_json::json!("")),
    ] {
        let mut invalid = policy.clone();
        invalid["declarations"][0][field] = value;
        assert!(parse(&invalid, &authorities).is_err(), "{field}");
    }
    let mut duplicate = policy.clone();
    duplicate["declarations"]
        .as_array_mut()
        .unwrap()
        .push(policy["declarations"][0].clone());
    assert!(parse(&duplicate, &authorities).is_err());
    let mut unknown = policy.clone();
    unknown["declarations"][0]["unknown"] = true.into();
    assert!(parse(&unknown, &authorities).is_err());
    assert!(
        SyntheticTrustPolicy::from_pinned_json(&vec![b' '; 65_537], &[0; 32], &authorities)
            .is_err()
    );
}
