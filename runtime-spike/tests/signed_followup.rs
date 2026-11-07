use ed25519_dalek::{Signer, SigningKey};
use sese_runtime_spike::attestation::{
    attestation_message, AuthorityRole, AuthoritySet, EvidenceReference, NamedSignedClaim,
    SyntheticOutcome, SyntheticOutcomeEvidence, SyntheticPreferenceEvidence,
    SyntheticTraceEvidence,
};
use sese_runtime_spike::storage::{Storage, SyntheticTraceInput};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn signed_observer_claim_must_reference_the_actual_checkpointed_followup() {
    let path = std::env::temp_dir().join(format!(
        "sese-signed-followup-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut child = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            env!("CARGO_BIN_EXE_event-worker"),
            path.to_str().unwrap(),
            "--followup-fixture",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/fixtures/event-followup-synthetic-v1.json"
            ),
            "--followup-case",
            "post-promotion",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"approve\nrollback\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut db = Storage::open(&path).unwrap();
    let input: SyntheticTraceInput = serde_json::from_str(r#"{"schema":"sese-synthetic-training-trace-v1","actor":"operator","source":"synthetic source","prompt":"prompt","response":"response","preference":{"preferred":"better","rejected":"response","source":"synthetic reviewer"}}"#).unwrap();
    let trace_id = db.record_synthetic_event_trace(1, &input).unwrap();
    let followup_id = db.record_selected_synthetic_followup(1, 2).unwrap();
    let pending_checkpoint = db.ledger_checkpoint().unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (candidate, replay, fixture, review_hash, trace_time, receipt_time, receipt_hash): (String,i64,String,String,String,String,String) = conn.query_row("SELECT r.candidate_id,r.replay_id,p.pack_digest,e.hash,t.created_unix_ns,f.created_unix_ns,ef.hash FROM event_training_traces t JOIN event_reviews r ON r.id=t.review_id JOIN event_replay_runs p ON p.id=r.replay_id JOIN ledger_events e ON e.kind='event_review' AND e.reference=r.id JOIN event_followup_receipts f ON f.review_id=r.id JOIN ledger_events ef ON ef.kind='event_followup_receipt' AND ef.reference=f.id WHERE t.id=?1", [trace_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).unwrap();
    let reference = EvidenceReference {
        review_id: 1,
        replay_id: replay,
        candidate_sha256: candidate,
        fixture_sha256: fixture,
        review_ledger_hash: review_hash,
    };
    let trace = SyntheticTraceEvidence {
        reference: reference.clone(),
        prompt: "prompt".into(),
        response: "response".into(),
        source: "synthetic source".into(),
        method: "fixture".into(),
        captured_unix_ns: trace_time.parse().unwrap(),
    };
    let mut outcome = SyntheticOutcomeEvidence {
        reference,
        outcome: SyntheticOutcome::Validated,
        source: "synthetic result".into(),
        method: "synthetic-followup-receipt-v1".into(),
        evidence_reference: format!("sese-followup:{followup_id}:{receipt_hash}"),
        observed_unix_ns: receipt_time.parse().unwrap(),
    };
    let source_key = SigningKey::from_bytes(&[11; 32]);
    let observer_key = SigningKey::from_bytes(&[12; 32]);
    let reviewer_key = SigningKey::from_bytes(&[13; 32]);
    let hex = |v: &[u8]| v.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let config = serde_json::to_vec(&serde_json::json!({"schema":"sese-authority-set-v1","authorities":[
        {"id":"source","role":"trace-source","public_key":hex(&source_key.verifying_key().to_bytes()),"revoked":false},
        {"id":"observer","role":"outcome-observer","public_key":hex(&observer_key.verifying_key().to_bytes()),"revoked":false},
        {"id":"reviewer","role":"preference-reviewer","public_key":hex(&reviewer_key.verifying_key().to_bytes()),"revoked":false}]})).unwrap();
    let set = AuthoritySet::from_pinned_json(&config, &Sha256::digest(&config).into()).unwrap();
    let trace_sig = source_key
        .sign(
            &attestation_message(
                "source",
                AuthorityRole::TraceSource,
                &trace.canonical_bytes().unwrap(),
            )
            .unwrap(),
        )
        .to_bytes();
    // A valid signed claim must not turn a pending review into an exportable outcome.
    let pending_sig = observer_key
        .sign(
            &attestation_message(
                "observer",
                AuthorityRole::OutcomeObserver,
                &outcome.canonical_bytes().unwrap(),
            )
            .unwrap(),
        )
        .to_bytes();
    let pending = db.record_signed_synthetic_followup(
        trace_id,
        followup_id,
        &set,
        NamedSignedClaim {
            evidence: &trace,
            authority_id: "source",
            signature: &trace_sig,
        },
        NamedSignedClaim {
            evidence: &outcome,
            authority_id: "observer",
            signature: &pending_sig,
        },
        (pending_checkpoint.0, &pending_checkpoint.1),
    );
    assert!(pending.is_err());
    let count: i64 = conn
        .query_row("SELECT count(*) FROM event_attestation_receipts", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    let pending_config_path = path.with_extension("pending-authorities.json");
    let pending_policy_path = path.with_extension("pending-policy.json");
    let pending_rule_path = path.with_extension("pending-rule.json");
    let pending_policy = serde_json::to_vec(&serde_json::json!({"schema":"sese-synthetic-trust-policy-v1","training_export_allowed":true,"authority_config_sha256":hex(&set.digest()),"declarations":[{"trace_source":"source","outcome_observer":"observer","operator":"synthetic operator","observer_relationship_basis":"controlled synthetic observer","preference_reviewer":"reviewer","reviewer_relationship_basis":"controlled synthetic reviewer"}]})).unwrap();
    let pending_rule = include_bytes!("../fixtures/synthetic-reward-rule-v1.json");
    std::fs::write(&pending_config_path, &config).unwrap();
    std::fs::write(&pending_policy_path, &pending_policy).unwrap();
    std::fs::write(&pending_rule_path, pending_rule).unwrap();
    for (format, first, second) in [
        ("sft", "-".to_string(), "-".to_string()),
        ("dpo", "1".to_string(), "-".to_string()),
        (
            "rlvr",
            pending_rule_path.to_str().unwrap().to_string(),
            hex(&Sha256::digest(pending_rule)),
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args([
                "event-training-export",
                path.to_str().unwrap(),
                format,
                "1",
                &followup_id.to_string(),
                pending_config_path.to_str().unwrap(),
                &hex(&Sha256::digest(&config)),
                pending_policy_path.to_str().unwrap(),
                &hex(&Sha256::digest(&pending_policy)),
                &first,
                &second,
                &pending_checkpoint.0.to_string(),
                &pending_checkpoint.1,
                "accept-synthetic-trust-v1",
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let report: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(report["status"], "not_exported");
    }
    std::fs::remove_file(pending_config_path).unwrap();
    std::fs::remove_file(pending_policy_path).unwrap();
    std::fs::remove_file(pending_rule_path).unwrap();
    // This trace is logged before resolution but after the observation receipt.
    let post_observation_trace_id = db.record_synthetic_event_trace(1, &input).unwrap();
    db.resolve_event_review(1, "validated", "operator", "synthetic result")
        .unwrap();
    let checkpoint = db.ledger_checkpoint().unwrap();
    let check = |db: &mut Storage, outcome: &SyntheticOutcomeEvidence, checkpoint: (i64, &str)| {
        let sig = observer_key
            .sign(
                &attestation_message(
                    "observer",
                    AuthorityRole::OutcomeObserver,
                    &outcome.canonical_bytes().unwrap(),
                )
                .unwrap(),
            )
            .to_bytes();
        db.check_signed_synthetic_followup(
            trace_id,
            followup_id,
            &set,
            NamedSignedClaim {
                evidence: &trace,
                authority_id: "source",
                signature: &trace_sig,
            },
            NamedSignedClaim {
                evidence: outcome,
                authority_id: "observer",
                signature: &sig,
            },
            checkpoint,
        )
    };
    let result = check(&mut db, &outcome, (checkpoint.0, &checkpoint.1)).unwrap();
    assert_eq!(result["signed_followup_matches"], true);
    assert_eq!(result["training_ready"], false);
    let observer_sig = observer_key
        .sign(
            &attestation_message(
                "observer",
                AuthorityRole::OutcomeObserver,
                &outcome.canonical_bytes().unwrap(),
            )
            .unwrap(),
        )
        .to_bytes();
    let reference_json = serde_json::json!({"review_id":trace.reference.review_id,"replay_id":trace.reference.replay_id,"candidate_sha256":trace.reference.candidate_sha256,"fixture_sha256":trace.reference.fixture_sha256,"review_ledger_hash":trace.reference.review_ledger_hash});
    let bundle = serde_json::json!({"schema":"sese-synthetic-attestation-bundle-v1","trace_id":trace_id,
        "trace":{"authority_id":"source","signature":hex(&trace_sig),"evidence":{"reference":reference_json,"prompt":trace.prompt,"response":trace.response,"source":trace.source,"method":trace.method,"captured_unix_ns":trace.captured_unix_ns}},
        "outcome":{"authority_id":"observer","signature":hex(&observer_sig),"evidence":{"reference":reference_json,"outcome":"validated","source":outcome.source,"method":outcome.method,"evidence_reference":outcome.evidence_reference,"observed_unix_ns":outcome.observed_unix_ns}}});
    let bundle_path = path.with_extension("bundle.json");
    let config_path = path.with_extension("authorities.json");
    std::fs::write(&bundle_path, serde_json::to_vec(&bundle).unwrap()).unwrap();
    std::fs::write(&config_path, &config).unwrap();
    let cli = |pin: &str| {
        Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args([
                "event-signed-followup-check",
                path.to_str().unwrap(),
                &followup_id.to_string(),
                bundle_path.to_str().unwrap(),
                config_path.to_str().unwrap(),
                pin,
                &checkpoint.0.to_string(),
                &checkpoint.1,
            ])
            .output()
            .unwrap()
    };
    let checked = cli(&hex(&Sha256::digest(&config)));
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let checked: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(checked["signed_followup_matches"], true);
    assert_eq!(checked["training_ready"], false);
    let bad = cli(&"0".repeat(64));
    assert!(!bad.status.success());
    assert!(bad.stdout.is_empty());
    let record = |db: &mut Storage| {
        db.record_signed_synthetic_followup(
            trace_id,
            followup_id,
            &set,
            NamedSignedClaim {
                evidence: &trace,
                authority_id: "source",
                signature: &trace_sig,
            },
            NamedSignedClaim {
                evidence: &outcome,
                authority_id: "observer",
                signature: &observer_sig,
            },
            (checkpoint.0, &checkpoint.1),
        )
    };
    conn.execute_batch("CREATE TRIGGER reject_signed_followup BEFORE INSERT ON ledger_events WHEN NEW.kind='event_attestation_receipt' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(record(&mut db).is_err());
    let count: i64 = conn
        .query_row("SELECT count(*) FROM event_attestation_receipts", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    conn.execute_batch("DROP TRIGGER reject_signed_followup")
        .unwrap();
    let receipt_id = record(&mut db).unwrap();
    assert!(record(&mut db).is_err());
    let attestation_checkpoint = db.ledger_checkpoint().unwrap();
    let trace_hash: String = conn
        .query_row(
            "SELECT hash FROM ledger_events WHERE kind='event_training_trace' AND reference=?1",
            [trace_id],
            |r| r.get(0),
        )
        .unwrap();
    let preference = SyntheticPreferenceEvidence {
        reference: trace.reference.clone(),
        trace_id,
        trace_ledger_hash: trace_hash,
        prompt: "prompt".into(),
        preferred: "better".into(),
        rejected: "response".into(),
        source: "synthetic reviewer".into(),
        comparability_basis: "same synthetic prompt".into(),
        reviewed_unix_ns: trace.captured_unix_ns,
    };
    let preference_sig = reviewer_key
        .sign(
            &attestation_message(
                "reviewer",
                AuthorityRole::PreferenceReviewer,
                &preference.canonical_bytes().unwrap(),
            )
            .unwrap(),
        )
        .to_bytes();
    let preference_id = db
        .record_synthetic_preference_receipt(
            &set,
            NamedSignedClaim {
                evidence: &preference,
                authority_id: "reviewer",
                signature: &preference_sig,
            },
            (checkpoint.0, &checkpoint.1),
        )
        .unwrap();
    drop(db);
    let mut db = Storage::open(&path).unwrap();
    let receipt_checkpoint = db.ledger_checkpoint().unwrap();
    let recheck = |db: &mut Storage| {
        db.recheck_signed_synthetic_followup(
            receipt_id,
            followup_id,
            &set,
            NamedSignedClaim {
                evidence: &trace,
                authority_id: "source",
                signature: &trace_sig,
            },
            NamedSignedClaim {
                evidence: &outcome,
                authority_id: "observer",
                signature: &observer_sig,
            },
            (receipt_checkpoint.0, &receipt_checkpoint.1),
        )
    };
    let rechecked = recheck(&mut db).unwrap();
    assert_eq!(rechecked["signed_followup_matches"], true);
    assert_eq!(rechecked["training_ready"], false);
    let retained = db
        .recheck_retained_signed_followup(
            receipt_id,
            followup_id,
            &set,
            (receipt_checkpoint.0, &receipt_checkpoint.1),
        )
        .unwrap();
    assert_eq!(retained["signed_followup_matches"], true);
    assert_eq!(retained["training_ready"], false);
    let policy_bytes = serde_json::to_vec(&serde_json::json!({"schema":"sese-synthetic-trust-policy-v1","training_export_allowed":true, "authority_config_sha256":hex(&set.digest()),
        "declarations":[{"trace_source":"source","outcome_observer":"observer","preference_reviewer":"reviewer",
        "operator":"synthetic operator","observer_relationship_basis":"controlled synthetic observer","reviewer_relationship_basis":"controlled synthetic reviewer"}]})).unwrap();
    let policy = sese_runtime_spike::attestation::SyntheticTrustPolicy::from_pinned_json(
        &policy_bytes,
        &Sha256::digest(&policy_bytes).into(),
        &set,
    )
    .unwrap();
    let rule_bytes = include_bytes!("../fixtures/synthetic-reward-rule-v1.json");
    let rule = sese_runtime_spike::reward_rule::SyntheticRewardRule::from_pinned_json(
        rule_bytes,
        &Sha256::digest(rule_bytes).into(),
    )
    .unwrap();
    let mut inspection_policy: serde_json::Value = serde_json::from_slice(&policy_bytes).unwrap();
    inspection_policy
        .as_object_mut()
        .unwrap()
        .remove("training_export_allowed");
    let inspection_bytes = serde_json::to_vec(&inspection_policy).unwrap();
    let inspection_policy =
        sese_runtime_spike::attestation::SyntheticTrustPolicy::from_pinned_json(
            &inspection_bytes,
            &Sha256::digest(&inspection_bytes).into(),
            &set,
        )
        .unwrap();
    for format in ["sft", "rlvr"] {
        let result = db.export_conditionally_trusted_synthetic_training(
            format,
            receipt_id,
            followup_id,
            &set,
            &inspection_policy,
            Some(&rule),
            true,
            (receipt_checkpoint.0, &receipt_checkpoint.1),
        );
        assert!(result
            .unwrap_err()
            .contains("policy does not permit training export"));
    }
    let dpo = db.export_conditionally_trusted_synthetic_preference(
        receipt_id,
        followup_id,
        preference_id,
        &set,
        &inspection_policy,
        true,
        (receipt_checkpoint.0, &receipt_checkpoint.1),
    );
    assert!(dpo
        .unwrap_err()
        .contains("policy does not permit training export"));
    let preview = db
        .preview_synthetic_training(
            "sft",
            receipt_id,
            followup_id,
            &set,
            &inspection_policy,
            None,
            true,
            (receipt_checkpoint.0, &receipt_checkpoint.1),
        )
        .unwrap();
    assert_eq!(preview["export_kind"], "inspection_only");
    assert_eq!(preview["training_export_allowed_by_policy"], false);
    assert_eq!(
        preview["synthetic_training_eligible_under_operator_assumptions"],
        false
    );
    assert!(db
        .preview_synthetic_training(
            "sft",
            receipt_id,
            followup_id,
            &set,
            &policy,
            None,
            true,
            (receipt_checkpoint.0, &receipt_checkpoint.1)
        )
        .is_err());
    let export = |db: &mut Storage, format: &str, accepted: bool, anchor: (i64, &str)| {
        db.export_conditionally_trusted_synthetic_training(
            format,
            receipt_id,
            followup_id,
            &set,
            &policy,
            Some(&rule),
            accepted,
            anchor,
        )
    };
    assert!(export(
        &mut db,
        "sft",
        false,
        (receipt_checkpoint.0, &receipt_checkpoint.1)
    )
    .is_err());
    let sft = export(
        &mut db,
        "sft",
        true,
        (receipt_checkpoint.0, &receipt_checkpoint.1),
    )
    .unwrap();
    assert_eq!(sft["schema"], "sese-synthetic-sft-v1");
    assert_eq!(sft["messages"][0]["content"], "prompt");
    assert_eq!(sft["messages"][1]["content"], "response");
    assert_eq!(sft["synthetic"], true);
    assert_eq!(sft["trust_basis"], "operator_declared");
    assert_eq!(sft["training_export_allowed_by_policy"], true);
    assert_eq!(sft["observer_independence_verified"], false);
    assert_eq!(sft["training_eligibility_independently_verified"], false);
    assert_eq!(
        sft["synthetic_training_eligible_under_operator_assumptions"],
        true
    );
    let rlvr = export(
        &mut db,
        "rlvr",
        true,
        (receipt_checkpoint.0, &receipt_checkpoint.1),
    )
    .unwrap();
    assert_eq!(rlvr["schema"], "sese-synthetic-rlvr-v1");
    assert_eq!(rlvr["reward"], 1);
    assert_eq!(rlvr["reward_rule"]["version"], "outcome-sign-v1");
    assert!(export(
        &mut db,
        "dpo",
        true,
        (receipt_checkpoint.0, &receipt_checkpoint.1)
    )
    .is_err());
    let dpo = db
        .export_conditionally_trusted_synthetic_preference(
            receipt_id,
            followup_id,
            preference_id,
            &set,
            &policy,
            true,
            (receipt_checkpoint.0, &receipt_checkpoint.1),
        )
        .unwrap();
    assert_eq!(dpo["schema"], "sese-synthetic-dpo-v1");
    assert_eq!(dpo["prompt"], "prompt");
    assert_eq!(dpo["chosen"], "better");
    assert_eq!(dpo["rejected"], "response");
    assert_eq!(dpo["comparability_verified"], false);
    // A source may backdate its signed claim; a trace first recorded after the
    // outcome must not become a training example merely on that assertion.
    let late_trace_id = db.record_synthetic_event_trace(1, &input).unwrap();
    let late_checkpoint = db.ledger_checkpoint().unwrap();
    let late_receipt = db
        .record_signed_synthetic_followup(
            late_trace_id,
            followup_id,
            &set,
            NamedSignedClaim {
                evidence: &trace,
                authority_id: "source",
                signature: &trace_sig,
            },
            NamedSignedClaim {
                evidence: &outcome,
                authority_id: "observer",
                signature: &observer_sig,
            },
            (late_checkpoint.0, &late_checkpoint.1),
        )
        .unwrap();
    let late_anchor = db.ledger_checkpoint().unwrap();
    for format in ["sft", "rlvr"] {
        let rejected = db.export_conditionally_trusted_synthetic_training(
            format,
            late_receipt,
            followup_id,
            &set,
            &policy,
            Some(&rule),
            true,
            (late_anchor.0, &late_anchor.1),
        );
        assert!(
            rejected.is_err(),
            "post-outcome trace exported for {format}"
        );
    }
    let late_trace_hash: String = conn
        .query_row(
            "SELECT hash FROM ledger_events WHERE kind='event_training_trace' AND reference=?1",
            [late_trace_id],
            |r| r.get(0),
        )
        .unwrap();
    let late_trace_time: String = conn
        .query_row(
            "SELECT created_unix_ns FROM event_training_traces WHERE id=?1",
            [late_trace_id],
            |r| r.get(0),
        )
        .unwrap();
    let late_preference = SyntheticPreferenceEvidence {
        reference: trace.reference.clone(),
        trace_id: late_trace_id,
        trace_ledger_hash: late_trace_hash,
        prompt: "prompt".into(),
        preferred: "better".into(),
        rejected: "response".into(),
        source: "synthetic reviewer".into(),
        comparability_basis: "same synthetic prompt".into(),
        reviewed_unix_ns: late_trace_time.parse().unwrap(),
    };
    let late_preference_sig = reviewer_key
        .sign(
            &attestation_message(
                "reviewer",
                AuthorityRole::PreferenceReviewer,
                &late_preference.canonical_bytes().unwrap(),
            )
            .unwrap(),
        )
        .to_bytes();
    let late_preference_id = db
        .record_synthetic_preference_receipt(
            &set,
            NamedSignedClaim {
                evidence: &late_preference,
                authority_id: "reviewer",
                signature: &late_preference_sig,
            },
            (late_anchor.0, &late_anchor.1),
        )
        .unwrap();
    let late_anchor = db.ledger_checkpoint().unwrap();
    let rejected = db.export_conditionally_trusted_synthetic_preference(
        late_receipt,
        followup_id,
        late_preference_id,
        &set,
        &policy,
        true,
        (late_anchor.0, &late_anchor.1),
    );
    assert!(rejected
        .unwrap_err()
        .contains("training trace must precede"));

    // A trace in the ledger before the outcome can still have a signed capture
    // time after the observed receipt. This independent time check must fail.
    let post_time: String = conn
        .query_row(
            "SELECT created_unix_ns FROM event_training_traces WHERE id=?1",
            [post_observation_trace_id],
            |r| r.get(0),
        )
        .unwrap();
    let post_trace = SyntheticTraceEvidence {
        reference: trace.reference.clone(),
        prompt: trace.prompt.clone(),
        response: trace.response.clone(),
        source: trace.source.clone(),
        method: trace.method.clone(),
        captured_unix_ns: post_time.parse().unwrap(),
    };
    assert!(post_trace.captured_unix_ns > outcome.observed_unix_ns);
    let post_sig = source_key
        .sign(
            &attestation_message(
                "source",
                AuthorityRole::TraceSource,
                &post_trace.canonical_bytes().unwrap(),
            )
            .unwrap(),
        )
        .to_bytes();
    let post_receipt = db
        .record_signed_synthetic_followup(
            post_observation_trace_id,
            followup_id,
            &set,
            NamedSignedClaim {
                evidence: &post_trace,
                authority_id: "source",
                signature: &post_sig,
            },
            NamedSignedClaim {
                evidence: &outcome,
                authority_id: "observer",
                signature: &observer_sig,
            },
            (late_anchor.0, &late_anchor.1),
        )
        .unwrap();
    let post_anchor = db.ledger_checkpoint().unwrap();
    let rejected = db.export_conditionally_trusted_synthetic_training(
        "sft",
        post_receipt,
        followup_id,
        &set,
        &policy,
        None,
        true,
        (post_anchor.0, &post_anchor.1),
    );
    assert!(rejected
        .unwrap_err()
        .contains("training trace must precede"));
    let mut no_reviewer: serde_json::Value = serde_json::from_slice(&policy_bytes).unwrap();
    no_reviewer["declarations"][0]["preference_reviewer"] = serde_json::Value::Null;
    no_reviewer["declarations"][0]["reviewer_relationship_basis"] = serde_json::Value::Null;
    let no_reviewer = serde_json::to_vec(&no_reviewer).unwrap();
    let no_reviewer = sese_runtime_spike::attestation::SyntheticTrustPolicy::from_pinned_json(
        &no_reviewer,
        &Sha256::digest(&no_reviewer).into(),
        &set,
    )
    .unwrap();
    assert!(db
        .export_conditionally_trusted_synthetic_preference(
            receipt_id,
            followup_id,
            preference_id,
            &set,
            &no_reviewer,
            true,
            (receipt_checkpoint.0, &receipt_checkpoint.1)
        )
        .is_err());
    let future = SyntheticPreferenceEvidence {
        reference: trace.reference.clone(),
        trace_id,
        trace_ledger_hash: preference.trace_ledger_hash.clone(),
        prompt: "prompt".into(),
        preferred: "better".into(),
        rejected: "response".into(),
        source: "synthetic reviewer".into(),
        comparability_basis: "same synthetic prompt".into(),
        reviewed_unix_ns: u64::MAX,
    };
    let future_sig = reviewer_key
        .sign(
            &attestation_message(
                "reviewer",
                AuthorityRole::PreferenceReviewer,
                &future.canonical_bytes().unwrap(),
            )
            .unwrap(),
        )
        .to_bytes();
    let future_id = db
        .record_synthetic_preference_receipt(
            &set,
            NamedSignedClaim {
                evidence: &future,
                authority_id: "reviewer",
                signature: &future_sig,
            },
            (checkpoint.0, &checkpoint.1),
        )
        .unwrap();
    let future_checkpoint = db.ledger_checkpoint().unwrap();
    assert!(db
        .export_conditionally_trusted_synthetic_preference(
            receipt_id,
            followup_id,
            future_id,
            &set,
            &policy,
            true,
            (future_checkpoint.0, &future_checkpoint.1)
        )
        .is_err());
    assert!(db
        .export_conditionally_trusted_synthetic_preference(
            receipt_id,
            followup_id,
            preference_id,
            &set,
            &policy,
            true,
            (attestation_checkpoint.0, &attestation_checkpoint.1)
        )
        .is_err());
    assert!(db
        .export_conditionally_trusted_synthetic_preference(
            receipt_id,
            followup_id,
            preference_id,
            &set,
            &policy,
            false,
            (receipt_checkpoint.0, &receipt_checkpoint.1)
        )
        .is_err());
    assert!(export(&mut db, "sft", true, (checkpoint.0, &checkpoint.1)).is_err());
    assert!(db
        .export_conditionally_trusted_synthetic_training(
            "rlvr",
            receipt_id,
            followup_id,
            &set,
            &policy,
            None,
            true,
            (receipt_checkpoint.0, &receipt_checkpoint.1)
        )
        .is_err());
    assert!(db
        .export_conditionally_trusted_synthetic_training(
            "sft",
            999999,
            followup_id,
            &set,
            &policy,
            None,
            true,
            (receipt_checkpoint.0, &receipt_checkpoint.1)
        )
        .is_err());
    let policy_path = path.with_extension("policy.json");
    let rule_path = path.with_extension("rule.json");
    std::fs::write(&policy_path, &policy_bytes).unwrap();
    std::fs::write(&rule_path, rule_bytes).unwrap();
    let policy_pin = hex(&Sha256::digest(&policy_bytes));
    let rule_pin = hex(&Sha256::digest(rule_bytes));
    let config_pin = hex(&Sha256::digest(&config));
    let cli_export = |format: &str, accepted: &str, rule_file: &str, rule_digest: &str| {
        Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args([
                "event-training-export",
                path.to_str().unwrap(),
                format,
                &receipt_id.to_string(),
                &followup_id.to_string(),
                config_path.to_str().unwrap(),
                &config_pin,
                policy_path.to_str().unwrap(),
                &policy_pin,
                rule_file,
                rule_digest,
                &receipt_checkpoint.0.to_string(),
                &receipt_checkpoint.1,
                accepted,
            ])
            .output()
            .unwrap()
    };
    let cli_sft = cli_export("sft", "accept-synthetic-trust-v1", "-", "-");
    assert!(
        cli_sft.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_sft.stderr)
    );
    let row: serde_json::Value = serde_json::from_slice(&cli_sft.stdout).unwrap();
    assert_eq!(row["messages"][1]["content"], "response");
    let report: serde_json::Value = serde_json::from_slice(&cli_sft.stderr).unwrap();
    assert_eq!(report["status"], "included");
    let cli_rlvr = cli_export(
        "rlvr",
        "accept-synthetic-trust-v1",
        rule_path.to_str().unwrap(),
        &rule_pin,
    );
    assert!(
        cli_rlvr.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_rlvr.stderr)
    );
    let row: serde_json::Value = serde_json::from_slice(&cli_rlvr.stdout).unwrap();
    assert_eq!(row["reward"], 1);
    let bad_rule = cli_export(
        "rlvr",
        "accept-synthetic-trust-v1",
        rule_path.to_str().unwrap(),
        &"0".repeat(64),
    );
    assert!(!bad_rule.status.success());
    assert!(bad_rule.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bad_rule.stderr).unwrap()["status"],
        "not_exported"
    );
    let cli_dpo = cli_export(
        "dpo",
        "accept-synthetic-trust-v1",
        &preference_id.to_string(),
        "-",
    );
    assert!(
        cli_dpo.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_dpo.stderr)
    );
    let row: serde_json::Value = serde_json::from_slice(&cli_dpo.stdout).unwrap();
    assert_eq!(row["chosen"], "better");
    assert_eq!(row["rejected"], "response");
    let absent_pair = cli_export("dpo", "accept-synthetic-trust-v1", "999999", "-");
    assert!(!absent_pair.status.success());
    assert!(absent_pair.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&absent_pair.stderr).unwrap()["status"],
        "not_exported"
    );
    conn.execute(
        "UPDATE event_preference_receipts SET payload=x'00' WHERE id=?1",
        [preference_id],
    )
    .unwrap();
    let tampered_pair = cli_export(
        "dpo",
        "accept-synthetic-trust-v1",
        &preference_id.to_string(),
        "-",
    );
    assert!(!tampered_pair.status.success());
    assert!(tampered_pair.stdout.is_empty());
    conn.execute(
        "UPDATE event_preference_receipts SET payload=?1 WHERE id=?2",
        rusqlite::params![preference.canonical_bytes().unwrap(), preference_id],
    )
    .unwrap();
    let declined = cli_export("sft", "not-accepted", "-", "-");
    assert!(!declined.status.success());
    assert!(declined.stdout.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&declined.stderr).unwrap();
    assert_eq!(report["status"], "not_exported");
    std::fs::write(&policy_path, b"{}").unwrap();
    let changed_policy = cli_export("sft", "accept-synthetic-trust-v1", "-", "-");
    assert!(!changed_policy.status.success());
    assert!(changed_policy.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&changed_policy.stderr).unwrap()["status"],
        "not_exported"
    );
    std::fs::write(&policy_path, &policy_bytes).unwrap();
    std::fs::write(&config_path, b"{}").unwrap();
    let changed_authorities = cli_export("sft", "accept-synthetic-trust-v1", "-", "-");
    assert!(!changed_authorities.status.success());
    assert!(changed_authorities.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&changed_authorities.stderr).unwrap()["status"],
        "not_exported"
    );
    std::fs::write(&config_path, &config).unwrap();
    std::fs::remove_file(policy_path).unwrap();
    std::fs::remove_file(rule_path).unwrap();
    assert!(db
        .recheck_retained_signed_followup(
            receipt_id,
            followup_id,
            &set,
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    assert!(db
        .recheck_signed_synthetic_followup(
            receipt_id,
            followup_id,
            &set,
            NamedSignedClaim {
                evidence: &trace,
                authority_id: "source",
                signature: &trace_sig
            },
            NamedSignedClaim {
                evidence: &outcome,
                authority_id: "observer",
                signature: &observer_sig
            },
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    let pin = hex(&Sha256::digest(&config));
    let retained_cli = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-retained-followup-check",
            path.to_str().unwrap(),
            &receipt_id.to_string(),
            &followup_id.to_string(),
            config_path.to_str().unwrap(),
            &pin,
            &receipt_checkpoint.0.to_string(),
            &receipt_checkpoint.1,
        ])
        .output()
        .unwrap();
    assert!(
        retained_cli.status.success(),
        "{}",
        String::from_utf8_lossy(&retained_cli.stderr)
    );
    let retained_cli: serde_json::Value = serde_json::from_slice(&retained_cli.stdout).unwrap();
    assert_eq!(retained_cli["retained_claims_rechecked"], true);
    assert_eq!(retained_cli["training_ready"], false);
    let recheck_cli = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-signed-followup-recheck",
            path.to_str().unwrap(),
            &receipt_id.to_string(),
            &followup_id.to_string(),
            bundle_path.to_str().unwrap(),
            config_path.to_str().unwrap(),
            &pin,
            &receipt_checkpoint.0.to_string(),
            &receipt_checkpoint.1,
        ])
        .output()
        .unwrap();
    assert!(
        recheck_cli.status.success(),
        "{}",
        String::from_utf8_lossy(&recheck_cli.stderr)
    );
    let recheck_cli: serde_json::Value = serde_json::from_slice(&recheck_cli.stdout).unwrap();
    assert_eq!(recheck_cli["retained_claim_matches"], true);
    let mut wrong_bundle = bundle.clone();
    wrong_bundle["trace_id"] = (trace_id + 1).into();
    std::fs::write(&bundle_path, serde_json::to_vec(&wrong_bundle).unwrap()).unwrap();
    let bad_bundle = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-signed-followup-recheck",
            path.to_str().unwrap(),
            &receipt_id.to_string(),
            &followup_id.to_string(),
            bundle_path.to_str().unwrap(),
            config_path.to_str().unwrap(),
            &pin,
            &receipt_checkpoint.0.to_string(),
            &receipt_checkpoint.1,
        ])
        .output()
        .unwrap();
    assert!(!bad_bundle.status.success());
    assert!(bad_bundle.stdout.is_empty());
    std::fs::write(&bundle_path, serde_json::to_vec(&bundle).unwrap()).unwrap();
    let record_cli = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-signed-followup-record",
            path.to_str().unwrap(),
            &followup_id.to_string(),
            bundle_path.to_str().unwrap(),
            config_path.to_str().unwrap(),
            &pin,
            &receipt_checkpoint.0.to_string(),
            &receipt_checkpoint.1,
        ])
        .output()
        .unwrap();
    assert!(
        record_cli.status.success(),
        "{}",
        String::from_utf8_lossy(&record_cli.stderr)
    );
    let record_cli: serde_json::Value = serde_json::from_slice(&record_cli.stdout).unwrap();
    assert_eq!(record_cli["training_ready"], false);
    std::fs::remove_file(bundle_path).unwrap();
    std::fs::remove_file(config_path).unwrap();
    conn.execute(
        "UPDATE event_attestation_receipts SET outcome_payload=x'00' WHERE id=?1",
        [receipt_id],
    )
    .unwrap();
    assert!(recheck(&mut db).is_err());
    assert!(export(
        &mut db,
        "sft",
        true,
        (receipt_checkpoint.0, &receipt_checkpoint.1)
    )
    .is_err());
    assert!(db
        .recheck_retained_signed_followup(
            receipt_id,
            followup_id,
            &set,
            (receipt_checkpoint.0, &receipt_checkpoint.1)
        )
        .is_err());
    conn.execute(
        "UPDATE event_attestation_receipts SET outcome_payload=?1 WHERE id=?2",
        rusqlite::params![outcome.canonical_bytes().unwrap(), receipt_id],
    )
    .unwrap();
    assert!(recheck(&mut db).is_ok());
    outcome.evidence_reference = format!("sese-followup:{followup_id}:{}", "0".repeat(64));
    assert!(check(&mut db, &outcome, (checkpoint.0, &checkpoint.1)).is_err());
    outcome.evidence_reference = format!("sese-followup:{followup_id}:{receipt_hash}");
    outcome.method = "unrelated method".into();
    assert!(check(&mut db, &outcome, (checkpoint.0, &checkpoint.1)).is_err());
    outcome.method = "synthetic-followup-receipt-v1".into();
    outcome.observed_unix_ns -= 1;
    assert!(check(&mut db, &outcome, (checkpoint.0, &checkpoint.1)).is_err());
    outcome.observed_unix_ns += 1;
    assert!(check(&mut db, &outcome, (checkpoint.0, &"0".repeat(64))).is_err());
    outcome.outcome = SyntheticOutcome::Falsified;
    assert!(check(&mut db, &outcome, (checkpoint.0, &checkpoint.1)).is_err());
    outcome.outcome = SyntheticOutcome::Validated;
    let mut invalid_signature = observer_sig;
    invalid_signature[0] ^= 1;
    assert!(db
        .check_signed_synthetic_followup(
            trace_id,
            followup_id,
            &set,
            NamedSignedClaim {
                evidence: &trace,
                authority_id: "source",
                signature: &trace_sig
            },
            NamedSignedClaim {
                evidence: &outcome,
                authority_id: "observer",
                signature: &invalid_signature
            },
            (checkpoint.0, &checkpoint.1)
        )
        .is_err());
    drop(conn);
    drop(db);
    std::fs::remove_file(path).unwrap();
}
