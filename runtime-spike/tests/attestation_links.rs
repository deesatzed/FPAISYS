use ed25519_dalek::{Signer, SigningKey};
use sese_runtime_spike::attestation::{
    attestation_message, Authority, AuthorityRole, EvidenceReference, SignedClaim,
    SyntheticOutcome, SyntheticOutcomeEvidence, SyntheticPreferenceEvidence,
    SyntheticTraceEvidence,
};
use sese_runtime_spike::attestation::{AuthoritySet, NamedSignedClaim};
use sese_runtime_spike::storage::{Storage, SyntheticTraceInput};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn signed_claims_must_match_checkpointed_synthetic_records() {
    let path = std::env::temp_dir().join(format!(
        "sese-signed-links-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut demo = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([env!("CARGO_BIN_EXE_event-worker"), path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    demo.stdin.take().unwrap().write_all(b"reject\n").unwrap();
    assert!(demo.wait().unwrap().success());
    let mut db = Storage::open(&path).unwrap();
    let input: SyntheticTraceInput = serde_json::from_str(r#"{"schema":"sese-synthetic-training-trace-v1","actor":"operator","source":"synthetic source","prompt":"prompt","response":"response","preference":{"preferred":"preferred","rejected":"response","source":"synthetic preference"}}"#).unwrap();
    let trace_id = db.record_synthetic_event_trace(1, &input).unwrap();
    let old_checkpoint = db.ledger_checkpoint().unwrap();
    db.resolve_event_review(1, "validated", "operator", "synthetic result")
        .unwrap();
    let checkpoint = db.ledger_checkpoint().unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (candidate,replay,fixture,review_hash,review_time,trace_time,outcome_time):(String,i64,String,String,String,String,String)=conn.query_row("SELECT r.candidate_id,r.replay_id,p.pack_digest,e.hash,r.created_unix_ns,t.created_unix_ns,o.created_unix_ns FROM event_reviews r JOIN event_replay_runs p ON p.id=r.replay_id JOIN ledger_events e ON e.kind='event_review' AND e.reference=r.id JOIN event_training_traces t ON t.review_id=r.id JOIN event_review_outcomes o ON o.review_id=r.id WHERE r.id=1", [], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).unwrap();
    let reference = EvidenceReference {
        review_id: 1,
        replay_id: replay,
        candidate_sha256: candidate,
        fixture_sha256: fixture,
        review_ledger_hash: review_hash,
    };
    let mut trace = SyntheticTraceEvidence {
        reference: reference.clone(),
        prompt: "prompt".into(),
        response: "response".into(),
        source: "synthetic source".into(),
        method: "test fixture".into(),
        captured_unix_ns: trace_time.parse().unwrap(),
    };
    let mut outcome = SyntheticOutcomeEvidence {
        reference,
        outcome: SyntheticOutcome::Validated,
        source: "synthetic result".into(),
        method: "test observation".into(),
        evidence_reference: "synthetic case".into(),
        observed_unix_ns: outcome_time.parse().unwrap(),
    };
    let sign_trace = SigningKey::from_bytes(&[11; 32]);
    let sign_outcome = SigningKey::from_bytes(&[12; 32]);
    let sign_preference = SigningKey::from_bytes(&[13; 32]);
    let source = Authority {
        id: "trace-source".into(),
        role: AuthorityRole::TraceSource,
        public_key: sign_trace.verifying_key().to_bytes(),
        revoked: false,
    };
    let observer = Authority {
        id: "observer".into(),
        role: AuthorityRole::OutcomeObserver,
        public_key: sign_outcome.verifying_key().to_bytes(),
        revoked: false,
    };
    let sign_t = |t: &SyntheticTraceEvidence| {
        sign_trace
            .sign(
                &attestation_message(&source.id, source.role, &t.canonical_bytes().unwrap())
                    .unwrap(),
            )
            .to_bytes()
    };
    let sign_o = |o: &SyntheticOutcomeEvidence| {
        sign_outcome
            .sign(
                &attestation_message(&observer.id, observer.role, &o.canonical_bytes().unwrap())
                    .unwrap(),
            )
            .to_bytes()
    };
    let check = |db: &mut Storage,
                 t: &SyntheticTraceEvidence,
                 o: &SyntheticOutcomeEvidence,
                 checkpoint: &(i64, String)| {
        let ts = sign_t(t);
        let os = sign_o(o);
        db.check_synthetic_attestation_links(
            trace_id,
            SignedClaim {
                evidence: t,
                authority: &source,
                signature: &ts,
            },
            SignedClaim {
                evidence: o,
                authority: &observer,
                signature: &os,
            },
            (checkpoint.0, &checkpoint.1),
        )
    };
    drop(db);
    conn.execute_batch("DROP TABLE event_preference_receipts; DROP TABLE event_attestation_receipts; PRAGMA user_version=35;")
        .unwrap();
    let mut db = Storage::open(&path).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 37);
    let result = check(&mut db, &trace, &outcome, &checkpoint).unwrap();
    assert_eq!(result["signature_and_record_links_match"], true);
    assert_eq!(result["training_ready"], false);
    let trace_hash: String = conn
        .query_row(
            "SELECT hash FROM ledger_events WHERE kind='event_training_trace' AND reference=?1",
            [trace_id],
            |r| r.get(0),
        )
        .unwrap();
    let reviewer = Authority {
        id: "reviewer".into(),
        role: AuthorityRole::PreferenceReviewer,
        public_key: sign_preference.verifying_key().to_bytes(),
        revoked: false,
    };
    let mut preference = SyntheticPreferenceEvidence {
        reference: trace.reference.clone(),
        trace_id,
        trace_ledger_hash: trace_hash,
        prompt: "prompt".into(),
        preferred: "preferred".into(),
        rejected: "response".into(),
        source: "synthetic preference".into(),
        comparability_basis: "same synthetic prompt".into(),
        reviewed_unix_ns: trace_time.parse().unwrap(),
    };
    let sign_p = |p: &SyntheticPreferenceEvidence| {
        sign_preference
            .sign(
                &attestation_message(&reviewer.id, reviewer.role, &p.canonical_bytes().unwrap())
                    .unwrap(),
            )
            .to_bytes()
    };
    let check_p = |db: &mut Storage, p: &SyntheticPreferenceEvidence| {
        let signature = sign_p(p);
        db.check_synthetic_preference_link(
            SignedClaim {
                evidence: p,
                authority: &reviewer,
                signature: &signature,
            },
            (checkpoint.0, &checkpoint.1),
        )
    };
    let pref_result = check_p(&mut db, &preference).unwrap();
    assert_eq!(pref_result["signature_and_record_links_match"], true);
    assert_eq!(pref_result["comparability_verified"], false);
    assert_eq!(pref_result["training_ready"], false);
    preference.preferred = "other".into();
    assert!(check_p(&mut db, &preference).is_err());
    preference.preferred = "preferred".into();
    preference.trace_ledger_hash = "0".repeat(64);
    assert!(check_p(&mut db, &preference).is_err());
    preference.trace_ledger_hash = conn
        .query_row(
            "SELECT hash FROM ledger_events WHERE kind='event_training_trace' AND reference=?1",
            [trace_id],
            |r| r.get(0),
        )
        .unwrap();
    preference.reviewed_unix_ns = trace_time.parse::<u64>().unwrap() - 1;
    assert!(check_p(&mut db, &preference).is_err());
    preference.reviewed_unix_ns = trace_time.parse().unwrap();
    let bad_preference_sig = [0u8; 64];
    assert!(db
        .check_synthetic_preference_link(
            SignedClaim {
                evidence: &preference,
                authority: &reviewer,
                signature: &bad_preference_sig
            },
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    let hex = |key: &[u8; 32]| key.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let config=serde_json::to_vec(&serde_json::json!({"schema":"sese-authority-set-v1","authorities":[
        {"id":"trace-source","role":"trace-source","public_key":hex(&source.public_key),"revoked":false},
        {"id":"observer","role":"outcome-observer","public_key":hex(&observer.public_key),"revoked":false},
        {"id":"reviewer","role":"preference-reviewer","public_key":hex(&reviewer.public_key),"revoked":false}
    ]})).unwrap();
    let pin: [u8; 32] = Sha256::digest(&config).into();
    let set = AuthoritySet::from_pinned_json(&config, &pin).unwrap();
    let ts = sign_t(&trace);
    let os = sign_o(&outcome);
    let using_set = db
        .check_synthetic_attestation_links_in_set(
            trace_id,
            &set,
            NamedSignedClaim {
                evidence: &trace,
                authority_id: "trace-source",
                signature: &ts,
            },
            NamedSignedClaim {
                evidence: &outcome,
                authority_id: "observer",
                signature: &os,
            },
            (checkpoint.0, &checkpoint.1),
        )
        .unwrap();
    assert_eq!(using_set["authority_config_digest_matched"], true);
    assert_eq!(using_set["authority_config_verified"], false);
    assert_eq!(using_set["training_ready"], false);
    drop(db);
    conn.execute_batch("DROP TABLE event_preference_receipts; PRAGMA user_version=36;")
        .unwrap();
    let verifier_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/verify_ledger.py");
    let anchor_seq = checkpoint.0.to_string();
    let legacy_check = Command::new("python3")
        .args([
            verifier_path.to_str().unwrap(),
            path.to_str().unwrap(),
            "--seq",
            &anchor_seq,
            "--hash",
            &checkpoint.1,
            "--verify-event-attestations",
        ])
        .output()
        .unwrap();
    assert!(
        legacy_check.status.success(),
        "{}",
        String::from_utf8_lossy(&legacy_check.stderr)
    );
    let legacy_check: serde_json::Value = serde_json::from_slice(&legacy_check.stdout).unwrap();
    assert!(legacy_check["event_preference_receipts_checked"].is_null());
    let mut db = Storage::open(&path).unwrap();
    let upgraded: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(upgraded, 37);
    assert!(check(&mut db, &trace, &outcome, &checkpoint).is_ok());
    let pref_signature = sign_p(&preference);
    let record_preference = |db: &mut Storage| {
        db.record_synthetic_preference_receipt(
            &set,
            NamedSignedClaim {
                evidence: &preference,
                authority_id: "reviewer",
                signature: &pref_signature,
            },
            (checkpoint.0, &checkpoint.1),
        )
    };
    conn.execute_batch("CREATE TRIGGER reject_preference_event BEFORE INSERT ON ledger_events WHEN NEW.kind='event_preference_receipt' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(record_preference(&mut db).is_err());
    let count: i64 = conn
        .query_row("SELECT count(*) FROM event_preference_receipts", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    conn.execute_batch("DROP TRIGGER reject_preference_event")
        .unwrap();
    let preference_receipt = record_preference(&mut db).unwrap();
    let inspected_preference = db
        .inspect_synthetic_preference_receipt(preference_receipt)
        .unwrap();
    assert_eq!(inspected_preference["signature_rechecked"], false);
    assert_eq!(inspected_preference["training_ready"], false);
    assert_eq!(
        db.recheck_synthetic_preference_receipt(preference_receipt, &set)
            .unwrap()["signature_rechecked"],
        true
    );
    assert!(record_preference(&mut db).is_err());
    conn.execute(
        "UPDATE event_preference_receipts SET payload=x'00' WHERE id=?1",
        [preference_receipt],
    )
    .unwrap();
    assert!(db
        .inspect_synthetic_preference_receipt(preference_receipt)
        .is_err());
    conn.execute(
        "UPDATE event_preference_receipts SET payload=?1 WHERE id=?2",
        rusqlite::params![preference.canonical_bytes().unwrap(), preference_receipt],
    )
    .unwrap();
    assert!(db
        .inspect_synthetic_preference_receipt(preference_receipt)
        .is_ok());
    let record = |db: &mut Storage| {
        db.record_synthetic_attestation_receipt(
            trace_id,
            &set,
            NamedSignedClaim {
                evidence: &trace,
                authority_id: "trace-source",
                signature: &ts,
            },
            NamedSignedClaim {
                evidence: &outcome,
                authority_id: "observer",
                signature: &os,
            },
            (checkpoint.0, &checkpoint.1),
        )
    };
    conn.execute_batch("CREATE TRIGGER reject_attestation_event BEFORE INSERT ON ledger_events WHEN NEW.kind='event_attestation_receipt' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(record(&mut db).is_err());
    let count: i64 = conn
        .query_row("SELECT count(*) FROM event_attestation_receipts", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    conn.execute_batch("DROP TRIGGER reject_attestation_event")
        .unwrap();
    let receipt = record(&mut db).unwrap();
    drop(db);
    let mut db = Storage::open(&path).unwrap();
    let receipt_checkpoint = db.ledger_checkpoint().unwrap();
    let combined = db
        .check_synthetic_evidence_package(
            receipt,
            preference_receipt,
            &set,
            (receipt_checkpoint.0, &receipt_checkpoint.1),
        )
        .unwrap();
    let policy_bytes = serde_json::to_vec(&serde_json::json!({"schema":"sese-synthetic-trust-policy-v1",
        "authority_config_sha256": pin.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        "declarations":[{"trace_source":"trace-source","outcome_observer":"observer","preference_reviewer":"reviewer",
        "operator":"synthetic operator","observer_relationship_basis":"synthetic observer declaration",
        "reviewer_relationship_basis":"synthetic reviewer declaration"}]})).unwrap();
    let policy_pin: [u8; 32] = Sha256::digest(&policy_bytes).into();
    let policy = sese_runtime_spike::attestation::SyntheticTrustPolicy::from_pinned_json(
        &policy_bytes,
        &policy_pin,
        &set,
    )
    .unwrap();
    let policy_check = db
        .check_synthetic_evidence_policy(
            receipt,
            preference_receipt,
            &set,
            &policy,
            (receipt_checkpoint.0, &receipt_checkpoint.1),
        )
        .unwrap();
    assert_eq!(policy_check["operator_declaration_matches"], true);
    assert_eq!(policy_check["observer_independence_verified"], false);
    assert_eq!(policy_check["training_ready"], false);
    assert!(db
        .check_synthetic_evidence_policy(
            receipt,
            preference_receipt,
            &set,
            &policy,
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    let mut undeclared: serde_json::Value = serde_json::from_slice(&policy_bytes).unwrap();
    undeclared["declarations"][0]["preference_reviewer"] = serde_json::Value::Null;
    undeclared["declarations"][0]["reviewer_relationship_basis"] = serde_json::Value::Null;
    let undeclared = serde_json::to_vec(&undeclared).unwrap();
    let undeclared = sese_runtime_spike::attestation::SyntheticTrustPolicy::from_pinned_json(
        &undeclared,
        &Sha256::digest(&undeclared).into(),
        &set,
    )
    .unwrap();
    let common_check = db
        .check_synthetic_common_evidence_policy(
            receipt,
            &set,
            &undeclared,
            (receipt_checkpoint.0, &receipt_checkpoint.1),
        )
        .unwrap();
    assert_eq!(common_check["operator_declaration_matches"], true);
    assert_eq!(common_check["signatures_rechecked"], true);
    assert_eq!(common_check["outcome"], "validated");
    assert_eq!(common_check["training_ready"], false);
    let rule_bytes = serde_json::to_vec(&serde_json::json!({"schema":"sese-synthetic-reward-rule-v1", "version":"outcome-sign-v1", "validated":1,"falsified":-1,"actor":"synthetic operator","source":"local experiment"})).unwrap();
    let rule_pin: [u8; 32] = Sha256::digest(&rule_bytes).into();
    let rule = sese_runtime_spike::reward_rule::SyntheticRewardRule::from_pinned_json(
        &rule_bytes,
        &rule_pin,
    )
    .unwrap();
    let reward = db
        .preview_synthetic_reward(
            receipt,
            &set,
            &undeclared,
            &rule,
            (receipt_checkpoint.0, &receipt_checkpoint.1),
        )
        .unwrap();
    assert_eq!(reward["reward"], 1);
    assert_eq!(reward["reward_rule"]["version"], "outcome-sign-v1");
    assert_eq!(reward["training_ready"], false);
    assert!(db
        .preview_synthetic_reward(
            receipt,
            &set,
            &undeclared,
            &rule,
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    assert!(db
        .check_synthetic_common_evidence_policy(
            receipt,
            &set,
            &undeclared,
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    assert!(db
        .check_synthetic_evidence_policy(
            receipt,
            preference_receipt,
            &set,
            &undeclared,
            (receipt_checkpoint.0, &receipt_checkpoint.1)
        )
        .is_err());
    assert_eq!(combined["all_signatures_rechecked"], true);
    assert_eq!(combined["outcome"], "validated");
    assert_eq!(combined["comparability_verified"], false);
    assert_eq!(combined["authority_config_verified"], false);
    assert_eq!(combined["training_ready"], false);
    let exported = db
        .export_synthetic_evidence_package(
            receipt,
            preference_receipt,
            &set,
            (receipt_checkpoint.0, &receipt_checkpoint.1),
        )
        .unwrap();
    let exported: serde_json::Value = serde_json::from_str(exported.trim()).unwrap();
    assert_eq!(exported["schema"], "sese-synthetic-signed-evidence-v1");
    assert_eq!(exported["trace"]["prompt"], "prompt");
    assert_eq!(exported["trace"]["response"], "response");
    assert_eq!(exported["preference"]["preferred"], "preferred");
    assert_eq!(exported["preference"]["rejected"], "response");
    assert_eq!(exported["outcome"]["evidence"], "caller_asserted");
    assert_eq!(exported["training_ready"], false);
    assert_eq!(
        exported["signatures"]["trace"]["signature_hex"]
            .as_str()
            .unwrap()
            .len(),
        128
    );
    let expected_trace_payload = trace
        .canonical_bytes()
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let expected_trace_signature = ts.iter().map(|b| format!("{b:02x}")).collect::<String>();
    assert_eq!(
        exported["signatures"]["trace"]["payload_hex"],
        expected_trace_payload
    );
    assert_eq!(
        exported["signatures"]["trace"]["signature_hex"],
        expected_trace_signature
    );
    assert!(db
        .export_synthetic_evidence_package(
            receipt,
            preference_receipt,
            &set,
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    assert!(db
        .check_synthetic_evidence_package(
            receipt,
            preference_receipt,
            &set,
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    assert!(db
        .check_synthetic_evidence_package(
            receipt,
            999,
            &set,
            (receipt_checkpoint.0, &receipt_checkpoint.1)
        )
        .is_err());
    assert_eq!(
        db.recheck_synthetic_preference_receipt(preference_receipt, &set)
            .unwrap()["signature_rechecked"],
        true
    );
    let receipt_seq = receipt_checkpoint.0.to_string();
    let verifier =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/verify_ledger.py");
    let independent = || {
        Command::new("python3")
            .args([
                verifier.to_str().unwrap(),
                path.to_str().unwrap(),
                "--seq",
                &receipt_seq,
                "--hash",
                &receipt_checkpoint.1,
                "--verify-event-attestations",
            ])
            .output()
            .unwrap()
    };
    let checked = independent();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let checked: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(checked["event_attestation_receipts_checked"], 1);
    assert_eq!(checked["event_preference_receipts_checked"], 1);
    assert_eq!(checked["attestation_signatures_checked"], false);
    let second_trace = db.record_synthetic_event_trace(1, &input).unwrap();
    let (second_hash,second_time):(String,String)=conn.query_row("SELECT e.hash,t.created_unix_ns FROM event_training_traces t JOIN ledger_events e ON e.kind='event_training_trace' AND e.reference=t.id WHERE t.id=?1",[second_trace],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    let second_pref = SyntheticPreferenceEvidence {
        reference: trace.reference.clone(),
        trace_id: second_trace,
        trace_ledger_hash: second_hash,
        prompt: "prompt".into(),
        preferred: "preferred".into(),
        rejected: "response".into(),
        source: "synthetic preference".into(),
        comparability_basis: "same synthetic prompt".into(),
        reviewed_unix_ns: second_time.parse().unwrap(),
    };
    let second_sig = sign_p(&second_pref);
    let second_anchor = db.ledger_checkpoint().unwrap();
    let second_receipt = db
        .record_synthetic_preference_receipt(
            &set,
            NamedSignedClaim {
                evidence: &second_pref,
                authority_id: "reviewer",
                signature: &second_sig,
            },
            (second_anchor.0, &second_anchor.1),
        )
        .unwrap();
    let later_anchor = db.ledger_checkpoint().unwrap();
    assert!(db
        .check_synthetic_evidence_package(
            receipt,
            second_receipt,
            &set,
            (later_anchor.0, &later_anchor.1)
        )
        .is_err());
    assert!(db
        .export_synthetic_evidence_package(
            receipt,
            second_receipt,
            &set,
            (later_anchor.0, &later_anchor.1)
        )
        .is_err());
    let original_pref_signature: Vec<u8> = conn
        .query_row(
            "SELECT signature FROM event_preference_receipts WHERE id=?1",
            [preference_receipt],
            |r| r.get(0),
        )
        .unwrap();
    let mut changed_pref_signature = original_pref_signature.clone();
    changed_pref_signature[0] ^= 1;
    conn.execute(
        "UPDATE event_preference_receipts SET signature=?1 WHERE id=?2",
        rusqlite::params![changed_pref_signature, preference_receipt],
    )
    .unwrap();
    assert!(!independent().status.success());
    assert!(db
        .check_synthetic_evidence_package(
            receipt,
            preference_receipt,
            &set,
            (receipt_checkpoint.0, &receipt_checkpoint.1)
        )
        .is_err());
    conn.execute(
        "UPDATE event_preference_receipts SET signature=?1 WHERE id=?2",
        rusqlite::params![original_pref_signature, preference_receipt],
    )
    .unwrap();
    assert!(independent().status.success());
    let inspected = db.inspect_synthetic_attestation_receipt(receipt).unwrap();
    assert_eq!(inspected["training_ready"], false);
    assert_eq!(inspected["signature_rechecked"], false);
    let rechecked = db
        .recheck_synthetic_attestation_receipt(receipt, &set)
        .unwrap();
    assert_eq!(rechecked["signature_rechecked"], true);
    assert_eq!(rechecked["training_ready"], false);
    assert!(record(&mut db).is_err());
    let original_signature: Vec<u8> = conn
        .query_row(
            "SELECT trace_signature FROM event_attestation_receipts WHERE id=?1",
            [receipt],
            |r| r.get(0),
        )
        .unwrap();
    let mut modified_signature = original_signature.clone();
    modified_signature[0] ^= 1;
    conn.execute(
        "UPDATE event_attestation_receipts SET trace_signature=?1 WHERE id=?2",
        rusqlite::params![modified_signature, receipt],
    )
    .unwrap();
    assert!(!independent().status.success());
    assert!(db
        .check_synthetic_common_evidence_policy(
            receipt,
            &set,
            &undeclared,
            (receipt_checkpoint.0, &receipt_checkpoint.1)
        )
        .is_err());
    conn.execute(
        "UPDATE event_attestation_receipts SET trace_signature=?1 WHERE id=?2",
        rusqlite::params![original_signature, receipt],
    )
    .unwrap();
    let original_anchor: i64 = conn
        .query_row(
            "SELECT checkpoint_seq FROM event_attestation_receipts WHERE id=?1",
            [receipt],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute(
        "UPDATE event_attestation_receipts SET checkpoint_seq=1 WHERE id=?1",
        [receipt],
    )
    .unwrap();
    assert!(!independent().status.success());
    conn.execute(
        "UPDATE event_attestation_receipts SET checkpoint_seq=?1 WHERE id=?2",
        rusqlite::params![original_anchor, receipt],
    )
    .unwrap();
    assert!(independent().status.success());
    conn.execute(
        "UPDATE event_attestation_receipts SET trace_payload=x'00' WHERE id=?1",
        [receipt],
    )
    .unwrap();
    assert!(db.inspect_synthetic_attestation_receipt(receipt).is_err());
    assert!(!independent().status.success());
    conn.execute(
        "UPDATE event_attestation_receipts SET trace_payload=?1 WHERE id=?2",
        rusqlite::params![trace.canonical_bytes().unwrap(), receipt],
    )
    .unwrap();
    assert!(db.inspect_synthetic_attestation_receipt(receipt).is_ok());
    assert!(independent().status.success());
    let bundle_path = path.with_extension("bundle.json");
    let authority_path = path.with_extension("authorities.json");
    let hex_bytes = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let reference = serde_json::json!({"review_id":trace.reference.review_id,"replay_id":trace.reference.replay_id,"candidate_sha256":trace.reference.candidate_sha256,"fixture_sha256":trace.reference.fixture_sha256,"review_ledger_hash":trace.reference.review_ledger_hash});
    let bundle = serde_json::json!({"schema":"sese-synthetic-attestation-bundle-v1","trace_id":trace_id,
        "trace":{"authority_id":"trace-source","signature":hex_bytes(&ts),"evidence":{"reference":reference,"prompt":trace.prompt,"response":trace.response,"source":trace.source,"method":trace.method,"captured_unix_ns":trace.captured_unix_ns}},
        "outcome":{"authority_id":"observer","signature":hex_bytes(&os),"evidence":{"reference":reference,"outcome":"validated","source":outcome.source,"method":outcome.method,"evidence_reference":outcome.evidence_reference,"observed_unix_ns":outcome.observed_unix_ns}}});
    let raw_bundle = serde_json::to_vec(&bundle).unwrap();
    std::fs::write(&bundle_path, &raw_bundle).unwrap();
    std::fs::write(&authority_path, &config).unwrap();
    let pin_hex = hex_bytes(&pin);
    let seq = checkpoint.0.to_string();
    let receipt_id_text = receipt.to_string();
    let preference_receipt_text = preference_receipt.to_string();
    let policy_path = path.with_extension("policy.json");
    std::fs::write(&policy_path, &policy_bytes).unwrap();
    let policy_pin_hex = hex_bytes(&policy_pin);
    let policy_cli = |pin: &str| {
        Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args([
                "event-evidence-policy-check",
                path.to_str().unwrap(),
                &receipt_id_text,
                &preference_receipt_text,
                authority_path.to_str().unwrap(),
                &pin_hex,
                policy_path.to_str().unwrap(),
                pin,
                &receipt_checkpoint.0.to_string(),
                &receipt_checkpoint.1,
            ])
            .output()
            .unwrap()
    };
    let policy_result = policy_cli(&policy_pin_hex);
    assert!(
        policy_result.status.success(),
        "{}",
        String::from_utf8_lossy(&policy_result.stderr)
    );
    let policy_result: serde_json::Value = serde_json::from_slice(&policy_result.stdout).unwrap();
    assert_eq!(policy_result["operator_declaration_matches"], true);
    assert_eq!(policy_result["training_ready"], false);
    let rejected_policy = policy_cli(&"0".repeat(64));
    assert!(!rejected_policy.status.success());
    assert!(rejected_policy.stdout.is_empty());
    let common_cli = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-common-policy-check",
            path.to_str().unwrap(),
            &receipt_id_text,
            authority_path.to_str().unwrap(),
            &pin_hex,
            policy_path.to_str().unwrap(),
            &policy_pin_hex,
            &receipt_seq,
            &receipt_checkpoint.1,
        ])
        .output()
        .unwrap();
    assert!(
        common_cli.status.success(),
        "{}",
        String::from_utf8_lossy(&common_cli.stderr)
    );
    let common_cli: serde_json::Value = serde_json::from_slice(&common_cli.stdout).unwrap();
    assert_eq!(common_cli["operator_declaration_matches"], true);
    assert_eq!(common_cli["training_ready"], false);
    let rule_path = path.with_extension("reward-rule.json");
    std::fs::write(&rule_path, &rule_bytes).unwrap();
    let rule_pin_hex = hex_bytes(&rule_pin);
    let reward_cli = |rule_hash: &str| {
        Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args([
                "event-reward-preview",
                path.to_str().unwrap(),
                &receipt_id_text,
                authority_path.to_str().unwrap(),
                &pin_hex,
                policy_path.to_str().unwrap(),
                &policy_pin_hex,
                rule_path.to_str().unwrap(),
                rule_hash,
                &receipt_seq,
                &receipt_checkpoint.1,
            ])
            .output()
            .unwrap()
    };
    let reward_cli_ok = reward_cli(&rule_pin_hex);
    assert!(
        reward_cli_ok.status.success(),
        "{}",
        String::from_utf8_lossy(&reward_cli_ok.stderr)
    );
    let reward_cli_ok: serde_json::Value = serde_json::from_slice(&reward_cli_ok.stdout).unwrap();
    assert_eq!(reward_cli_ok["reward"], 1);
    assert_eq!(reward_cli_ok["training_ready"], false);
    let reward_cli_bad = reward_cli(&"0".repeat(64));
    assert!(!reward_cli_bad.status.success());
    assert!(reward_cli_bad.stdout.is_empty());
    std::fs::remove_file(rule_path).unwrap();
    std::fs::remove_file(policy_path).unwrap();
    let package_cli = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-evidence-check",
            path.to_str().unwrap(),
            &receipt_id_text,
            &preference_receipt_text,
            authority_path.to_str().unwrap(),
            &pin_hex,
            &receipt_seq,
            &receipt_checkpoint.1,
        ])
        .output()
        .unwrap();
    assert!(
        package_cli.status.success(),
        "{}",
        String::from_utf8_lossy(&package_cli.stderr)
    );
    let package_cli: serde_json::Value = serde_json::from_slice(&package_cli.stdout).unwrap();
    assert_eq!(package_cli["all_signatures_rechecked"], true);
    assert_eq!(package_cli["training_ready"], false);
    let exported_cli = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-evidence-export",
            path.to_str().unwrap(),
            &receipt_id_text,
            &preference_receipt_text,
            authority_path.to_str().unwrap(),
            &pin_hex,
            &receipt_seq,
            &receipt_checkpoint.1,
        ])
        .output()
        .unwrap();
    assert!(
        exported_cli.status.success(),
        "{}",
        String::from_utf8_lossy(&exported_cli.stderr)
    );
    let exported_bytes = exported_cli.stdout.clone();
    let exported_cli: serde_json::Value = serde_json::from_slice(&exported_bytes).unwrap();
    assert_eq!(exported_cli["training_ready"], false);
    let export_path = path.with_extension("export.jsonl");
    std::fs::write(&export_path, &exported_bytes).unwrap();
    let independent_export = || {
        Command::new("python3")
            .args([
                concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/verify_ledger.py"),
                path.to_str().unwrap(),
                "--seq",
                &receipt_seq,
                "--hash",
                &receipt_checkpoint.1,
                "--verify-event-export",
                export_path.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    };
    let independent_result = independent_export();
    assert!(
        independent_result.status.success(),
        "{}",
        String::from_utf8_lossy(&independent_result.stderr)
    );
    let independent_result: serde_json::Value =
        serde_json::from_slice(&independent_result.stdout).unwrap();
    assert_eq!(independent_result["event_export_matches_database"], true);
    assert_eq!(independent_result["attestation_signatures_checked"], false);
    assert_eq!(
        independent_result["attestation_payload_record_links_checked"],
        true
    );
    let duplicate = String::from_utf8(exported_bytes.clone()).unwrap().replace(
        "\"training_ready\":false",
        "\"training_ready\":false,\"training_ready\":false",
    );
    std::fs::write(&export_path, duplicate).unwrap();
    assert!(!independent_export().status.success());
    let mut wrong_type = exported_cli.clone();
    wrong_type["training_ready"] = 0.into();
    std::fs::write(&export_path, format!("{}\n", wrong_type)).unwrap();
    assert!(!independent_export().status.success());
    let mut wrong_checkpoint = exported_cli.clone();
    wrong_checkpoint["checkpoint"]["seq"] = 1.into();
    std::fs::write(&export_path, format!("{}\n", wrong_checkpoint)).unwrap();
    assert!(!independent_export().status.success());
    std::fs::write(&export_path, &exported_bytes).unwrap();
    let verify_export = |pin: &str, seq: &str, hash: &str| {
        Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args([
                "event-evidence-verify",
                path.to_str().unwrap(),
                export_path.to_str().unwrap(),
                authority_path.to_str().unwrap(),
                pin,
                seq,
                hash,
            ])
            .output()
            .unwrap()
    };
    let verified = verify_export(&pin_hex, &receipt_seq, &receipt_checkpoint.1);
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    let verified: serde_json::Value = serde_json::from_slice(&verified.stdout).unwrap();
    assert_eq!(verified["matches_verified_database"], true);
    assert_eq!(verified["training_ready"], false);
    assert!(
        !verify_export(&"0".repeat(64), &receipt_seq, &receipt_checkpoint.1)
            .status
            .success()
    );
    assert!(!verify_export(&pin_hex, &seq, &checkpoint.1)
        .status
        .success());
    let mut altered_export = exported_cli.clone();
    altered_export["trace"]["prompt"] = "tampered".into();
    std::fs::write(&export_path, format!("{}\n", altered_export)).unwrap();
    assert!(!independent_export().status.success());
    let rejected_export = verify_export(&pin_hex, &receipt_seq, &receipt_checkpoint.1);
    assert!(!rejected_export.status.success());
    assert!(rejected_export.stdout.is_empty());
    std::fs::write(
        &export_path,
        [exported_bytes.as_slice(), exported_bytes.as_slice()].concat(),
    )
    .unwrap();
    assert!(
        !verify_export(&pin_hex, &receipt_seq, &receipt_checkpoint.1)
            .status
            .success()
    );
    std::fs::write(&export_path, &exported_bytes[..exported_bytes.len() - 1]).unwrap();
    assert!(
        !verify_export(&pin_hex, &receipt_seq, &receipt_checkpoint.1)
            .status
            .success()
    );
    std::fs::write(&export_path, vec![b' '; 1_048_577]).unwrap();
    assert!(
        !verify_export(&pin_hex, &receipt_seq, &receipt_checkpoint.1)
            .status
            .success()
    );
    std::fs::remove_file(export_path).unwrap();
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let fifo_path = path.with_extension("input.fifo");
        let fifo = CString::new(fifo_path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let mut child = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args([
                "event-evidence-verify",
                path.to_str().unwrap(),
                fifo_path.to_str().unwrap(),
                authority_path.to_str().unwrap(),
                &pin_hex,
                &receipt_seq,
                &receipt_checkpoint.1,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut completed = None;
        for _ in 0..100 {
            if let Some(status) = child.try_wait().unwrap() {
                completed = Some(status);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if completed.is_none() {
            child.kill().unwrap();
            child.wait().unwrap();
        }
        std::fs::remove_file(fifo_path).unwrap();
        assert!(
            completed.is_some(),
            "FIFO input blocked before file validation"
        );
        assert!(!completed.unwrap().success());
    }
    let stale_export = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-evidence-export",
            path.to_str().unwrap(),
            &receipt_id_text,
            &preference_receipt_text,
            authority_path.to_str().unwrap(),
            &pin_hex,
            &seq,
            &checkpoint.1,
        ])
        .output()
        .unwrap();
    assert!(!stale_export.status.success());
    assert!(stale_export.stdout.is_empty());
    let run_cli = |pin: &str| {
        Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args([
                "event-attestation-check",
                path.to_str().unwrap(),
                bundle_path.to_str().unwrap(),
                authority_path.to_str().unwrap(),
                pin,
                &seq,
                &checkpoint.1,
            ])
            .output()
            .unwrap()
    };
    let output = run_cli(&pin_hex);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let checked: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(checked["signature_and_record_links_match"], true);
    assert_eq!(checked["training_ready"], false);
    let new_checkpoint = db.ledger_checkpoint().unwrap();
    let new_seq = new_checkpoint.0.to_string();
    let record_cli = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-attestation-record",
            path.to_str().unwrap(),
            bundle_path.to_str().unwrap(),
            authority_path.to_str().unwrap(),
            &pin_hex,
            &new_seq,
            &new_checkpoint.1,
        ])
        .output()
        .unwrap();
    assert!(
        record_cli.status.success(),
        "{}",
        String::from_utf8_lossy(&record_cli.stderr)
    );
    let recorded: serde_json::Value = serde_json::from_slice(&record_cli.stdout).unwrap();
    let receipt_id = recorded["receipt_id"].as_i64().unwrap().to_string();
    let inspect_cli = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-attestation-inspect",
            path.to_str().unwrap(),
            &receipt_id,
        ])
        .output()
        .unwrap();
    assert!(inspect_cli.status.success());
    let inspected: serde_json::Value = serde_json::from_slice(&inspect_cli.stdout).unwrap();
    assert_eq!(inspected["signature_rechecked"], false);
    assert_eq!(inspected["training_ready"], false);
    let recheck_cli = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-attestation-recheck",
            path.to_str().unwrap(),
            &receipt_id,
            authority_path.to_str().unwrap(),
            &pin_hex,
        ])
        .output()
        .unwrap();
    assert!(recheck_cli.status.success());
    let rechecked: serde_json::Value = serde_json::from_slice(&recheck_cli.stdout).unwrap();
    assert_eq!(rechecked["signature_rechecked"], true);
    assert_eq!(rechecked["training_ready"], false);
    let preference_path = path.with_extension("preference.json");
    let preference_signature = sign_p(&preference);
    let preference_json = serde_json::json!({"schema":"sese-synthetic-preference-bundle-v1","claim":{"authority_id":"reviewer","signature":hex_bytes(&preference_signature),"evidence":{"reference":reference,"trace_id":trace_id,"trace_ledger_hash":preference.trace_ledger_hash,"prompt":preference.prompt,"preferred":preference.preferred,"rejected":preference.rejected,"source":preference.source,"comparability_basis":preference.comparability_basis,"reviewed_unix_ns":preference.reviewed_unix_ns}}});
    std::fs::write(
        &preference_path,
        serde_json::to_vec(&preference_json).unwrap(),
    )
    .unwrap();
    let run_preference = || {
        Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args([
                "event-preference-check",
                path.to_str().unwrap(),
                preference_path.to_str().unwrap(),
                authority_path.to_str().unwrap(),
                &pin_hex,
                &seq,
                &checkpoint.1,
            ])
            .output()
            .unwrap()
    };
    let pref_cli = run_preference();
    assert!(
        pref_cli.status.success(),
        "{}",
        String::from_utf8_lossy(&pref_cli.stderr)
    );
    let pref_cli: serde_json::Value = serde_json::from_slice(&pref_cli.stdout).unwrap();
    assert_eq!(pref_cli["training_ready"], false);
    assert_eq!(pref_cli["comparability_verified"], false);
    let pref_checkpoint = db.ledger_checkpoint().unwrap();
    let pref_seq = pref_checkpoint.0.to_string();
    let recorded_pref = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-preference-record",
            path.to_str().unwrap(),
            preference_path.to_str().unwrap(),
            authority_path.to_str().unwrap(),
            &pin_hex,
            &pref_seq,
            &pref_checkpoint.1,
        ])
        .output()
        .unwrap();
    assert!(
        recorded_pref.status.success(),
        "{}",
        String::from_utf8_lossy(&recorded_pref.stderr)
    );
    let recorded_pref: serde_json::Value = serde_json::from_slice(&recorded_pref.stdout).unwrap();
    let pref_id = recorded_pref["receipt_id"].as_i64().unwrap().to_string();
    let inspected_pref = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args(["event-preference-inspect", path.to_str().unwrap(), &pref_id])
        .output()
        .unwrap();
    assert!(inspected_pref.status.success());
    let inspected_pref: serde_json::Value = serde_json::from_slice(&inspected_pref.stdout).unwrap();
    assert_eq!(inspected_pref["signature_rechecked"], false);
    let rechecked_pref = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-preference-recheck",
            path.to_str().unwrap(),
            &pref_id,
            authority_path.to_str().unwrap(),
            &pin_hex,
        ])
        .output()
        .unwrap();
    assert!(rechecked_pref.status.success());
    let rechecked_pref: serde_json::Value = serde_json::from_slice(&rechecked_pref.stdout).unwrap();
    assert_eq!(rechecked_pref["signature_rechecked"], true);
    assert_eq!(rechecked_pref["training_ready"], false);
    let mut changed_preference = preference_json.clone();
    changed_preference["claim"]["evidence"]["rejected"] = "changed".into();
    std::fs::write(
        &preference_path,
        serde_json::to_vec(&changed_preference).unwrap(),
    )
    .unwrap();
    let rejected_preference = run_preference();
    assert!(!rejected_preference.status.success());
    assert!(rejected_preference.stdout.is_empty());
    std::fs::remove_file(preference_path).unwrap();
    assert!(!run_cli(&"0".repeat(64)).status.success());
    let mut altered = bundle.clone();
    altered["trace"]["evidence"]["prompt"] = "changed".into();
    std::fs::write(&bundle_path, serde_json::to_vec(&altered).unwrap()).unwrap();
    let rejected = run_cli(&pin_hex);
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    let mut malformed = bundle.clone();
    malformed["outcome"]["signature"] = "xx".into();
    std::fs::write(&bundle_path, serde_json::to_vec(&malformed).unwrap()).unwrap();
    assert!(!run_cli(&pin_hex).status.success());
    let mut extra = bundle.clone();
    extra["trace"]["evidence"]["unexpected"] = "field".into();
    std::fs::write(&bundle_path, serde_json::to_vec(&extra).unwrap()).unwrap();
    assert!(!run_cli(&pin_hex).status.success());
    std::fs::write(&bundle_path, vec![b' '; 131_073]).unwrap();
    assert!(!run_cli(&pin_hex).status.success());
    std::fs::write(&bundle_path, &raw_bundle).unwrap();
    let invalid_checkpoint = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-attestation-check",
            path.to_str().unwrap(),
            bundle_path.to_str().unwrap(),
            authority_path.to_str().unwrap(),
            &pin_hex,
            &seq,
            &"0".repeat(64),
        ])
        .output()
        .unwrap();
    assert!(!invalid_checkpoint.status.success());
    assert!(invalid_checkpoint.stdout.is_empty());
    std::fs::remove_file(bundle_path).unwrap();
    std::fs::remove_file(authority_path).unwrap();
    assert!(db
        .check_synthetic_attestation_links_in_set(
            trace_id,
            &set,
            NamedSignedClaim {
                evidence: &trace,
                authority_id: "unknown",
                signature: &ts
            },
            NamedSignedClaim {
                evidence: &outcome,
                authority_id: "observer",
                signature: &os
            },
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    let revoked_config=serde_json::to_vec(&serde_json::json!({"schema":"sese-authority-set-v1","authorities":[
        {"id":"trace-source","role":"trace-source","public_key":hex(&source.public_key),"revoked":false},
        {"id":"observer","role":"outcome-observer","public_key":hex(&observer.public_key),"revoked":true}
    ]})).unwrap();
    let revoked_pin: [u8; 32] = Sha256::digest(&revoked_config).into();
    let revoked_set = AuthoritySet::from_pinned_json(&revoked_config, &revoked_pin).unwrap();
    assert!(db
        .check_synthetic_evidence_package(
            receipt,
            preference_receipt,
            &revoked_set,
            (receipt_checkpoint.0, &receipt_checkpoint.1)
        )
        .is_err());
    assert!(db
        .recheck_synthetic_attestation_receipt(receipt, &revoked_set)
        .is_err());
    assert!(db
        .recheck_synthetic_preference_receipt(preference_receipt, &revoked_set)
        .is_err());
    assert!(db
        .check_synthetic_attestation_links_in_set(
            trace_id,
            &revoked_set,
            NamedSignedClaim {
                evidence: &trace,
                authority_id: "trace-source",
                signature: &ts
            },
            NamedSignedClaim {
                evidence: &outcome,
                authority_id: "observer",
                signature: &os
            },
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    assert!(check(&mut db, &trace, &outcome, &old_checkpoint).is_err());
    let bad_checkpoint = (checkpoint.0, "0".repeat(64));
    assert!(check(&mut db, &trace, &outcome, &bad_checkpoint).is_err());
    let bad_signature = [0; 64];
    let valid_outcome_signature = sign_o(&outcome);
    assert!(db
        .check_synthetic_attestation_links(
            trace_id,
            SignedClaim {
                evidence: &trace,
                authority: &source,
                signature: &bad_signature
            },
            SignedClaim {
                evidence: &outcome,
                authority: &observer,
                signature: &valid_outcome_signature
            },
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    let revoked = Authority {
        id: "trace-source".into(),
        role: AuthorityRole::TraceSource,
        public_key: source.public_key,
        revoked: true,
    };
    let valid_trace_signature = sign_t(&trace);
    assert!(db
        .check_synthetic_attestation_links(
            trace_id,
            SignedClaim {
                evidence: &trace,
                authority: &revoked,
                signature: &valid_trace_signature
            },
            SignedClaim {
                evidence: &outcome,
                authority: &observer,
                signature: &valid_outcome_signature
            },
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    trace.prompt = "changed".into();
    assert!(check(&mut db, &trace, &outcome, &checkpoint).is_err());
    trace.prompt = "prompt".into();
    outcome.outcome = SyntheticOutcome::Falsified;
    assert!(check(&mut db, &trace, &outcome, &checkpoint).is_err());
    outcome.outcome = SyntheticOutcome::Validated;
    outcome.observed_unix_ns = review_time.parse::<u64>().unwrap() - 1;
    assert!(check(&mut db, &trace, &outcome, &checkpoint).is_err());
    outcome.observed_unix_ns = outcome_time.parse().unwrap();
    conn.execute("UPDATE event_training_traces SET response='tampered'", [])
        .unwrap();
    assert!(check(&mut db, &trace, &outcome, &checkpoint).is_err());
    conn.execute("UPDATE event_training_traces SET response='response'", [])
        .unwrap();
    assert!(db
        .recheck_synthetic_attestation_receipt(receipt, &set)
        .is_ok());
    conn.execute_batch("CREATE TEMP TABLE saved_preference_receipt AS SELECT * FROM event_preference_receipts WHERE id=1; DELETE FROM event_preference_receipts WHERE id=1;").unwrap();
    assert!(!independent().status.success());
    conn.execute_batch("INSERT INTO event_preference_receipts SELECT * FROM saved_preference_receipt; DROP TABLE saved_preference_receipt;").unwrap();
    assert!(independent().status.success());
    let no_pair: SyntheticTraceInput = serde_json::from_str(r#"{"schema":"sese-synthetic-training-trace-v1","actor":"operator","source":"synthetic source","prompt":"prompt","response":"response"}"#).unwrap();
    let no_pair_id = db.record_synthetic_event_trace(1, &no_pair).unwrap();
    let trace_time: String = conn
        .query_row(
            "SELECT created_unix_ns FROM event_training_traces WHERE id=?1",
            [no_pair_id],
            |r| r.get(0),
        )
        .unwrap();
    let no_pair_trace = SyntheticTraceEvidence {
        reference: trace.reference.clone(),
        prompt: "prompt".into(),
        response: "response".into(),
        source: "synthetic source".into(),
        method: "test fixture".into(),
        captured_unix_ns: trace_time.parse().unwrap(),
    };
    let prior_anchor = db.ledger_checkpoint().unwrap();
    let no_pair_sig = sign_t(&no_pair_trace);
    let no_pair_outcome_sig = sign_o(&outcome);
    let no_pair_receipt = db
        .record_synthetic_attestation_receipt(
            no_pair_id,
            &set,
            NamedSignedClaim {
                evidence: &no_pair_trace,
                authority_id: "trace-source",
                signature: &no_pair_sig,
            },
            NamedSignedClaim {
                evidence: &outcome,
                authority_id: "observer",
                signature: &no_pair_outcome_sig,
            },
            (prior_anchor.0, &prior_anchor.1),
        )
        .unwrap();
    let no_pair_anchor = db.ledger_checkpoint().unwrap();
    let checked = db
        .check_synthetic_common_evidence_policy(
            no_pair_receipt,
            &set,
            &undeclared,
            (no_pair_anchor.0, &no_pair_anchor.1),
        )
        .unwrap();
    assert_eq!(checked["trace_id"], no_pair_id);
    assert_eq!(checked["training_ready"], false);
    conn.execute(
        "DELETE FROM event_attestation_receipts WHERE id=?1",
        [receipt],
    )
    .unwrap();
    assert!(!independent().status.success());
    assert!(db
        .recheck_synthetic_attestation_receipt(receipt, &set)
        .is_err());
    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}
