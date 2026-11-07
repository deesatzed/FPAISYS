use std::{
    io::Write,
    process::{Command, Stdio},
};

fn run(path: &std::path::Path, choice: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sese-training-demo"))
        .args([path.to_str().unwrap(), "accept-synthetic-trust-v1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(choice).unwrap();
    child.wait_with_output().unwrap()
}
#[test]
fn training_demo_requires_approval_and_exports_reproducible_synthetic_rows() {
    let base = std::env::temp_dir().join(format!(
        "sese-training-demo-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&base).unwrap();
    let rejected = base.join("reject");
    let result = run(&rejected, b"reject\n");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!rejected.join("sft.jsonl").exists());
    let existing = Command::new(env!("CARGO_BIN_EXE_sese-training-demo"))
        .args([rejected.to_str().unwrap(), "accept-synthetic-trust-v1"])
        .output()
        .unwrap();
    assert!(!existing.status.success());
    let missing_ack = base.join("no-ack");
    let result = Command::new(env!("CARGO_BIN_EXE_sese-training-demo"))
        .arg(&missing_ack)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!missing_ack.exists());
    let approved = base.join("approve");
    let result = run(&approved, b"approve\nrollback\n");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(approved.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["test_keys_public"], true);
    assert_eq!(manifest["independent_custody_verified"], false);
    let arg = |key: &str| manifest[key].as_str().unwrap().to_string();
    let seq = manifest["checkpoint"]["seq"].as_i64().unwrap().to_string();
    let anchor = manifest["checkpoint"]["hash"].as_str().unwrap();
    let attestation = manifest["attestation_id"].as_i64().unwrap().to_string();
    let followup = manifest["followup_id"].as_i64().unwrap().to_string();
    let preference = manifest["preference_id"].as_i64().unwrap().to_string();
    let verified = Command::new("python3")
        .args([
            concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/verify_ledger.py"),
            approved.join("demo.db").to_str().unwrap(),
            "--seq",
            &seq,
            "--hash",
            anchor,
            "--verify-event-followups",
            "--verify-event-attestations",
        ])
        .output()
        .unwrap();
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&verified.stdout).unwrap();
    assert_eq!(report["chain_valid"], true);
    assert_eq!(report["attestation_signatures_checked"], false);
    if Command::new("python3")
        .args(["-c", "import cryptography"])
        .status()
        .unwrap()
        .success()
    {
        let verified = Command::new("python3")
            .args([
                concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/verify_ledger.py"),
                approved.join("demo.db").to_str().unwrap(),
                "--seq",
                &seq,
                "--hash",
                anchor,
                "--verify-event-followups",
                "--verify-event-signatures",
                approved.join("authorities.json").to_str().unwrap(),
                "--authority-sha256",
                &arg("authority_config_sha256"),
            ])
            .output()
            .unwrap();
        assert!(
            verified.status.success(),
            "{}",
            String::from_utf8_lossy(&verified.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&verified.stdout).unwrap();
        assert_eq!(report["attestation_signatures_checked"], true);
        assert_eq!(report["attestation_signature_count"], 3);
        assert_eq!(report["authority_key_strength_strictly_checked"], false);
        assert_eq!(report["attestation_payload_record_links_checked"], true);
        assert_eq!(report["attestation_payload_record_links_count"], 3);
        assert_eq!(report["signed_followup_claims_checked"], 1);
        assert_eq!(report["other_outcome_methods_not_checked"], 0);
        let conn = rusqlite::Connection::open(approved.join("demo.db")).unwrap();
        let early_hash: String = conn
            .query_row("SELECT hash FROM ledger_events WHERE seq=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        drop(conn);
        let stale = Command::new("python3")
            .args([
                concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/verify_ledger.py"),
                approved.join("demo.db").to_str().unwrap(),
                "--seq",
                "1",
                "--hash",
                &early_hash,
                "--verify-event-signatures",
                approved.join("authorities.json").to_str().unwrap(),
                "--authority-sha256",
                &arg("authority_config_sha256"),
            ])
            .output()
            .unwrap();
        assert!(
            !stale.status.success(),
            "checkpoint must cover the verified receipts"
        );
        assert!(stale.stdout.is_empty());
        let invalid = Command::new("python3")
            .args([
                concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/verify_ledger.py"),
                approved.join("demo.db").to_str().unwrap(),
                "--seq",
                &seq,
                "--hash",
                anchor,
                "--verify-event-signatures",
                approved.join("authorities.json").to_str().unwrap(),
                "--authority-sha256",
                &"0".repeat(64),
            ])
            .output()
            .unwrap();
        assert!(!invalid.status.success());
        assert!(invalid.stdout.is_empty());
    }
    for format in ["sft", "dpo", "rlvr"] {
        let first = if format == "dpo" {
            preference.clone()
        } else if format == "rlvr" {
            approved
                .join("reward-rule.json")
                .to_string_lossy()
                .into_owned()
        } else {
            "-".into()
        };
        let second = if format == "rlvr" {
            arg("reward_rule_sha256")
        } else {
            "-".into()
        };
        let output = Command::new(env!("CARGO_BIN_EXE_sese-outcome"))
            .args([
                "event-training-export",
                approved.join("demo.db").to_str().unwrap(),
                format,
                &attestation,
                &followup,
                approved.join("authorities.json").to_str().unwrap(),
                &arg("authority_config_sha256"),
                approved.join("policy.json").to_str().unwrap(),
                &arg("trust_policy_sha256"),
                &first,
                &second,
                &seq,
                anchor,
                "accept-synthetic-trust-v1",
            ])
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "demo policy must not authorize training export"
        );
        assert!(output.stdout.is_empty());
        let report: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(report["status"], "not_exported");
        assert!(report["reason"]
            .as_str()
            .unwrap()
            .contains("policy does not permit training export"));
        let saved: serde_json::Value = serde_json::from_slice(
            &std::fs::read(approved.join(format!("{format}.jsonl"))).unwrap(),
        )
        .unwrap();
        assert_eq!(saved["demonstration_only"], true);
        assert_eq!(
            saved["synthetic_training_eligible_under_operator_assumptions"],
            false
        );
        assert_eq!(saved["synthetic"], true);
        assert_eq!(saved["training_eligibility_independently_verified"], false);
        assert_eq!(saved["real_world_outcome_verified"], false);
    }
    std::fs::remove_dir_all(base).unwrap();
}
