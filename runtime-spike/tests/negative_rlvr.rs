#[cfg(unix)]
#[test]
fn synthetic_worker_fault_produces_falsified_followup_and_negative_rlvr() {
    use ed25519_dalek::{Signer, SigningKey};
    use sese_runtime_spike::attestation::{
        attestation_message, AuthorityRole, AuthoritySet, EvidenceReference, NamedSignedClaim,
        SyntheticOutcome, SyntheticOutcomeEvidence, SyntheticPreferenceEvidence,
        SyntheticTraceEvidence, SyntheticTrustPolicy,
    };
    use sese_runtime_spike::event_abi::Event;
    use sese_runtime_spike::event_host::EventHost;
    use sese_runtime_spike::reward_rule::SyntheticRewardRule;
    use sese_runtime_spike::storage::{Storage, SyntheticTraceInput};
    use sha2::{Digest, Sha256};
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};
    use std::time::Duration;
    let dir = std::env::temp_dir().join(format!(
        "sese-negative-rlvr-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("db.sqlite");
    let worker = dir.join("controlled-worker.py");
    let fault = dir.join("inject-fault");
    // Known synthetic test wrapper. It forwards every request to the real event
    // worker, then changes the returned score only when the test creates a marker.
    // The same wrapper bytes are used throughout; this demonstrates why the
    // recorded worker digest does not attest behavior or external dependencies.
    let script = format!("#!/usr/bin/python3\nimport os,struct,subprocess,sys\ndata=sys.stdin.buffer.read()\nr=subprocess.run([{}],input=data,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)\nif r.returncode: sys.exit(r.returncode)\nout=bytearray(r.stdout)\nif os.path.exists({}) and len(out)==16: struct.pack_into('<d',out,4,99.0)\nsys.stdout.buffer.write(out)\n",serde_json::to_string(env!("CARGO_BIN_EXE_event-worker")).unwrap(),serde_json::to_string(fault.to_str().unwrap()).unwrap());
    std::fs::write(&worker, script).unwrap();
    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut demo = Command::new(env!("CARGO_BIN_EXE_sese-event-cycle-demo"))
        .args([
            worker.to_str().unwrap(),
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
    demo.stdin
        .take()
        .unwrap()
        .write_all(b"approve\nkeep\n")
        .unwrap();
    let result = demo.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let conn = rusqlite::Connection::open(&path).unwrap();
    let original: String = conn
        .query_row(
            "SELECT champion_id FROM event_observations WHERE id=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut db = Storage::open(&path).unwrap();
    let input: SyntheticTraceInput = serde_json::from_str(r#"{"schema":"sese-synthetic-training-trace-v1","actor":"operator","source":"synthetic source","prompt":"prompt","response":"response","preference":{"preferred":"better","rejected":"response","source":"synthetic reviewer"}}"#).unwrap();
    let trace_id = db.record_synthetic_event_trace(1, &input).unwrap();
    std::fs::write(&fault, b"synthetic test fault").unwrap();
    let mut host = EventHost::open(&mut db, "fault-stream", 32).unwrap();
    let event = Event {
        timestamp_ns: 1,
        entity_id: 42,
        metric: 3.0,
        flags: 4,
    };
    host.process_domain(
        &mut db,
        worker.to_str().unwrap(),
        "typed-cycle",
        &original,
        &event,
        0,
        Duration::from_secs(2),
    )
    .unwrap();
    let observed = host
        .process_domain(
            &mut db,
            worker.to_str().unwrap(),
            "typed-cycle",
            &original,
            &event,
            0,
            Duration::from_secs(2),
        )
        .unwrap();
    let followup_id = db
        .record_selected_synthetic_followup(1, observed.id)
        .unwrap();
    let receipt = db.inspect_synthetic_followup_receipt(followup_id).unwrap();
    assert_eq!(receipt["comparison"]["status"], "verdict_mismatch");
    db.resolve_event_review(1, "falsified", "operator", "synthetic result")
        .unwrap();
    let checkpoint = db.ledger_checkpoint().unwrap();
    let (candidate,replay,fixture,review_hash,trace_time,follow_time,follow_hash): (String,i64,String,String,String,String,String) = conn.query_row("SELECT r.candidate_id,r.replay_id,p.pack_digest,e.hash,t.created_unix_ns,f.created_unix_ns,ef.hash FROM event_training_traces t JOIN event_reviews r ON r.id=t.review_id JOIN event_replay_runs p ON p.id=r.replay_id JOIN ledger_events e ON e.kind='event_review' AND e.reference=r.id JOIN event_followup_receipts f ON f.review_id=r.id JOIN ledger_events ef ON ef.kind='event_followup_receipt' AND ef.reference=f.id WHERE t.id=?1",[trace_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).unwrap();
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
    let outcome = SyntheticOutcomeEvidence {
        reference,
        outcome: SyntheticOutcome::Falsified,
        source: "synthetic result".into(),
        method: "synthetic-followup-receipt-v1".into(),
        evidence_reference: format!("sese-followup:{followup_id}:{follow_hash}"),
        observed_unix_ns: follow_time.parse().unwrap(),
    };
    let source_key = SigningKey::from_bytes(&[21; 32]);
    let observer_key = SigningKey::from_bytes(&[22; 32]);
    let reviewer_key = SigningKey::from_bytes(&[23; 32]);
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let config=serde_json::to_vec(&serde_json::json!({"schema":"sese-authority-set-v1","authorities":[{"id":"source","role":"trace-source","public_key":hex(&source_key.verifying_key().to_bytes()),"revoked":false},{"id":"observer","role":"outcome-observer","public_key":hex(&observer_key.verifying_key().to_bytes()),"revoked":false},{"id":"reviewer","role":"preference-reviewer","public_key":hex(&reviewer_key.verifying_key().to_bytes()),"revoked":false}]})).unwrap();
    let set = AuthoritySet::from_pinned_json(&config, &Sha256::digest(&config).into()).unwrap();
    let sign = |key: &SigningKey, id: &str, role: AuthorityRole, bytes: &[u8]| {
        key.sign(&attestation_message(id, role, bytes).unwrap())
            .to_bytes()
    };
    let trace_sig = sign(
        &source_key,
        "source",
        AuthorityRole::TraceSource,
        &trace.canonical_bytes().unwrap(),
    );
    let outcome_sig = sign(
        &observer_key,
        "observer",
        AuthorityRole::OutcomeObserver,
        &outcome.canonical_bytes().unwrap(),
    );
    let attestation = db
        .record_signed_synthetic_followup(
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
                signature: &outcome_sig,
            },
            (checkpoint.0, &checkpoint.1),
        )
        .unwrap();
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
    let preference_sig = sign(
        &reviewer_key,
        "reviewer",
        AuthorityRole::PreferenceReviewer,
        &preference.canonical_bytes().unwrap(),
    );
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
    let anchor = db.ledger_checkpoint().unwrap();
    let policy_bytes=serde_json::to_vec(&serde_json::json!({"schema":"sese-synthetic-trust-policy-v1","training_export_allowed":true,"authority_config_sha256":hex(&set.digest()),"declarations":[{"trace_source":"source","outcome_observer":"observer","operator":"test operator","observer_relationship_basis":"controlled test keys","preference_reviewer":"reviewer","reviewer_relationship_basis":"controlled test reviewer"}]})).unwrap();
    let policy = SyntheticTrustPolicy::from_pinned_json(
        &policy_bytes,
        &Sha256::digest(&policy_bytes).into(),
        &set,
    )
    .unwrap();
    let rule_bytes = include_bytes!("../fixtures/synthetic-reward-rule-v1.json");
    let rule =
        SyntheticRewardRule::from_pinned_json(rule_bytes, &Sha256::digest(rule_bytes).into())
            .unwrap();
    let row = db
        .export_conditionally_trusted_synthetic_training(
            "rlvr",
            attestation,
            followup_id,
            &set,
            &policy,
            Some(&rule),
            true,
            (anchor.0, &anchor.1),
        )
        .unwrap();
    assert_eq!(row["reward"], -1);
    assert_eq!(row["outcome"]["status"], "falsified");
    assert_eq!(row["fixture_independence_verified"], false);
    assert!(db
        .export_conditionally_trusted_synthetic_training(
            "sft",
            attestation,
            followup_id,
            &set,
            &policy,
            None,
            true,
            (anchor.0, &anchor.1)
        )
        .is_err());
    let dpo_error = db
        .export_conditionally_trusted_synthetic_preference(
            attestation,
            followup_id,
            preference_id,
            &set,
            &policy,
            true,
            (anchor.0, &anchor.1),
        )
        .unwrap_err();
    assert!(
        dpo_error.contains("sft requires validated outcome"),
        "{dpo_error}"
    );
    let config_path = dir.join("authorities.json");
    let policy_path = dir.join("policy.json");
    let rule_path = dir.join("rule.json");
    std::fs::write(&config_path, &config).unwrap();
    std::fs::write(&policy_path, &policy_bytes).unwrap();
    std::fs::write(&rule_path, rule_bytes).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
        .args([
            "event-training-export",
            path.to_str().unwrap(),
            "rlvr",
            &attestation.to_string(),
            &followup_id.to_string(),
            config_path.to_str().unwrap(),
            &hex(&Sha256::digest(&config)),
            policy_path.to_str().unwrap(),
            &hex(&Sha256::digest(&policy_bytes)),
            rule_path.to_str().unwrap(),
            &hex(&Sha256::digest(rule_bytes)),
            &anchor.0.to_string(),
            &anchor.1,
            "accept-synthetic-trust-v1",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli_row: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(cli_row["reward"], -1);
    assert_eq!(cli_row["outcome"]["status"], "falsified");
    if Command::new("python3")
        .args(["-c", "import cryptography"])
        .status()
        .unwrap()
        .success()
    {
        let checked = Command::new("python3")
            .args([
                concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/verify_ledger.py"),
                path.to_str().unwrap(),
                "--seq",
                &anchor.0.to_string(),
                "--hash",
                &anchor.1,
                "--verify-event-followups",
                "--verify-event-signatures",
                config_path.to_str().unwrap(),
                "--authority-sha256",
                &hex(&Sha256::digest(&config)),
            ])
            .output()
            .unwrap();
        assert!(
            checked.status.success(),
            "{}",
            String::from_utf8_lossy(&checked.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
        assert_eq!(report["signed_followup_claims_checked"], 1);
        assert_eq!(report["other_outcome_methods_not_checked"], 0);
    }

    for (format, argument) in [("sft", "-".to_string()), ("dpo", preference_id.to_string())] {
        let rejected = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args([
                "event-training-export",
                path.to_str().unwrap(),
                format,
                &attestation.to_string(),
                &followup_id.to_string(),
                config_path.to_str().unwrap(),
                &hex(&Sha256::digest(&config)),
                policy_path.to_str().unwrap(),
                &hex(&Sha256::digest(&policy_bytes)),
                &argument,
                "-",
                &anchor.0.to_string(),
                &anchor.1,
                "accept-synthetic-trust-v1",
            ])
            .output()
            .unwrap();
        assert!(!rejected.status.success());
        assert!(rejected.stdout.is_empty());
        let report: serde_json::Value = serde_json::from_slice(&rejected.stderr).unwrap();
        assert_eq!(report["status"], "not_exported");
    }
    drop(conn);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
