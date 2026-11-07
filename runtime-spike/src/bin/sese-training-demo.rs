//! Local demonstration only: fixed public test keys and caller-asserted synthetic evidence.
use ed25519_dalek::{Signer, SigningKey};
use sese_runtime_spike::attestation::{
    attestation_message, AuthorityRole, AuthoritySet, EvidenceReference, NamedSignedClaim,
    SyntheticOutcome, SyntheticOutcomeEvidence, SyntheticPreferenceEvidence,
    SyntheticTraceEvidence, SyntheticTrustPolicy,
};
use sese_runtime_spike::reward_rule::SyntheticRewardRule;
use sese_runtime_spike::storage::{Storage, SyntheticTraceInput};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(bytes)?;
    Ok(())
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[1] != "accept-synthetic-trust-v1" {
        return Err("usage: sese-training-demo NEW_DIRECTORY accept-synthetic-trust-v1".into());
    }
    let dir = Path::new(&args[0]);
    std::fs::create_dir(dir)?;
    let path = dir.join("demo.db");
    let fixture_path = dir.join("followup-fixture.json");
    write_new(
        &fixture_path,
        include_bytes!("../../fixtures/event-followup-synthetic-v1.json"),
    )?;
    let trace_fixture = include_bytes!("../../fixtures/training-trace-synthetic-v1.json");
    write_new(&dir.join("trace-fixture.json"), trace_fixture)?;
    let bin_dir = std::env::current_exe()?
        .parent()
        .ok_or("missing executable directory")?
        .to_path_buf();
    eprintln!("Synthetic demonstration: fixed public test keys, one operator, no independent custody or real-world outcome. Never use these keys for real evidence. The explicitly supplied synthetic trace and pair are in {}/trace-fixture.json.", dir.display());
    let status = std::process::Command::new(bin_dir.join("sese-event-cycle-demo"))
        .arg(bin_dir.join("event-worker"))
        .arg(&path)
        .arg("--followup-fixture")
        .arg(&fixture_path)
        .args(["--followup-case", "post-promotion"])
        .status()?;
    if !status.success() {
        return Err("synthetic event cycle failed".into());
    }
    let conn =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let action: String =
        conn.query_row("SELECT action FROM event_reviews WHERE id=1", [], |r| {
            r.get(0)
        })?;
    if action == "reject" {
        println!("Review rejected; no training examples exported.");
        return Ok(());
    }
    let mut db = Storage::open(&path)?;
    let input: SyntheticTraceInput = serde_json::from_slice(trace_fixture)?;
    let trace_id = db.record_synthetic_event_trace(1, &input)?;
    let followup_id = db.record_selected_synthetic_followup(1, 2)?;
    let followup = db.inspect_synthetic_followup_receipt(followup_id)?;
    if followup["comparison"]["status"] != "match" {
        return Err(
            "synthetic follow-up did not match; no outcome or training rows recorded".into(),
        );
    }
    db.resolve_event_review(
        1,
        "validated",
        "demo operator",
        "fixed synthetic fixture comparison",
    )?;
    let checkpoint = db.ledger_checkpoint()?;
    let (candidate,replay,fixture,review_hash,trace_time,trace_hash,follow_time,follow_hash): (String,i64,String,String,String,String,String,String) = conn.query_row("SELECT r.candidate_id,r.replay_id,p.pack_digest,er.hash,t.created_unix_ns,et.hash,f.created_unix_ns,ef.hash FROM event_training_traces t JOIN event_reviews r ON r.id=t.review_id JOIN event_replay_runs p ON p.id=r.replay_id JOIN ledger_events er ON er.kind='event_review' AND er.reference=r.id JOIN ledger_events et ON et.kind='event_training_trace' AND et.reference=t.id JOIN event_followup_receipts f ON f.review_id=r.id JOIN ledger_events ef ON ef.kind='event_followup_receipt' AND ef.reference=f.id WHERE t.id=?1",[trace_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?)))?;
    let reference = EvidenceReference {
        review_id: 1,
        replay_id: replay,
        candidate_sha256: candidate,
        fixture_sha256: fixture,
        review_ledger_hash: review_hash,
    };
    let capture: u64 = trace_time.parse()?;
    let trace = SyntheticTraceEvidence {
        reference: reference.clone(),
        prompt: input.prompt.clone(),
        response: input.response.clone(),
        source: input.source.clone(),
        method: "fixed synthetic demonstration".into(),
        captured_unix_ns: capture,
    };
    let outcome = SyntheticOutcomeEvidence {
        reference: reference.clone(),
        outcome: SyntheticOutcome::Validated,
        source: "fixed synthetic fixture comparison".into(),
        method: "synthetic-followup-receipt-v1".into(),
        evidence_reference: format!("sese-followup:{followup_id}:{follow_hash}"),
        observed_unix_ns: follow_time.parse()?,
    };
    let pair = input.preference.as_ref().ok_or("missing demo pair")?;
    let preference = SyntheticPreferenceEvidence {
        reference,
        trace_id,
        trace_ledger_hash: trace_hash,
        prompt: input.prompt.clone(),
        preferred: pair.preferred.clone(),
        rejected: pair.rejected.clone(),
        source: pair.source.clone(),
        comparability_basis: "fixed synthetic pair, same prompt".into(),
        reviewed_unix_ns: capture,
    };
    // These public, fixed seeds intentionally provide no custody or independence.
    let trace_key = SigningKey::from_bytes(&[61; 32]);
    let outcome_key = SigningKey::from_bytes(&[62; 32]);
    let preference_key = SigningKey::from_bytes(&[63; 32]);
    let config = serde_json::to_vec(
        &serde_json::json!({"schema":"sese-authority-set-v1","authorities":[{"id":"demo-source","role":"trace-source","public_key":hex(&trace_key.verifying_key().to_bytes()),"revoked":false},{"id":"demo-observer","role":"outcome-observer","public_key":hex(&outcome_key.verifying_key().to_bytes()),"revoked":false},{"id":"demo-reviewer","role":"preference-reviewer","public_key":hex(&preference_key.verifying_key().to_bytes()),"revoked":false}]}),
    )?;
    let set = AuthoritySet::from_pinned_json(&config, &Sha256::digest(&config).into())?;
    let sign = |key: &SigningKey,
                id: &str,
                role: AuthorityRole,
                payload: &[u8]|
     -> Result<[u8; 64], String> {
        Ok(key
            .sign(&attestation_message(id, role, payload)?)
            .to_bytes())
    };
    let trace_sig = sign(
        &trace_key,
        "demo-source",
        AuthorityRole::TraceSource,
        &trace.canonical_bytes()?,
    )?;
    let outcome_sig = sign(
        &outcome_key,
        "demo-observer",
        AuthorityRole::OutcomeObserver,
        &outcome.canonical_bytes()?,
    )?;
    let pref_sig = sign(
        &preference_key,
        "demo-reviewer",
        AuthorityRole::PreferenceReviewer,
        &preference.canonical_bytes()?,
    )?;
    let attestation_id = db.record_signed_synthetic_followup(
        trace_id,
        followup_id,
        &set,
        NamedSignedClaim {
            evidence: &trace,
            authority_id: "demo-source",
            signature: &trace_sig,
        },
        NamedSignedClaim {
            evidence: &outcome,
            authority_id: "demo-observer",
            signature: &outcome_sig,
        },
        (checkpoint.0, &checkpoint.1),
    )?;
    let preference_id = db.record_synthetic_preference_receipt(
        &set,
        NamedSignedClaim {
            evidence: &preference,
            authority_id: "demo-reviewer",
            signature: &pref_sig,
        },
        (checkpoint.0, &checkpoint.1),
    )?;
    let anchor = db.ledger_checkpoint()?;
    let policy_bytes = serde_json::to_vec(
        &serde_json::json!({"schema":"sese-synthetic-trust-policy-v1","training_export_allowed":false,"authority_config_sha256":hex(&set.digest()),"declarations":[{"trace_source":"demo-source","outcome_observer":"demo-observer","operator":"demo operator","observer_relationship_basis":"same demonstration process; no independence","preference_reviewer":"demo-reviewer","reviewer_relationship_basis":"same demonstration process; no independence"}]}),
    )?;
    let policy = SyntheticTrustPolicy::from_pinned_json(
        &policy_bytes,
        &Sha256::digest(&policy_bytes).into(),
        &set,
    )?;
    let rule_bytes = include_bytes!("../../fixtures/synthetic-reward-rule-v1.json");
    let rule =
        SyntheticRewardRule::from_pinned_json(rule_bytes, &Sha256::digest(rule_bytes).into())?;
    let sft = db.preview_synthetic_training(
        "sft",
        attestation_id,
        followup_id,
        &set,
        &policy,
        None,
        true,
        (anchor.0, &anchor.1),
    )?;
    let dpo = db.preview_synthetic_preference(
        attestation_id,
        followup_id,
        preference_id,
        &set,
        &policy,
        true,
        (anchor.0, &anchor.1),
    )?;
    let rlvr = db.preview_synthetic_training(
        "rlvr",
        attestation_id,
        followup_id,
        &set,
        &policy,
        Some(&rule),
        true,
        (anchor.0, &anchor.1),
    )?;
    write_new(&dir.join("authorities.json"), &config)?;
    write_new(&dir.join("policy.json"), &policy_bytes)?;
    write_new(&dir.join("reward-rule.json"), rule_bytes)?;
    let manifest = serde_json::json!({"schema":"sese-synthetic-training-demo-v1","synthetic":true,"test_keys_public":true,"demonstration_only":true,"independent_custody_verified":false,"real_world_outcome_verified":false,"review_id":1,"trace_id":trace_id,"followup_id":followup_id,"attestation_id":attestation_id,"preference_id":preference_id,"checkpoint":{"seq":anchor.0,"hash":anchor.1},"authority_config_sha256":hex(&set.digest()),"trust_policy_sha256":hex(&policy.digest()),"reward_rule_sha256":hex(&rule.digest())});
    write_new(
        &dir.join("manifest.json"),
        format!("{manifest}\n").as_bytes(),
    )?;
    for (name, mut row) in [("sft.jsonl", sft), ("dpo.jsonl", dpo), ("rlvr.jsonl", rlvr)] {
        row["demonstration_only"] = serde_json::json!(true);
        row["eligibility"] = serde_json::json!("demonstration_only_ineligible");
        row["synthetic_training_eligible_under_operator_assumptions"] = serde_json::json!(false);
        row["demonstration_exclusion_reason"] =
            serde_json::json!("public test keys and one demonstration operator");
        write_new(&dir.join(name), format!("{row}\n").as_bytes())?;
    }
    println!(
        "Demonstration-only examples and manifest saved to {}",
        dir.display()
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("synthetic training demo failed: {e}");
        std::process::exit(1);
    }
}
