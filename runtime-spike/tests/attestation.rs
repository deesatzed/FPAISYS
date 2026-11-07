use ed25519_dalek::{Signer, SigningKey};
use sese_runtime_spike::attestation::verify_ed25519;
use sese_runtime_spike::attestation::AuthoritySet;
use sese_runtime_spike::attestation::{
    attestation_message, verify_attestation, Authority, AuthorityRole,
};
use sese_runtime_spike::attestation::{
    verify_synthetic_outcome, verify_synthetic_trace, EvidenceReference, SyntheticOutcome,
    SyntheticOutcomeEvidence, SyntheticTraceEvidence,
};
use sese_runtime_spike::attestation::{verify_synthetic_preference, SyntheticPreferenceEvidence};
use sha2::{Digest, Sha256};

#[test]
fn canonical_trace_and_outcome_decode_strictly() {
    let reference = EvidenceReference {
        review_id: 7,
        replay_id: 9,
        candidate_sha256: "ab".repeat(32),
        fixture_sha256: "cd".repeat(32),
        review_ledger_hash: "ef".repeat(32),
    };
    let trace = SyntheticTraceEvidence {
        reference: reference.clone(),
        prompt: "π prompt".into(),
        response: "response".into(),
        source: "fixture".into(),
        method: "local".into(),
        captured_unix_ns: 42,
    };
    let outcome = SyntheticOutcomeEvidence {
        reference,
        outcome: SyntheticOutcome::Falsified,
        source: "fixture".into(),
        method: "local".into(),
        evidence_reference: "case".into(),
        observed_unix_ns: 43,
    };
    let trace_bytes = trace.canonical_bytes().unwrap();
    let outcome_bytes = outcome.canonical_bytes().unwrap();
    assert_eq!(
        SyntheticTraceEvidence::from_canonical_bytes(&trace_bytes)
            .unwrap()
            .canonical_bytes()
            .unwrap(),
        trace_bytes
    );
    assert_eq!(
        SyntheticOutcomeEvidence::from_canonical_bytes(&outcome_bytes)
            .unwrap()
            .canonical_bytes()
            .unwrap(),
        outcome_bytes
    );
    for bad in [
        &trace_bytes[..trace_bytes.len() - 1],
        &[trace_bytes.as_slice(), b"x"].concat(),
        &outcome_bytes,
    ] {
        assert!(SyntheticTraceEvidence::from_canonical_bytes(bad).is_err());
    }
    for bad in [
        &outcome_bytes[..outcome_bytes.len() - 1],
        &[outcome_bytes.as_slice(), b"x"].concat(),
        &trace_bytes,
    ] {
        assert!(SyntheticOutcomeEvidence::from_canonical_bytes(bad).is_err());
    }
    let mut invalid_status = outcome_bytes.clone();
    invalid_status[b"sese-synthetic-outcome-attestation-v1\0".len() + 16 + 192] = 3;
    assert!(SyntheticOutcomeEvidence::from_canonical_bytes(&invalid_status).is_err());
    let mut invalid_text = trace_bytes.clone();
    invalid_text[b"sese-synthetic-trace-attestation-v1\0".len() + 16 + 192 + 4] = 0xff;
    assert!(SyntheticTraceEvidence::from_canonical_bytes(&invalid_text).is_err());
    let mut oversized_field = trace_bytes.clone();
    let length_offset = b"sese-synthetic-trace-attestation-v1\0".len() + 16 + 192;
    oversized_field[length_offset..length_offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(SyntheticTraceEvidence::from_canonical_bytes(&oversized_field).is_err());
    let mut invalid_hash = trace_bytes.clone();
    invalid_hash[b"sese-synthetic-trace-attestation-v1\0".len() + 16] = b'G';
    assert!(SyntheticTraceEvidence::from_canonical_bytes(&invalid_hash).is_err());
}

fn bytes<const N: usize>(hex: &str) -> [u8; N] {
    assert_eq!(hex.len(), N * 2);
    let mut out = [0u8; N];
    for (i, item) in out.iter_mut().enumerate() {
        *item = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
    }
    out
}

#[test]
fn verifies_rfc8032_vector_and_rejects_tampering() {
    let key = bytes::<32>("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
    let signature = bytes::<64>(concat!(
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155",
        "5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
    ));
    assert!(verify_ed25519(&key, b"", &signature));
    assert!(!verify_ed25519(&key, b"changed", &signature));
    let mut changed = signature;
    changed[0] ^= 1;
    assert!(!verify_ed25519(&key, b"", &changed));
    assert!(!verify_ed25519(&[0; 32], b"", &signature));
}

#[test]
fn signed_preference_binds_same_prompt_pair_and_trace_identity() {
    let signer = SigningKey::from_bytes(&[19; 32]);
    let reviewer = Authority {
        id: "reviewer".into(),
        role: AuthorityRole::PreferenceReviewer,
        public_key: signer.verifying_key().to_bytes(),
        revoked: false,
    };
    let reference = EvidenceReference {
        review_id: 3,
        replay_id: 5,
        candidate_sha256: "ab".repeat(32),
        fixture_sha256: "cd".repeat(32),
        review_ledger_hash: "ef".repeat(32),
    };
    let mut preference = SyntheticPreferenceEvidence {
        reference,
        trace_id: 9,
        trace_ledger_hash: "12".repeat(32),
        prompt: "shared prompt".into(),
        preferred: "explicit preferred".into(),
        rejected: "explicit rejected".into(),
        source: "synthetic reviewer".into(),
        comparability_basis: "same prompt and task".into(),
        reviewed_unix_ns: 123,
    };
    let payload = preference.canonical_bytes().unwrap();
    assert_eq!(
        SyntheticPreferenceEvidence::from_canonical_bytes(&payload)
            .unwrap()
            .canonical_bytes()
            .unwrap(),
        payload
    );
    assert!(SyntheticPreferenceEvidence::from_canonical_bytes(
        &[payload.as_slice(), b"x"].concat()
    )
    .is_err());
    assert!(
        SyntheticPreferenceEvidence::from_canonical_bytes(&payload[..payload.len() - 1]).is_err()
    );
    let signature = signer
        .sign(&attestation_message(&reviewer.id, reviewer.role, &payload).unwrap())
        .to_bytes();
    assert!(verify_synthetic_preference(
        &reviewer,
        &preference,
        &signature
    ));
    preference.prompt.push('!');
    assert!(!verify_synthetic_preference(
        &reviewer,
        &preference,
        &signature
    ));
    preference.prompt.pop();
    preference.trace_ledger_hash = "34".repeat(32);
    assert!(!verify_synthetic_preference(
        &reviewer,
        &preference,
        &signature
    ));
    preference.trace_ledger_hash = "12".repeat(32);
    preference.rejected = preference.preferred.clone();
    assert!(preference.canonical_bytes().is_err());
    preference.rejected = "explicit rejected".into();
    preference.comparability_basis.clear();
    assert!(preference.canonical_bytes().is_err());
}

#[test]
fn authority_configuration_is_pinned_unique_bounded_and_fail_closed() {
    let key1 = SigningKey::from_bytes(&[11; 32])
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let key2 = SigningKey::from_bytes(&[12; 32])
        .verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let config = serde_json::json!({"schema":"sese-authority-set-v1","authorities":[
        {"id":"source","role":"trace-source","public_key":key1,"revoked":false},
        {"id":"observer","role":"outcome-observer","public_key":key2,"revoked":true}
    ]});
    let raw = serde_json::to_vec(&config).unwrap();
    let pin: [u8; 32] = Sha256::digest(&raw).into();
    let set = AuthoritySet::from_pinned_json(&raw, &pin).unwrap();
    assert!(set.get("source", AuthorityRole::TraceSource).is_some());
    assert!(set.get("source", AuthorityRole::OutcomeObserver).is_none());
    assert!(set
        .get("observer", AuthorityRole::OutcomeObserver)
        .is_none());
    assert!(AuthoritySet::from_pinned_json(&raw, &[0; 32]).is_err());
    for changed in [
        serde_json::json!({"schema":"sese-authority-set-v1","authorities":[]}),
        serde_json::json!({"schema":"sese-authority-set-v1","authorities":[config["authorities"][0].clone(),config["authorities"][0].clone()]}),
        serde_json::json!({"schema":"sese-authority-set-v1","authorities":[config["authorities"][0].clone(),{"id":"other","role":"outcome-observer","public_key":key1,"revoked":false}]}),
        serde_json::json!({"schema":"sese-authority-set-v1","authorities":[{"id":"bad","role":"trace-source","public_key":"00".repeat(32),"revoked":false}]}),
        serde_json::json!({"schema":"sese-authority-set-v1","authorities":[{"id":"bad","role":"unknown","public_key":key2,"revoked":false}]}),
        serde_json::json!({"schema":"sese-authority-set-v1","authorities":[{"id":"bad","role":"trace-source","public_key":key2,"revoked":false,"secret":"not allowed"}]}),
    ] {
        let bytes = serde_json::to_vec(&changed).unwrap();
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        assert!(AuthoritySet::from_pinned_json(&bytes, &digest).is_err());
    }
    assert!(AuthoritySet::from_pinned_json(&vec![b' '; 65_537], &[0; 32]).is_err());
}

#[test]
fn attestation_binds_role_authority_and_payload_and_honors_revocation() {
    let signer = SigningKey::from_bytes(&[7; 32]);
    let mut authority = Authority {
        id: "synthetic-source".into(),
        role: AuthorityRole::TraceSource,
        public_key: signer.verifying_key().to_bytes(),
        revoked: false,
    };
    let message = attestation_message(&authority.id, authority.role, b"synthetic payload").unwrap();
    let signature = signer.sign(&message).to_bytes();
    assert!(verify_attestation(
        &authority,
        AuthorityRole::TraceSource,
        b"synthetic payload",
        &signature
    ));
    assert!(!verify_attestation(
        &authority,
        AuthorityRole::OutcomeObserver,
        b"synthetic payload",
        &signature
    ));
    assert!(!verify_attestation(
        &authority,
        AuthorityRole::TraceSource,
        b"changed",
        &signature
    ));
    authority.id = "different-source".into();
    assert!(!verify_attestation(
        &authority,
        AuthorityRole::TraceSource,
        b"synthetic payload",
        &signature
    ));
    authority.id = "synthetic-source".into();
    authority.revoked = true;
    assert!(!verify_attestation(
        &authority,
        AuthorityRole::TraceSource,
        b"synthetic payload",
        &signature
    ));
    assert!(attestation_message("", AuthorityRole::TraceSource, b"x").is_err());
    assert!(attestation_message("source", AuthorityRole::TraceSource, &vec![0; 131_073]).is_err());
}

#[test]
fn typed_synthetic_evidence_binds_review_and_exact_content() {
    let reference = EvidenceReference {
        review_id: 7,
        replay_id: 8,
        candidate_sha256: "ab".repeat(32),
        fixture_sha256: "ef".repeat(32),
        review_ledger_hash: "cd".repeat(32),
    };
    let mut trace = SyntheticTraceEvidence {
        reference: reference.clone(),
        prompt: "synthetic prompt".into(),
        response: "synthetic response".into(),
        source: "fixture-source".into(),
        method: "local-fixture".into(),
        captured_unix_ns: 42,
    };
    let outcome = SyntheticOutcomeEvidence {
        reference,
        outcome: SyntheticOutcome::Validated,
        source: "local-observer".into(),
        method: "synthetic-test".into(),
        evidence_reference: "fixture-result-1".into(),
        observed_unix_ns: 43,
    };
    let signer = SigningKey::from_bytes(&[9; 32]);
    let source = Authority {
        id: "source".into(),
        role: AuthorityRole::TraceSource,
        public_key: signer.verifying_key().to_bytes(),
        revoked: false,
    };
    let payload = trace.canonical_bytes().unwrap();
    let sig = signer
        .sign(&attestation_message(&source.id, source.role, &payload).unwrap())
        .to_bytes();
    assert!(verify_attestation(
        &source,
        AuthorityRole::TraceSource,
        &payload,
        &sig
    ));
    assert!(verify_synthetic_trace(&source, &trace, &sig));
    assert!(!verify_synthetic_outcome(&source, &outcome, &sig));
    trace.reference.review_id += 1;
    assert!(!verify_attestation(
        &source,
        AuthorityRole::TraceSource,
        &trace.canonical_bytes().unwrap(),
        &sig
    ));
    trace.reference.review_id -= 1;
    trace.reference.fixture_sha256 = "12".repeat(32);
    assert!(!verify_synthetic_trace(&source, &trace, &sig));
    trace.reference.fixture_sha256 = "ef".repeat(32);
    trace.reference.review_ledger_hash = "34".repeat(32);
    assert!(!verify_synthetic_trace(&source, &trace, &sig));
    trace.reference.review_ledger_hash = "cd".repeat(32);
    trace.reference.replay_id += 1;
    assert!(!verify_synthetic_trace(&source, &trace, &sig));
    trace.reference.replay_id -= 1;
    trace.prompt.push('!');
    assert!(!verify_attestation(
        &source,
        AuthorityRole::TraceSource,
        &trace.canonical_bytes().unwrap(),
        &sig
    ));
    assert_ne!(outcome.canonical_bytes().unwrap(), payload);
    let mut observer = Authority {
        id: "observer".into(),
        role: AuthorityRole::OutcomeObserver,
        public_key: signer.verifying_key().to_bytes(),
        revoked: false,
    };
    let outcome_bytes = outcome.canonical_bytes().unwrap();
    let outcome_sig = signer
        .sign(&attestation_message(&observer.id, observer.role, &outcome_bytes).unwrap())
        .to_bytes();
    assert!(verify_attestation(
        &observer,
        AuthorityRole::OutcomeObserver,
        &outcome_bytes,
        &outcome_sig
    ));
    assert!(verify_synthetic_outcome(&observer, &outcome, &outcome_sig));
    observer.role = AuthorityRole::TraceSource;
    assert!(!verify_attestation(
        &observer,
        AuthorityRole::TraceSource,
        &outcome_bytes,
        &outcome_sig
    ));
    trace.reference.candidate_sha256 = "wrong".into();
    assert!(trace.canonical_bytes().is_err());
    let mut outcome = outcome;
    outcome.outcome = SyntheticOutcome::Falsified;
    observer.role = AuthorityRole::OutcomeObserver;
    assert!(!verify_attestation(
        &observer,
        AuthorityRole::OutcomeObserver,
        &outcome.canonical_bytes().unwrap(),
        &outcome_sig
    ));
    outcome.observed_unix_ns = 0;
    assert!(outcome.canonical_bytes().is_err());
}
