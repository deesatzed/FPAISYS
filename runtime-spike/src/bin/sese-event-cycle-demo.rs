//! Synthetic typed-event lifecycle with fixed guests; not an isolation boundary.
use sese_runtime_spike::{
    event_abi::{Decision, Event, Verdict, Window},
    event_host::{EventHost, MeasuredEventHost, Observation},
    event_replay::{self, Case},
    fixture_file,
    storage::Storage,
};
use std::{
    io::{self, Read, Write},
    time::Duration,
};
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EventFile {
    schema: String,
    events: Vec<FileEvent>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FileEvent {
    timestamp_ns: u64,
    entity_id: u64,
    metric: f64,
    flags: u32,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewCard {
    schema: String,
    human: String,
    bridge: String,
    proposal: String,
}
fn load_card(path: &str) -> Result<ReviewCard, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err("review card must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(65_537).read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() > 65_536 {
        return Err("invalid review card size".into());
    }
    let card: ReviewCard = serde_json::from_slice(&bytes)?;
    if card.schema != "sese-synthetic-review-card-v1"
        || [&card.human, &card.bridge, &card.proposal]
            .iter()
            .any(|field| field.trim().is_empty() || field.len() > 16_384)
    {
        return Err("invalid review card".into());
    }
    Ok(card)
}
fn load_events(path: &str, measured: bool) -> Result<Vec<Event>, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err("event stream must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(65_537).read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() > 65_536 {
        return Err("invalid event stream size".into());
    }
    let input: EventFile = serde_json::from_slice(&bytes)?;
    if input.schema != "sese-synthetic-events-v1"
        || input.events.is_empty()
        || input.events.len() > 100
    {
        return Err("invalid synthetic event stream".into());
    }
    input
        .events
        .into_iter()
        .map(|e| {
            let minimum = if measured { -1_000_000_000_000.0 } else { 0.0 };
            if !e.metric.is_finite() || !(minimum..=1_000_000_000_000.0).contains(&e.metric) {
                return Err("invalid synthetic event metric".into());
            }
            Ok(Event {
                timestamp_ns: e.timestamp_ns,
                entity_id: e.entity_id,
                metric: e.metric,
                flags: e.flags,
            })
        })
        .collect()
}
enum DemoHost {
    Legacy(EventHost),
    Measured(MeasuredEventHost),
}
impl DemoHost {
    fn process(
        &mut self,
        db: &mut Storage,
        worker: &str,
        domain: &str,
        shadow: &str,
        event: &Event,
    ) -> Result<Observation, String> {
        match self {
            Self::Legacy(host) => {
                host.process_domain(db, worker, domain, shadow, event, 0, Duration::from_secs(2))
            }
            Self::Measured(host) => {
                host.process_domain(db, worker, domain, shadow, event, Duration::from_secs(2))
            }
        }
    }
}
fn choice(prompt: &str, allowed: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let answer = line.trim();
    if !allowed.contains(&answer) {
        return Err("explicit valid choice required".into());
    }
    Ok(answer.into())
}
fn verdict_value(v: Verdict) -> serde_json::Value {
    serde_json::json!({"decision":format!("{:?}", v.decision),"score":v.score,"score_bits":format!("{:016x}",v.score.to_bits()),"reason":v.reason})
}
fn event_value(e: Event) -> serde_json::Value {
    serde_json::json!({"timestamp_ns":e.timestamp_ns,"entity_id":e.entity_id,"metric":e.metric,"metric_bits":format!("{:016x}",e.metric.to_bits()),"flags":e.flags})
}
fn window_value(w: Window) -> serde_json::Value {
    serde_json::json!({"count":w.count,"mean":w.mean,"mean_bits":format!("{:016x}",w.mean.to_bits()),"p99_us":w.p99_us,"last_error":w.last_error})
}
fn display_verdict(value: &mut serde_json::Value) {
    let decision = &value["decision"];
    let label = match (decision.as_u64(), decision.as_str()) {
        (Some(0), _) | (_, Some("allow")) => Some("Allow"),
        (Some(1), _) | (_, Some("throttle")) => Some("Throttle"),
        (Some(2), _) | (_, Some("quarantine")) => Some("Quarantine"),
        (Some(3), _) | (_, Some("escalate")) => Some("Escalate"),
        _ => None,
    };
    if let Some(label) = label {
        value["decision"] = label.into();
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return Err("usage: sese-event-cycle-demo EVENT_WORKER_PATH [NEW_DATABASE_PATH] [--candidate throttle|quarantine] [--measured] [--fixture FILE] [--events FILE] [--card FILE] [--followup-fixture FILE --followup-case ID]".into());
    }
    let mut measured = false;
    let mut path = None;
    let mut fixture_path = None;
    let mut events_path = None;
    let mut card_path = None;
    let mut followup_path = None;
    let mut followup_case = None;
    let mut candidate_name = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--candidate" if candidate_name.is_none() && i + 1 < args.len() => {
                i += 1;
                candidate_name = Some(args[i].as_str());
            }
            "--measured" if !measured => measured = true,
            "--fixture" if fixture_path.is_none() && i + 1 < args.len() => {
                i += 1;
                fixture_path = Some(args[i].as_str());
            }
            "--events" if events_path.is_none() && i + 1 < args.len() => {
                i += 1;
                events_path = Some(args[i].as_str());
            }
            "--card" if card_path.is_none() && i + 1 < args.len() => {
                i += 1;
                card_path = Some(args[i].as_str());
            }
            "--followup-fixture" if followup_path.is_none() && i + 1 < args.len() => {
                i += 1;
                followup_path = Some(args[i].as_str());
            }
            "--followup-case" if followup_case.is_none() && i + 1 < args.len() => {
                i += 1;
                followup_case = Some(args[i].as_str());
            }
            value if !value.starts_with('-') && path.is_none() => path = Some(value),
            _ => return Err("invalid arguments".into()),
        }
        i += 1;
    }
    // Only these fixed synthetic modules can run through this demo; user artifacts remain untrusted.
    let (candidate_label, candidate_decision, case_id, legacy_version, measured_version) =
        match candidate_name.unwrap_or("throttle") {
            "throttle" => (
                "throttle",
                Decision::Throttle,
                "synthetic-throttle",
                "typed-cycle-v1",
                "typed-cycle-v2",
            ),
            "quarantine" => (
                "quarantine",
                Decision::Quarantine,
                "synthetic-quarantine",
                "typed-quarantine-v1",
                "typed-quarantine-v2",
            ),
            _ => return Err("unknown synthetic candidate".into()),
        };
    let fixture = fixture_path.map(fixture_file::load).transpose()?;
    if followup_path.is_some() != followup_case.is_some() {
        return Err("follow-up fixture and case must be provided together".into());
    }
    let followup_fixture = followup_path.map(fixture_file::load).transpose()?;
    let semantics = if measured { "measured-v2" } else { "legacy-v1" };
    if fixture
        .as_ref()
        .is_some_and(|pack| pack.state_semantics != semantics)
    {
        return Err("fixture state semantics do not match demo mode".into());
    }
    if followup_fixture.as_ref().is_some_and(|pack| {
        pack.state_semantics != semantics
            || !pack
                .cases
                .iter()
                .any(|case| Some(case.id.as_str()) == followup_case)
    }) {
        return Err("invalid follow-up fixture semantics or selected case".into());
    }
    let default_event = Event {
        timestamp_ns: 1,
        entity_id: 42,
        metric: 3.0,
        flags: 4,
    };
    let events = match events_path {
        Some(path) => load_events(path, measured)?,
        None => vec![default_event],
    };
    let card = match card_path {
        Some(path) => load_card(path)?,
        None => ReviewCard {
            schema: "sese-synthetic-review-card-v1".into(),
            human: "Review the synthetic decision.".into(),
            bridge: "Compare typed replay and shadow results.".into(),
            proposal: format!("Apply {candidate_label} to the synthetic event."),
        },
    };
    let conn = if let Some(path) = path {
        let _file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?
    } else {
        rusqlite::Connection::open_in_memory()?
    };
    let mut db = Storage::from_connection(conn)?;
    let original =
        db.register_synthetic_event(&sese_runtime_spike::builtin_event::artifact("allow")?)?;
    let candidate = db.register_synthetic_event(&sese_runtime_spike::builtin_event::artifact(
        candidate_label,
    )?)?;
    let event = default_event;
    let case = Case {
        id: case_id.into(),
        window: Window {
            count: 0,
            mean: 0.0,
            p99_us: 0,
            last_error: 0,
        },
        event,
        expected: Verdict {
            decision: candidate_decision,
            score: 3.0,
            reason: 4,
        },
    };
    let mut replay = if let Some(pack) = fixture.as_ref() {
        pack.replay_synthetic(&mut db, &args[0], &candidate, Duration::from_secs(2))?
    } else if measured {
        event_replay::replay_measured(
            &mut db,
            &args[0],
            &candidate,
            measured_version,
            &[case.clone()],
            Duration::from_secs(2),
        )?
    } else {
        event_replay::replay(
            &mut db,
            &args[0],
            &candidate,
            legacy_version,
            &[case.clone()],
            Duration::from_secs(2),
        )?
    };
    if !replay.passed {
        println!("Synthetic replay {}: candidate={candidate}", replay.run_id);
        for result in &replay.cases {
            println!(
                "Replay case: {}",
                serde_json::json!({"id":result.case.id,"window":window_value(result.case.window),"event":event_value(result.case.event),"expected":verdict_value(result.case.expected),"actual":result.actual.map(verdict_value),"error":result.error,"passed":result.passed})
            );
        }
        return Err("synthetic replay failed".into());
    }
    db.pin_event_fixture("typed-cycle", replay.run_id, "local-operator")?;
    if let (Some(pack), Some(case_id)) = (followup_fixture, followup_case) {
        let follow =
            pack.replay_synthetic(&mut db, &args[0], &candidate, Duration::from_secs(2))?;
        println!(
            "Separate synthetic follow-up replay {}: passed={}; selected case={case_id}",
            follow.run_id, follow.passed
        );
        for result in &follow.cases {
            println!(
                "Follow-up replay case: {}",
                serde_json::json!({"id":result.case.id,"window":window_value(result.case.window),"event":event_value(result.case.event),"expected":verdict_value(result.case.expected),"actual":result.actual.map(verdict_value),"error":result.error,"passed":result.passed})
            );
        }
        if !follow.passed {
            return Err("synthetic follow-up replay failed".into());
        }
        db.pin_synthetic_event_followup("typed-cycle", follow.run_id, case_id, "local-operator")?;
        // Keep the reviewed replay as the newest candidate replay without weakening freshness.
        replay = if let Some(pack) = fixture.as_ref() {
            pack.replay_synthetic(&mut db, &args[0], &candidate, Duration::from_secs(2))?
        } else if measured {
            event_replay::replay_measured(
                &mut db,
                &args[0],
                &candidate,
                measured_version,
                &[case.clone()],
                Duration::from_secs(2),
            )?
        } else {
            event_replay::replay(
                &mut db,
                &args[0],
                &candidate,
                legacy_version,
                &[case.clone()],
                Duration::from_secs(2),
            )?
        };
        if !replay.passed {
            return Err("synthetic reviewed fixture recheck failed".into());
        }
    }
    db.bootstrap_event_champion("typed-cycle", &original, "local-operator")?;
    let mut host = if measured {
        DemoHost::Measured(MeasuredEventHost::open(&mut db, "typed-cycle", 32)?)
    } else {
        DemoHost::Legacy(EventHost::open(&mut db, "typed-cycle", 32)?)
    };
    let mut last_observation = None;
    for input in &events {
        let observation = host.process(&mut db, &args[0], "typed-cycle", &candidate, input)?;
        if observation.champion.map(|v| v.decision) != Some(Decision::Allow)
            || observation.shadow.map(|v| v.decision) != Some(candidate_decision)
        {
            println!(
                "Synthetic observation: {}",
                serde_json::json!({"id":observation.id,"event":event_value(*input),"prior":window_value(observation.prior),"champion":observation.champion.map(verdict_value),"champion_error":observation.champion_error,"shadow":observation.shadow.map(verdict_value),"shadow_error":observation.shadow_error})
            );
            return Err("synthetic comparison failed".into());
        }
        last_observation = Some(observation);
    }
    let observed = last_observation.ok_or("empty event stream")?;
    let event = *events.last().ok_or("empty event stream")?;
    let preview = db.event_review_preview(replay.run_id, "typed-cycle")?;
    let observations = preview["observations"]
        .as_array()
        .ok_or("invalid preview")?;
    if preview["replay"]["candidate_id"] != candidate
        || observations.len() != events.len()
        || observations.last().and_then(|v| v["id"].as_i64()) != Some(observed.id)
    {
        return Err("review preview differs from the completed cycle".into());
    }
    let review_seq = preview["checkpoint"]["seq"]
        .as_i64()
        .ok_or("invalid preview checkpoint")?;
    let review_hash = preview["checkpoint"]["hash"]
        .as_str()
        .ok_or("invalid preview checkpoint")?;
    println!("Review snapshot: {preview}");
    println!("Synthetic replay {}: candidate={candidate}", replay.run_id);
    for case in preview["replay"]["cases"]
        .as_array()
        .ok_or("invalid preview cases")?
    {
        let mut case = case.clone();
        display_verdict(&mut case["expected"]);
        if !case["actual"].is_null() {
            display_verdict(&mut case["actual"]);
        }
        println!("Replay case: {case}");
    }
    for observation in observations {
        let mut observation = observation.clone();
        if !observation["champion"].is_null() {
            display_verdict(&mut observation["champion"]);
        }
        if !observation["shadow"].is_null() {
            display_verdict(&mut observation["shadow"]);
        }
        println!("Synthetic observation: {observation}");
    }
    println!("Synthetic cycle only; no untrusted modules or production traffic.");
    println!(
        "state semantics: {}",
        preview["replay"]["state_semantics"]
            .as_str()
            .ok_or("invalid preview semantics")?
    );
    println!(
        "Before review: champion=allow shadow={candidate_label}; replay {} observation {}; fixture {} {}",
        replay.run_id,
        observed.id,
        preview["replay"]["fixture_version"]
            .as_str()
            .ok_or("invalid preview version")?,
        preview["replay"]["fixture_digest"]
            .as_str()
            .ok_or("invalid preview digest")?
    );
    println!(
        "Decision card: {}",
        serde_json::json!({"human":card.human,"bridge":card.bridge,"proposal":card.proposal})
    );
    let action = choice("Enter approve or reject: ", &["approve", "reject"])?;
    let review = db.record_event_review_at_checkpoint(
        &candidate,
        replay.run_id,
        observed.id,
        "local-operator",
        &action,
        &card.human,
        &card.bridge,
        &card.proposal,
        (review_seq, review_hash),
    )?;
    println!("Review {review}: {action}; outcome pending.");
    if action == "reject" {
        println!(
            "Rejected; champion unchanged: {}",
            db.current_event_champion("typed-cycle")?
        );
        return Ok(());
    }
    db.promote_event("typed-cycle", &original, review, "local-operator")?;
    let promoted = host.process(&mut db, &args[0], "typed-cycle", &original, &event)?;
    if promoted.champion.map(|v| v.decision) != Some(candidate_decision) {
        return Err("promoted synthetic guest failed".into());
    }
    println!("After promotion: champion={candidate_label}");
    if choice("Enter rollback or keep: ", &["rollback", "keep"])? == "rollback" {
        db.rollback_event("typed-cycle", &candidate, "local-operator")?;
        let rolled = host.process(&mut db, &args[0], "typed-cycle", &candidate, &event)?;
        if rolled.champion.map(|v| v.decision) != Some(Decision::Allow) {
            return Err("rolled back synthetic guest failed".into());
        }
        println!("After rollback: champion=allow");
    }
    println!("Ledger valid: {}", db.verify_ledger()?);
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("event cycle demo failed: {e}");
        std::process::exit(1);
    }
}
