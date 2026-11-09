//! Operator interface for synthetic outcomes; no claim of independent validation.
use sese_runtime_spike::storage::Storage;
use std::io::{self, Read, Write};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonClaim<T> {
    authority_id: String,
    signature: String,
    evidence: T,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SyntheticAttestationBundle {
    schema: String,
    trace_id: i64,
    trace: JsonClaim<sese_runtime_spike::attestation::SyntheticTraceEvidence>,
    outcome: JsonClaim<sese_runtime_spike::attestation::SyntheticOutcomeEvidence>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SyntheticPreferenceBundle {
    schema: String,
    claim: JsonClaim<sese_runtime_spike::attestation::SyntheticPreferenceEvidence>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedCard {
    human: String,
    bridge: String,
    proposal: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InputReviewCard {
    schema: String,
    human: String,
    bridge: String,
    proposal: String,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedEvaluation {
    schema: String,
    synthetic: bool,
    review_recorded: bool,
    domain: String,
    candidate_id: String,
    champion_id: String,
    replay_id: i64,
    observation_id: i64,
    state_semantics: String,
    checkpoint: serde_json::Value,
    preview: serde_json::Value,
    card: PreparedCard,
}

fn verify_prepared(
    prepared: &PreparedEvaluation,
    preview: &serde_json::Value,
    db: &mut Storage,
) -> Result<(), Box<dyn std::error::Error>> {
    if prepared.schema != "sese-prepared-synthetic-evaluation-v1"
        || !prepared.synthetic
        || prepared.review_recorded
        || preview != &prepared.preview
        || preview["checkpoint"] != prepared.checkpoint
        || preview["replay"]["candidate_id"] != prepared.candidate_id
        || preview["replay"]["state_semantics"] != prepared.state_semantics
        || preview["observations"]
            .as_array()
            .and_then(|v| v.last())
            .and_then(|v| v["id"].as_i64())
            != Some(prepared.observation_id)
        || db.current_event_champion(&prepared.domain)? != prepared.champion_id
    {
        return Err("prepared evaluation no longer matches current evidence".into());
    }
    Ok(())
}

fn read_bounded_file(path: &str, limit: u64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err("input must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() as u64 > limit {
        return Err("invalid input size".into());
    }
    Ok(bytes)
}

fn decode_lower_hex<const N: usize>(text: &str) -> Result<[u8; N], &'static str> {
    let input = text.as_bytes();
    if input.len() != N * 2
        || !input
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    {
        return Err("invalid lowercase hexadecimal value");
    }
    let mut out = [0; N];
    for (i, item) in out.iter_mut().enumerate() {
        let nibble = |b: u8| if b <= b'9' { b - b'0' } else { b - b'a' + 10 };
        *item = (nibble(input[i * 2]) << 4) | nibble(input[i * 2 + 1]);
    }
    Ok(out)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        return Err("usage: sese-outcome COMMAND DB [ARGS]; commands: status, resolve, export, event-init, event-builtin-register, event-builtin-replay, event-builtin-observe, event-propose-threshold, event-propose-from-fixture, event-proposal-inspect, event-register, event-review, event-review-preview, event-review-prepare, event-review-prepared, event-review-at, event-container-evaluate, event-container-replay, event-container-replay-inspect, event-container-review-preview, event-container-observation, event-container-observation-inspect, event-container-shadow, event-container-shadow-inspect, event-container-assert, event-container-capture, event-container-assert-inspect, event-inspect, event-replay-inspect, event-observations, event-history, event-training-export, event-evidence-check, event-evidence-policy-check, event-common-policy-check, event-reward-preview, event-evidence-export, event-evidence-verify, event-attestation-check, event-attestation-record, event-attestation-inspect, event-attestation-recheck, event-preference-check, event-preference-record, event-preference-inspect, event-preference-recheck, event-followup-check, event-followup-pin, event-followup-selected-check, event-followup-record, event-followup-inspect, event-followup-outcome-check, event-signed-followup-check, event-signed-followup-record, event-signed-followup-recheck, event-retained-followup-check, event-trace-record, event-trace-inspect, event-trace-compare-wat, event-trace-eligibility, event-pin, event-pin-match, event-bootstrap, event-champion, event-promote, event-rollback, event-status, event-resolve, event-export, window-inspect, window-status, window-resolve, window-export, window-pin, window-pin-match, window-register, window-bootstrap, window-champion, window-promote, window-rollback, checkpoint, verify-checkpoint; see README".into());
    }
    if args[0] == "event-init" && args.len() == 2 {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[1])?;
        let db = Storage::open(std::path::Path::new(&args[1]))?;
        drop(db);
        println!(
            "{}",
            serde_json::json!({"schema":"sese-synthetic-init-v1","synthetic":true})
        );
        return Ok(());
    }
    let mut db = if matches!(
        args[0].as_str(),
        "event-training-export"
            | "event-proposal-inspect"
            | "event-container-review-preview"
            | "event-review-prepare"
            | "event-container-observation-inspect"
    ) {
        Storage::open_read_only(std::path::Path::new(&args[1]))?
    } else {
        let conn = rusqlite::Connection::open_with_flags(
            &args[1],
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?;
        Storage::from_connection(conn)?
    };
    match args[0].as_str() {
        #[cfg(unix)]
        "event-container-evaluate" if args.len() == 13 || args.len() == 14 => {
            let replay_id: i64 = args[3].parse()?;
            let observation_id: i64 = args[4].parse()?;
            let timeout = std::time::Duration::from_secs(args[12].parse()?);
            if timeout.is_zero() || timeout > std::time::Duration::from_secs(120) {
                return Err("invalid container deadline".into());
            }
            let pack =
                sese_runtime_spike::fixture_file::parse(&read_bounded_file(&args[5], 1_048_576)?)?;
            let preview = db.event_review_preview(replay_id, &args[2])?;
            let mut prepared: Option<PreparedEvaluation> = if args.len() == 14 {
                let value = serde_json::from_slice(&read_bounded_file(&args[13], 8_388_608)?)?;
                Some(value)
            } else {
                None
            };
            if let Some(saved) = prepared.as_ref() {
                if saved.domain != args[2]
                    || saved.replay_id != replay_id
                    || saved.observation_id != observation_id
                {
                    return Err("prepared evaluation does not match requested review".into());
                }
                verify_prepared(saved, &preview, &mut db)?;
            }
            let original_seq = preview["checkpoint"]["seq"]
                .as_i64()
                .ok_or("invalid preview checkpoint")?;
            let local = &preview["replay"];
            if local["passed"] != true
                || local["state_semantics"] != "measured-v2"
                || local["fixture_version"].as_str() != Some(pack.version.as_str())
                || local["fixture_digest"].as_str() != Some(pack.digest.as_str())
                || pack.state_semantics != "measured-v2"
            {
                return Err("fixture does not match passed measured review replay".into());
            }
            let candidate = local["candidate_id"]
                .as_str()
                .ok_or("invalid replay candidate")?;
            let observations = preview["observations"]
                .as_array()
                .ok_or("invalid review observations")?;
            let observation = observations
                .iter()
                .find(|o| o["id"].as_i64() == Some(observation_id))
                .ok_or("observation not found in review")?;
            if observation["shadow_id"].as_str() != Some(candidate)
                || !observation["champion_error"].is_null()
                || !observation["shadow_error"].is_null()
                || observation["champion_id"].as_str()
                    != Some(db.current_event_champion(&args[2])?.as_str())
            {
                return Err("observation does not match current review".into());
            }
            let executor = sese_runtime_spike::event_executor::SyntheticContainerExecutor::new(
                &args[9], &args[11], &args[10], &args[6], &args[7], &args[8],
            )?;
            let container_replay = sese_runtime_spike::event_replay::replay_container_synthetic(
                &mut db, &executor, candidate, &pack, timeout,
            )?;
            if !container_replay.passed {
                return Err(format!(
                    "container replay failed; container_replay_seq={}",
                    container_replay.seq
                )
                .into());
            }
            let comparison = sese_runtime_spike::event_host::compare_container_observation(
                &mut db,
                &executor,
                &args[2],
                observation_id,
                timeout,
            )
            .map_err(|error| format!("{error}; container_replay_seq={}", container_replay.seq))?;
            let mut combined = db
                .event_container_review_preview(
                    &args[2],
                    replay_id,
                    observation_id,
                    container_replay.seq,
                    comparison.seq,
                )
                .map_err(|error| {
                    format!(
                        "{error}; container_replay_seq={}; container_observation_seq={}",
                        container_replay.seq, comparison.seq
                    )
                })?;
            if let Some(saved) = prepared.as_mut() {
                let mut original_review = preview.clone();
                let mut updated_review = combined["review"].clone();
                original_review["checkpoint"] = serde_json::Value::Null;
                updated_review["checkpoint"] = serde_json::Value::Null;
                if original_seq.checked_add(1) != Some(container_replay.seq)
                    || original_seq.checked_add(2) != Some(comparison.seq)
                    || combined["checkpoint"]["seq"].as_i64() != Some(comparison.seq)
                    || original_review != updated_review
                {
                    return Err(format!("review changed during container evaluation; container_replay_seq={}; container_observation_seq={}", container_replay.seq, comparison.seq).into());
                }
                saved.preview = combined["review"].clone();
                saved.checkpoint = combined["checkpoint"].clone();
                combined["prepared"] = serde_json::to_value(saved)?;
            }
            println!("{combined}");
        }
        #[cfg(unix)]
        "event-container-observation" if args.len() == 11 => {
            let executor = sese_runtime_spike::event_executor::SyntheticContainerExecutor::new(
                &args[7], &args[9], &args[8], &args[4], &args[5], &args[6],
            )?;
            let report = sese_runtime_spike::event_host::compare_container_observation(
                &mut db,
                &executor,
                &args[2],
                args[3].parse()?,
                std::time::Duration::from_secs(args[10].parse()?),
            )?;
            println!(
                "{}",
                serde_json::json!({"schema":"sese-container-observation-result-v1","container_event_seq":report.seq,"observation_id":args[3].parse::<i64>()?,"synthetic":true,"execution_authenticated":false,"promotion_evidence":false})
            );
        }
        #[cfg(unix)]
        "event-container-observation-inspect" if args.len() == 3 => {
            println!(
                "{}",
                db.inspect_container_observation_experiment(args[2].parse()?)?
            );
        }
        #[cfg(unix)]
        "event-container-review-preview" if args.len() == 7 => {
            println!(
                "{}",
                db.event_container_review_preview(
                    &args[2],
                    args[3].parse()?,
                    args[4].parse()?,
                    args[5].parse()?,
                    args[6].parse()?
                )?
            );
        }
        #[cfg(unix)]
        "event-container-shadow" if args.len() == 18 => {
            use sha2::{Digest, Sha256};
            let champion = format!(
                "{:x}",
                Sha256::digest(sese_runtime_spike::builtin_event::artifact(&args[5])?)
            );
            let shadow = format!(
                "{:x}",
                Sha256::digest(sese_runtime_spike::builtin_event::artifact(&args[6])?)
            );
            let event = sese_runtime_spike::event_abi::Event {
                timestamp_ns: args[7].parse()?,
                entity_id: args[8].parse()?,
                metric: args[9].parse()?,
                flags: args[10].parse()?,
            };
            let executor = sese_runtime_spike::event_executor::SyntheticContainerExecutor::new(
                &args[14], &args[16], &args[15], &args[11], &args[12], &args[13],
            )?;
            let mut host = sese_runtime_spike::event_host::ContainerShadowHost::open(
                &mut db,
                &args[2],
                &args[3],
                args[4].parse()?,
            )?;
            let result = host.process(
                &mut db,
                &executor,
                &champion,
                &shadow,
                &event,
                std::time::Duration::from_secs(args[17].parse()?),
            )?;
            println!(
                "{}",
                serde_json::json!({"schema":"sese-container-shadow-experiment-result-v1","container_event_seq":result.seq,"prior":{"count":result.prior.count,"mean":result.prior.mean,"p99_us":result.prior.p99_us,"last_error":result.prior.last_error},"champion":format!("{:?}",result.champion.decision),"shadow":format!("{:?}",result.shadow.decision),"synthetic":true,"execution_authenticated":false,"promotion_evidence":false})
            );
        }
        #[cfg(unix)]
        "event-container-shadow-inspect" if args.len() == 3 => {
            println!(
                "{}",
                db.inspect_container_shadow_experiment(args[2].parse()?)?
            );
        }
        #[cfg(unix)]
        "event-container-replay" if args.len() == 11 => {
            use sha2::{Digest, Sha256};
            let artifact = sese_runtime_spike::builtin_event::artifact(&args[2])?;
            let candidate = format!("{:x}", Sha256::digest(&artifact));
            if db.artifact(&candidate)? != artifact {
                return Err("built-in artifact mismatch".into());
            }
            let pack =
                sese_runtime_spike::fixture_file::parse(&read_bounded_file(&args[3], 1_048_576)?)?;
            let executor = sese_runtime_spike::event_executor::SyntheticContainerExecutor::new(
                &args[7], &args[9], &args[8], &args[4], &args[5], &args[6],
            )?;
            let timeout = std::time::Duration::from_secs(args[10].parse()?);
            let report = sese_runtime_spike::event_replay::replay_container_synthetic(
                &mut db, &executor, &candidate, &pack, timeout,
            )?;
            println!(
                "{}",
                serde_json::json!({"schema":"sese-container-replay-experiment-result-v1","container_event_seq":report.seq,"candidate_id":candidate,"passed":report.passed,"synthetic":true,"promotion_evidence":false})
            );
            if !report.passed {
                return Err("synthetic container replay failed".into());
            }
        }
        #[cfg(unix)]
        "event-container-replay-inspect" if args.len() == 3 => {
            println!(
                "{}",
                db.inspect_container_replay_experiment(args[2].parse()?)?
            );
        }
        "event-container-capture" if args.len() == 8 => {
            let bundle = read_bounded_file(&args[5], 4_194_304)?;
            let seq = db.record_container_comparison_capture(
                args[2].parse()?,
                args[3].parse()?,
                &args[4],
                &bundle,
                (args[6].parse()?, &args[7]),
            )?;
            println!(
                "{}",
                serde_json::json!({"seq":seq,"evidence_origin":"caller_asserted","promotion_evidence":false})
            );
        }
        "event-container-assert" if args.len() == 9 => {
            let seq = db.record_container_comparison_assertion(
                args[2].parse()?,
                args[3].parse()?,
                &args[4],
                &args[5],
                &args[6],
                (args[7].parse()?, &args[8]),
            )?;
            println!(
                "{}",
                serde_json::json!({"seq":seq,"evidence_origin":"caller_asserted","promotion_evidence":false})
            );
        }
        "event-container-assert-inspect" if args.len() == 3 => {
            println!(
                "{}",
                db.inspect_container_comparison_assertion(args[2].parse()?)?
            );
        }
        "event-training-export" if args.len() == 14 => {
            if args[13] != "accept-synthetic-trust-v1" {
                return Err("explicit synthetic trust acceptance required".into());
            }
            let authority_pin = decode_lower_hex::<32>(&args[6])?;
            let authorities = sese_runtime_spike::attestation::AuthoritySet::from_pinned_json(
                &read_bounded_file(&args[5], 65_536)?,
                &authority_pin,
            )?;
            let policy_pin = decode_lower_hex::<32>(&args[8])?;
            let policy = sese_runtime_spike::attestation::SyntheticTrustPolicy::from_pinned_json(
                &read_bounded_file(&args[7], 65_536)?,
                &policy_pin,
                &authorities,
            )?;
            let checkpoint = (args[11].parse()?, args[12].as_str());
            let row = match args[2].as_str() {
                "sft" if args[9] == "-" && args[10] == "-" => db
                    .export_conditionally_trusted_synthetic_training(
                        "sft",
                        args[3].parse()?,
                        args[4].parse()?,
                        &authorities,
                        &policy,
                        None,
                        true,
                        checkpoint,
                    )?,
                "rlvr" => {
                    let pin = decode_lower_hex::<32>(&args[10])?;
                    let rule =
                        sese_runtime_spike::reward_rule::SyntheticRewardRule::from_pinned_json(
                            &read_bounded_file(&args[9], 65_536)?,
                            &pin,
                        )?;
                    db.export_conditionally_trusted_synthetic_training(
                        "rlvr",
                        args[3].parse()?,
                        args[4].parse()?,
                        &authorities,
                        &policy,
                        Some(&rule),
                        true,
                        checkpoint,
                    )?
                }
                "dpo" if args[10] == "-" => db.export_conditionally_trusted_synthetic_preference(
                    args[3].parse()?,
                    args[4].parse()?,
                    args[9].parse()?,
                    &authorities,
                    &policy,
                    true,
                    checkpoint,
                )?,
                _ => return Err("unsupported format or reward arguments".into()),
            };
            println!("{row}");
            eprintln!(
                "{}",
                serde_json::json!({"schema":"sese-synthetic-training-export-report-v1","status":"included","format":args[2],"count":1,"trust_basis":"operator_declared","synthetic":true})
            );
        }
        "event-builtin-register" if args.len() == 3 => {
            let artifact = sese_runtime_spike::builtin_event::artifact(&args[2])?;
            let candidate = db.register_synthetic_event(&artifact)?;
            println!(
                "{}",
                serde_json::json!({"candidate_id":candidate,"name":args[2],"synthetic":true,"executed":false})
            );
        }
        "event-builtin-replay" if args.len() == 4 => {
            use sha2::{Digest, Sha256};
            let artifact = sese_runtime_spike::builtin_event::artifact(&args[2])?;
            let candidate = format!("{:x}", Sha256::digest(&artifact));
            if db.artifact(&candidate)? != artifact {
                return Err("built-in artifact mismatch".into());
            }
            let pack =
                sese_runtime_spike::fixture_file::parse(&read_bounded_file(&args[3], 1_048_576)?)?;
            let worker = std::env::current_exe()?
                .parent()
                .ok_or("missing binary directory")?
                .join("event-worker");
            let worker = worker.to_str().ok_or("invalid worker path")?;
            let report = pack.replay_synthetic(
                &mut db,
                worker,
                &candidate,
                std::time::Duration::from_secs(2),
            )?;
            println!(
                "{}",
                serde_json::json!({"schema":"sese-synthetic-builtin-replay-v1","candidate_id":candidate,"name":args[2],"run_id":report.run_id,"fixture_version":pack.version,"fixture_sha256":pack.digest,"passed":report.passed,"synthetic":true})
            );
            if !report.passed {
                return Err("synthetic replay failed".into());
            }
        }
        "event-builtin-observe" if args.len() == 11 => {
            use sese_runtime_spike::event_abi::{Decision, Event, Verdict};
            use sha2::{Digest, Sha256};
            let known = |name: &str| -> Result<(String, Vec<u8>), String> {
                let artifact = sese_runtime_spike::builtin_event::artifact(name)?;
                Ok((format!("{:x}", Sha256::digest(&artifact)), artifact))
            };
            let (champion, champion_bytes) = known(&args[4])?;
            let (shadow, shadow_bytes) = known(&args[5])?;
            let event = Event {
                timestamp_ns: args[7].parse()?,
                entity_id: args[8].parse()?,
                metric: args[9].parse()?,
                flags: args[10].parse()?,
            };
            let minimum = match args[6].as_str() {
                "legacy-v1" => 0.0,
                "measured-v2" => -1_000_000_000_000.0,
                _ => return Err("unknown state semantics".into()),
            };
            if !event.metric.is_finite() || !(minimum..=1_000_000_000_000.0).contains(&event.metric)
            {
                return Err("invalid synthetic metric".into());
            }
            if db.artifact(&champion)? != champion_bytes
                || db.artifact(&shadow)? != shadow_bytes
                || db.current_event_champion(&args[2])? != champion
            {
                return Err("built-in candidate or expected champion mismatch".into());
            }
            if db.event_domain_semantics(&args[2])? != args[6] {
                return Err("state semantics do not match domain".into());
            }
            let worker = std::env::current_exe()?
                .parent()
                .ok_or("missing binary directory")?
                .join("event-worker");
            let worker = worker.to_str().ok_or("invalid worker path")?;
            let timeout = std::time::Duration::from_secs(2);
            let observed = if args[6] == "legacy-v1" {
                let mut host =
                    sese_runtime_spike::event_host::EventHost::open(&mut db, &args[3], 32)?;
                host.process_domain_expected(
                    &mut db, worker, &args[2], &champion, &shadow, &event, 0, timeout,
                )?
            } else {
                let mut host =
                    sese_runtime_spike::event_host::MeasuredEventHost::open(&mut db, &args[3], 32)?;
                host.process_domain_expected(
                    &mut db, worker, &args[2], &champion, &shadow, &event, timeout,
                )?
            };
            let verdict = |v: Verdict| {
                let decision = match v.decision {
                    Decision::Allow => "allow",
                    Decision::Throttle => "throttle",
                    Decision::Quarantine => "quarantine",
                    Decision::Escalate => "escalate",
                };
                serde_json::json!({"decision":decision,"score":v.score,"reason":v.reason})
            };
            println!(
                "{}",
                serde_json::json!({"schema":"sese-synthetic-builtin-observation-v1","observation_id":observed.id,"domain":args[2],"stream":args[3],"state_semantics":args[6],"prior":{"count":observed.prior.count,"mean":observed.prior.mean,"p99_us":observed.prior.p99_us,"last_error":observed.prior.last_error},"champion":observed.champion.map(verdict),"champion_error":observed.champion_error,"shadow":observed.shadow.map(verdict),"shadow_error":observed.shadow_error,"synthetic":true})
            );
            if observed.champion.is_none() || observed.shadow.is_none() {
                return Err("synthetic observation failed".into());
            }
        }
        "event-propose-from-fixture" if args.len() == 3 => {
            println!(
                "{}",
                db.record_fixture_proposal(&read_bounded_file(&args[2], 1_048_576)?)?
            );
        }
        "event-proposal-inspect" if args.len() == 3 => {
            println!("{}", db.inspect_fixture_proposal(args[2].parse()?)?)
        }
        "event-propose-threshold" if args.len() == 3 => {
            let threshold: f64 = args[2].parse()?;
            let artifact = sese_runtime_spike::proposal::threshold_artifact(threshold)?;
            let candidate = db.register_event(&artifact)?;
            println!(
                "{}",
                serde_json::json!({"schema":"sese-threshold-proposal-v1", "candidate_id":candidate,
                "threshold":threshold, "rule":"throttle when metric > threshold; otherwise allow",
                "generator":"local-threshold-v1", "executed":false, "validated":false, "classification":"not asserted by this command"})
            );
        }
        "window-register" | "event-register" if args.len() == 3 => {
            let bytes = read_bounded_file(&args[2], 65_536)?;
            let structural_check = if args[0] == "event-register" {
                Some(sese_runtime_spike::event_abi::structural_check(&bytes))
            } else {
                None
            };
            let (id, abi) = if args[0] == "event-register" {
                (db.register_event(&bytes)?, "event-v1")
            } else {
                (db.register_window(&bytes)?, "window-v1")
            };
            let mut output =
                serde_json::json!({"candidate_id":id,"abi":abi,"executed":false,"validated":false});
            if let Some(check) = structural_check {
                output["structural_check"] = check;
            }
            println!("{output}");
        }
        "window-bootstrap" if args.len() == 5 => {
            db.bootstrap_champion(&args[2], &args[3], &args[4])?;
            println!("Synthetic champion initialized");
        }
        "window-champion" if args.len() == 3 => println!("{}", db.current_champion(&args[2])?),
        "window-promote" if args.len() == 6 => {
            db.promote_window(&args[2], &args[3], args[4].parse()?, &args[5])?;
            println!("Synthetic champion metadata promoted");
        }
        "window-rollback" if args.len() == 5 => {
            db.rollback_window(&args[2], &args[3], &args[4])?;
            println!("Synthetic champion metadata rolled back");
        }
        "window-pin" if args.len() == 6 => {
            db.pin_window_fixture(&args[2], &args[3], &args[4], &args[5])?;
            println!("Synthetic fixture selection recorded");
        }
        "window-pin-match" if args.len() == 4 => {
            if !db.window_replay_matches_pin(&args[2], args[3].parse()?)? {
                return Err("fixture mismatch".into());
            }
            println!("match");
        }
        "checkpoint" if args.len() == 2 => {
            let (seq, hash) = db.ledger_checkpoint()?;
            println!(
                "{}",
                serde_json::json!({"schema":"sese-ledger-checkpoint-v1","seq":seq,"hash":hash})
            );
        }
        "verify-checkpoint" if args.len() == 4 => {
            if !db.verify_ledger_checkpoint(args[2].parse()?, &args[3])? {
                return Err("checkpoint mismatch".into());
            }
            println!("Checkpoint verified");
        }
        "status" if args.len() == 3 => println!("{}", db.outcome_status(args[2].parse()?)?),
        "resolve" if args.len() == 6 => {
            db.resolve_review(args[2].parse()?, &args[3], &args[4], &args[5])?;
            println!("Synthetic outcome recorded");
        }
        "event-bootstrap" if args.len() == 5 => {
            db.bootstrap_event_champion(&args[2], &args[3], &args[4])?;
            println!("Synthetic event champion initialized");
        }
        "event-champion" if args.len() == 3 => println!("{}", db.current_event_champion(&args[2])?),
        "event-promote" if args.len() == 6 => {
            db.promote_event(&args[2], &args[3], args[4].parse()?, &args[5])?;
            println!("Synthetic event champion metadata promoted");
        }
        "event-rollback" if args.len() == 5 => {
            db.rollback_event(&args[2], &args[3], &args[4])?;
            println!("Synthetic event champion metadata rolled back");
        }
        "event-pin" if args.len() == 5 => {
            db.pin_event_fixture(&args[2], args[3].parse()?, &args[4])?;
            println!("Synthetic event fixture selected");
        }
        "event-pin-match" if args.len() == 4 => {
            if !db.event_replay_matches_pin(&args[2], args[3].parse()?)? {
                return Err("fixture mismatch".into());
            }
            println!("match");
        }
        "event-status" if args.len() == 3 => {
            println!("{}", db.event_outcome_status(args[2].parse()?)?)
        }
        "event-resolve" if args.len() == 6 => {
            db.resolve_event_review(args[2].parse()?, &args[3], &args[4], &args[5])?;
            println!("Synthetic event outcome recorded");
        }
        "event-export" if args.len() == 2 => {
            io::stdout().write_all(db.export_event_outcomes_jsonl()?.as_bytes())?
        }
        "event-retained-followup-check" if args.len() == 8 => {
            let pin = decode_lower_hex::<32>(&args[5])?;
            let set = sese_runtime_spike::attestation::AuthoritySet::from_pinned_json(
                &read_bounded_file(&args[4], 65_536)?,
                &pin,
            )?;
            println!(
                "{}",
                db.recheck_retained_signed_followup(
                    args[2].parse()?,
                    args[3].parse()?,
                    &set,
                    (args[6].parse()?, &args[7])
                )?
            );
        }
        "event-signed-followup-check"
        | "event-signed-followup-record"
        | "event-signed-followup-recheck"
            if (args[0] == "event-signed-followup-recheck" && args.len() == 9)
                || (args[0] != "event-signed-followup-recheck" && args.len() == 8) =>
        {
            use sese_runtime_spike::attestation::{AuthoritySet, NamedSignedClaim};
            let offset = usize::from(args[0] == "event-signed-followup-recheck");
            let bundle: SyntheticAttestationBundle =
                serde_json::from_slice(&read_bounded_file(&args[3 + offset], 131_072)?)?;
            if bundle.schema != "sese-synthetic-attestation-bundle-v1" {
                return Err("invalid attestation bundle schema".into());
            }
            let pin = decode_lower_hex::<32>(&args[5 + offset])?;
            let set = AuthoritySet::from_pinned_json(
                &read_bounded_file(&args[4 + offset], 65_536)?,
                &pin,
            )?;
            let trace_signature = decode_lower_hex::<64>(&bundle.trace.signature)?;
            let outcome_signature = decode_lower_hex::<64>(&bundle.outcome.signature)?;
            let trace = NamedSignedClaim {
                evidence: &bundle.trace.evidence,
                authority_id: &bundle.trace.authority_id,
                signature: &trace_signature,
            };
            let outcome = NamedSignedClaim {
                evidence: &bundle.outcome.evidence,
                authority_id: &bundle.outcome.authority_id,
                signature: &outcome_signature,
            };
            let checkpoint = (args[6 + offset].parse()?, args[7 + offset].as_str());
            let followup = args[2 + offset].parse()?;
            match args[0].as_str() {
                "event-signed-followup-record" => {
                    let id = db.record_signed_synthetic_followup(
                        bundle.trace_id,
                        followup,
                        &set,
                        trace,
                        outcome,
                        checkpoint,
                    )?;
                    println!(
                        "{}",
                        serde_json::json!({"receipt_id":id,"synthetic":true,"training_ready":false})
                    );
                }
                "event-signed-followup-recheck" => {
                    let result = db.recheck_signed_synthetic_followup(
                        args[2].parse()?,
                        followup,
                        &set,
                        trace,
                        outcome,
                        checkpoint,
                    )?;
                    if result["trace_id"].as_i64() != Some(bundle.trace_id) {
                        return Err("bundle trace does not match retained receipt".into());
                    }
                    println!("{result}");
                }
                _ => println!(
                    "{}",
                    db.check_signed_synthetic_followup(
                        bundle.trace_id,
                        followup,
                        &set,
                        trace,
                        outcome,
                        checkpoint
                    )?
                ),
            }
        }
        "event-attestation-check" | "event-attestation-record" if args.len() == 7 => {
            use sese_runtime_spike::attestation::{AuthoritySet, NamedSignedClaim};
            let bundle: SyntheticAttestationBundle =
                serde_json::from_slice(&read_bounded_file(&args[2], 131_072)?)?;
            if bundle.schema != "sese-synthetic-attestation-bundle-v1" {
                return Err("invalid attestation bundle schema".into());
            }
            let pin = decode_lower_hex::<32>(&args[4])?;
            let authority_bytes = read_bounded_file(&args[3], 65_536)?;
            let set = AuthoritySet::from_pinned_json(&authority_bytes, &pin)?;
            let trace_signature = decode_lower_hex::<64>(&bundle.trace.signature)?;
            let outcome_signature = decode_lower_hex::<64>(&bundle.outcome.signature)?;
            let trace_claim = NamedSignedClaim {
                evidence: &bundle.trace.evidence,
                authority_id: &bundle.trace.authority_id,
                signature: &trace_signature,
            };
            let outcome_claim = NamedSignedClaim {
                evidence: &bundle.outcome.evidence,
                authority_id: &bundle.outcome.authority_id,
                signature: &outcome_signature,
            };
            let checkpoint = (args[5].parse()?, args[6].as_str());
            if args[0] == "event-attestation-record" {
                let id = db.record_synthetic_attestation_receipt(
                    bundle.trace_id,
                    &set,
                    trace_claim,
                    outcome_claim,
                    checkpoint,
                )?;
                println!(
                    "{}",
                    serde_json::json!({"receipt_id":id,"synthetic":true,"training_ready":false})
                );
            } else {
                let value = db.check_synthetic_attestation_links_in_set(
                    bundle.trace_id,
                    &set,
                    trace_claim,
                    outcome_claim,
                    checkpoint,
                )?;
                println!("{value}");
            }
        }
        "event-attestation-inspect" if args.len() == 3 => println!(
            "{}",
            db.inspect_synthetic_attestation_receipt(args[2].parse()?)?
        ),
        "event-attestation-recheck" if args.len() == 5 => {
            let pin = decode_lower_hex::<32>(&args[4])?;
            let set = sese_runtime_spike::attestation::AuthoritySet::from_pinned_json(
                &read_bounded_file(&args[3], 65_536)?,
                &pin,
            )?;
            println!(
                "{}",
                db.recheck_synthetic_attestation_receipt(args[2].parse()?, &set)?
            );
        }
        "event-reward-preview" if args.len() == 11 => {
            let authority_pin = decode_lower_hex::<32>(&args[4])?;
            let set = sese_runtime_spike::attestation::AuthoritySet::from_pinned_json(
                &read_bounded_file(&args[3], 65_536)?,
                &authority_pin,
            )?;
            let policy_pin = decode_lower_hex::<32>(&args[6])?;
            let policy = sese_runtime_spike::attestation::SyntheticTrustPolicy::from_pinned_json(
                &read_bounded_file(&args[5], 65_536)?,
                &policy_pin,
                &set,
            )?;
            let rule_pin = decode_lower_hex::<32>(&args[8])?;
            let rule = sese_runtime_spike::reward_rule::SyntheticRewardRule::from_pinned_json(
                &read_bounded_file(&args[7], 65_536)?,
                &rule_pin,
            )?;
            println!(
                "{}",
                db.preview_synthetic_reward(
                    args[2].parse()?,
                    &set,
                    &policy,
                    &rule,
                    (args[9].parse()?, &args[10])
                )?
            );
        }
        "event-common-policy-check" if args.len() == 9 => {
            let authority_pin = decode_lower_hex::<32>(&args[4])?;
            let set = sese_runtime_spike::attestation::AuthoritySet::from_pinned_json(
                &read_bounded_file(&args[3], 65_536)?,
                &authority_pin,
            )?;
            let policy_pin = decode_lower_hex::<32>(&args[6])?;
            let policy = sese_runtime_spike::attestation::SyntheticTrustPolicy::from_pinned_json(
                &read_bounded_file(&args[5], 65_536)?,
                &policy_pin,
                &set,
            )?;
            println!(
                "{}",
                db.check_synthetic_common_evidence_policy(
                    args[2].parse()?,
                    &set,
                    &policy,
                    (args[7].parse()?, &args[8])
                )?
            );
        }
        "event-evidence-policy-check" if args.len() == 10 => {
            let authority_pin = decode_lower_hex::<32>(&args[5])?;
            let set = sese_runtime_spike::attestation::AuthoritySet::from_pinned_json(
                &read_bounded_file(&args[4], 65_536)?,
                &authority_pin,
            )?;
            let policy_pin = decode_lower_hex::<32>(&args[7])?;
            let policy = sese_runtime_spike::attestation::SyntheticTrustPolicy::from_pinned_json(
                &read_bounded_file(&args[6], 65_536)?,
                &policy_pin,
                &set,
            )?;
            println!(
                "{}",
                db.check_synthetic_evidence_policy(
                    args[2].parse()?,
                    args[3].parse()?,
                    &set,
                    &policy,
                    (args[8].parse()?, &args[9])
                )?
            );
        }
        "event-evidence-check" | "event-evidence-export" if args.len() == 8 => {
            let pin = decode_lower_hex::<32>(&args[5])?;
            let set = sese_runtime_spike::attestation::AuthoritySet::from_pinned_json(
                &read_bounded_file(&args[4], 65_536)?,
                &pin,
            )?;
            let attestation_id = args[2].parse()?;
            let preference_id = args[3].parse()?;
            let checkpoint = (args[6].parse()?, args[7].as_str());
            if args[0] == "event-evidence-export" {
                io::stdout().write_all(
                    db.export_synthetic_evidence_package(
                        attestation_id,
                        preference_id,
                        &set,
                        checkpoint,
                    )?
                    .as_bytes(),
                )?;
            } else {
                println!(
                    "{}",
                    db.check_synthetic_evidence_package(
                        attestation_id,
                        preference_id,
                        &set,
                        checkpoint
                    )?
                );
            }
        }
        "event-evidence-verify" if args.len() == 7 => {
            let record = read_bounded_file(&args[2], 1_048_576)?;
            let parsed: serde_json::Value = serde_json::from_slice(&record)?;
            if parsed["schema"].as_str() != Some("sese-synthetic-signed-evidence-v1") {
                return Err("invalid evidence export schema".into());
            }
            let attestation_id = parsed["attestation_receipt_id"]
                .as_i64()
                .ok_or("missing attestation receipt")?;
            let preference_id = parsed["preference_receipt_id"]
                .as_i64()
                .ok_or("missing preference receipt")?;
            let pin = decode_lower_hex::<32>(&args[4])?;
            let set = sese_runtime_spike::attestation::AuthoritySet::from_pinned_json(
                &read_bounded_file(&args[3], 65_536)?,
                &pin,
            )?;
            let expected = db.export_synthetic_evidence_package(
                attestation_id,
                preference_id,
                &set,
                (args[5].parse()?, &args[6]),
            )?;
            if record != expected.as_bytes() {
                return Err("evidence export mismatch".into());
            }
            println!(
                "{}",
                serde_json::json!({"schema":"sese-synthetic-evidence-export-verification-v1","matches_verified_database":true,"signatures_rechecked":true,"authority_config_verified":false,"checkpoint_custody_verified":false,"outcome_truth_verified":false,"synthetic":true,"training_ready":false})
            );
        }
        "event-preference-check" | "event-preference-record" if args.len() == 7 => {
            use sese_runtime_spike::attestation::{
                AuthorityRole, AuthoritySet, NamedSignedClaim, SignedClaim,
            };
            let bundle: SyntheticPreferenceBundle =
                serde_json::from_slice(&read_bounded_file(&args[2], 131_072)?)?;
            if bundle.schema != "sese-synthetic-preference-bundle-v1" {
                return Err("invalid preference bundle schema".into());
            }
            let pin = decode_lower_hex::<32>(&args[4])?;
            let set = AuthoritySet::from_pinned_json(&read_bounded_file(&args[3], 65_536)?, &pin)?;
            let signature = decode_lower_hex::<64>(&bundle.claim.signature)?;
            if args[0] == "event-preference-record" {
                let id = db.record_synthetic_preference_receipt(
                    &set,
                    NamedSignedClaim {
                        evidence: &bundle.claim.evidence,
                        authority_id: &bundle.claim.authority_id,
                        signature: &signature,
                    },
                    (args[5].parse()?, &args[6]),
                )?;
                println!(
                    "{}",
                    serde_json::json!({"receipt_id":id,"synthetic":true,"training_ready":false})
                );
            } else {
                let reviewer = set
                    .get(
                        &bundle.claim.authority_id,
                        AuthorityRole::PreferenceReviewer,
                    )
                    .ok_or("unknown or revoked preference authority")?;
                let mut value = db.check_synthetic_preference_link(
                    SignedClaim {
                        evidence: &bundle.claim.evidence,
                        authority: reviewer,
                        signature: &signature,
                    },
                    (args[5].parse()?, &args[6]),
                )?;
                value["authority_config_digest_matched"] = serde_json::json!(true);
                value["authority_config_sha256"] = serde_json::json!(args[4]);
                println!("{value}");
            }
        }
        "event-preference-inspect" if args.len() == 3 => println!(
            "{}",
            db.inspect_synthetic_preference_receipt(args[2].parse()?)?
        ),
        "event-preference-recheck" if args.len() == 5 => {
            let pin = decode_lower_hex::<32>(&args[4])?;
            let set = sese_runtime_spike::attestation::AuthoritySet::from_pinned_json(
                &read_bounded_file(&args[3], 65_536)?,
                &pin,
            )?;
            println!(
                "{}",
                db.recheck_synthetic_preference_receipt(args[2].parse()?, &set)?
            );
        }
        "event-trace-record" if args.len() == 4 => {
            let file = std::fs::File::open(&args[3])?;
            if !file.metadata()?.is_file() {
                return Err("trace must be a regular file".into());
            }
            let mut bytes = Vec::new();
            file.take(131_073).read_to_end(&mut bytes)?;
            if bytes.is_empty() || bytes.len() > 131_072 {
                return Err("invalid trace file size".into());
            }
            let input: sese_runtime_spike::storage::SyntheticTraceInput =
                serde_json::from_slice(&bytes)?;
            let id = db.record_synthetic_event_trace(args[2].parse()?, &input)?;
            println!(
                "{}",
                serde_json::json!({"trace_id":id,"provenance":"caller_asserted_synthetic","training_ready":false})
            );
        }
        "event-trace-inspect" if args.len() == 3 => {
            println!("{}", db.inspect_synthetic_event_trace(args[2].parse()?)?)
        }
        "event-trace-compare-wat" if args.len() == 3 => {
            println!("{}", db.compare_event_trace_wat(args[2].parse()?)?)
        }
        "event-trace-eligibility" if args.len() == 3 => println!(
            "{}",
            db.synthetic_event_trace_eligibility(args[2].parse()?)?
        ),
        "event-followup-outcome-check" if args.len() == 5 => println!(
            "{}",
            db.check_synthetic_followup_outcome(args[2].parse()?, (args[3].parse()?, &args[4]))?
        ),
        "event-followup-check" if args.len() == 5 => println!(
            "{}",
            db.check_synthetic_event_followup(args[2].parse()?, args[3].parse()?, &args[4])?
        ),
        "event-followup-pin" if args.len() == 6 => {
            let id =
                db.pin_synthetic_event_followup(&args[2], args[3].parse()?, &args[4], &args[5])?;
            println!(
                "{}",
                serde_json::json!({"selection_id":id,"synthetic":true})
            );
        }
        "event-followup-selected-check" if args.len() == 4 => println!(
            "{}",
            db.check_selected_synthetic_event_followup(args[2].parse()?, args[3].parse()?)?
        ),
        "event-followup-record" if args.len() == 4 => {
            let id = db.record_selected_synthetic_followup(args[2].parse()?, args[3].parse()?)?;
            println!(
                "{}",
                serde_json::json!({"receipt_id":id,"synthetic":true,"records_outcome":false,"training_ready":false})
            );
        }
        "event-followup-inspect" if args.len() == 3 => println!(
            "{}",
            db.inspect_synthetic_followup_receipt(args[2].parse()?)?
        ),
        "event-review" if args.len() == 10 => {
            let id = db.record_event_review(
                &args[2],
                args[3].parse()?,
                args[4].parse()?,
                &args[5],
                &args[6],
                &args[7],
                &args[8],
                &args[9],
            )?;
            println!(
                "{}",
                serde_json::json!({"review_id":id,"synthetic":true,"promoted":false})
            );
        }
        "event-review-preview" if args.len() == 4 => {
            println!("{}", db.event_review_preview(args[3].parse()?, &args[2])?)
        }
        "event-review-prepare" if args.len() == 4 || args.len() == 5 => {
            let replay_id: i64 = args[3].parse()?;
            let preview = db.event_review_preview(replay_id, &args[2])?;
            let candidate_id = preview["replay"]["candidate_id"]
                .as_str()
                .ok_or("invalid replay candidate")?
                .to_owned();
            let state_semantics = preview["replay"]["state_semantics"]
                .as_str()
                .ok_or("invalid replay semantics")?
                .to_owned();
            let observation = preview["observations"]
                .as_array()
                .and_then(|v| v.last())
                .ok_or("no observation for domain")?;
            let observation_id = observation["id"].as_i64().ok_or("invalid observation")?;
            let champion_id = db.current_event_champion(&args[2])?;
            if observation["shadow_id"] != candidate_id || observation["champion_id"] != champion_id
            {
                return Err(
                    "latest observation does not match candidate and current champion".into(),
                );
            }
            let card = if args.len() == 5 {
                let input: InputReviewCard =
                    serde_json::from_slice(&read_bounded_file(&args[4], 65_536)?)?;
                if input.schema != "sese-synthetic-review-card-v1"
                    || [&input.human, &input.bridge, &input.proposal]
                        .iter()
                        .any(|s| s.trim().is_empty() || s.len() > 16_384)
                {
                    return Err("invalid review card".into());
                }
                PreparedCard {
                    human: input.human,
                    bridge: input.bridge,
                    proposal: input.proposal,
                }
            } else {
                PreparedCard {
                    human: "Review the synthetic decision.".into(),
                    bridge: "Compare typed replay and shadow results.".into(),
                    proposal: "Consider the recorded synthetic candidate.".into(),
                }
            };
            let (seq, hash) = db.ledger_checkpoint()?;
            if preview["checkpoint"] != serde_json::json!({"seq":seq,"hash":hash}) {
                return Err("evidence changed while preparing review; retry".into());
            }
            let prepared = PreparedEvaluation {
                schema: "sese-prepared-synthetic-evaluation-v1".into(),
                synthetic: true,
                review_recorded: false,
                domain: args[2].clone(),
                candidate_id,
                champion_id,
                replay_id,
                observation_id,
                state_semantics,
                checkpoint: preview["checkpoint"].clone(),
                preview,
                card,
            };
            println!("{}", serde_json::to_string(&prepared)?);
        }
        "event-review-prepared" if args.len() == 5 => {
            let mut source: serde_json::Value =
                serde_json::from_slice(&read_bounded_file(&args[2], 8_388_608)?)?;
            if source["schema"] == "sese-container-review-preview-v1" {
                source = source
                    .get_mut("prepared")
                    .ok_or("comparison lacks prepared evaluation")?
                    .take();
            }
            let prepared: PreparedEvaluation = serde_json::from_value(source)?;
            if !["approve", "reject"].contains(&args[4].as_str()) {
                return Err("invalid decision".into());
            }
            let preview = db.event_review_preview(prepared.replay_id, &prepared.domain)?;
            verify_prepared(&prepared, &preview, &mut db)?;
            let seq = prepared.checkpoint["seq"]
                .as_i64()
                .ok_or("invalid prepared checkpoint")?;
            let hash = prepared.checkpoint["hash"]
                .as_str()
                .ok_or("invalid prepared checkpoint")?;
            let id = db.decide_prepared_event(
                &prepared.domain,
                &prepared.champion_id,
                &prepared.candidate_id,
                prepared.replay_id,
                prepared.observation_id,
                &args[3],
                &args[4],
                &prepared.card.human,
                &prepared.card.bridge,
                &prepared.card.proposal,
                (seq, hash),
            )?;
            println!(
                "{}",
                serde_json::json!({"review_id":id,"synthetic":true,"promoted":args[4]=="approve"})
            );
        }
        "event-review-at" if args.len() == 13 => {
            let id = db.record_event_review_from_preview(
                &args[2],
                &args[3],
                args[4].parse()?,
                args[5].parse()?,
                &args[6],
                &args[7],
                &args[8],
                &args[9],
                &args[10],
                (args[11].parse()?, &args[12]),
            )?;
            println!(
                "{}",
                serde_json::json!({"review_id":id,"synthetic":true,"promoted":false})
            );
        }
        "event-inspect" if args.len() == 3 => println!(
            "{}",
            serde_json::to_string(&db.inspect_event_review(args[2].parse()?)?)?
        ),
        "event-observations" if args.len() == 4 => println!(
            "{}",
            db.inspect_event_observations(&args[2], args[3].parse()?)?
        ),
        "event-history" if args.len() == 4 || args.len() == 5 => println!(
            "{}",
            db.inspect_event_history(
                &args[2],
                args[3].parse()?,
                if args.len() == 5 {
                    args[4].parse()?
                } else {
                    100
                }
            )?
        ),
        "event-replay-inspect" if args.len() == 3 => {
            println!("{}", db.inspect_event_replay(args[2].parse()?)?)
        }
        "window-inspect" if args.len() == 3 => println!(
            "{}",
            serde_json::to_string(&db.inspect_window_review(args[2].parse()?)?)?
        ),
        "window-status" if args.len() == 3 => {
            println!("{}", db.window_outcome_status(args[2].parse()?)?)
        }
        "window-resolve" if args.len() == 6 => {
            db.resolve_window_review(args[2].parse()?, &args[3], &args[4], &args[5])?;
            println!("Synthetic window outcome recorded");
        }
        "window-export" if args.len() == 2 => {
            io::stdout().write_all(db.export_window_rlvr_jsonl()?.as_bytes())?
        }
        "export" if args.len() == 2 => {
            io::stdout().write_all(db.export_rlvr_jsonl()?.as_bytes())?
        }
        _ => return Err("invalid arguments".into()),
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        if std::env::args().nth(1).as_deref() == Some("event-training-export") {
            eprintln!(
                "{}",
                serde_json::json!({"schema":"sese-synthetic-training-export-report-v1","status":"not_exported","count":0,"reason":error.to_string(),"synthetic":true})
            );
        } else {
            eprintln!("outcome command failed: {error}");
        }
        std::process::exit(1);
    }
}
