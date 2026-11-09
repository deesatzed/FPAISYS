# Synthetic event-cycle walkthrough

For the same synthetic lifecycle using separate operator commands and explicit decisions, see [MANUAL_CYCLE.md](MANUAL_CYCLE.md).
For a shorter noninteractive preparation followed by one decision command, see [README.md](README.md).

This walkthrough runs fixed synthetic Wasm guests against local sample data. The worker is not an OS sandbox: do not use untrusted candidates or sensitive data. It requires Rust/Cargo and Python 3; run commands from the repository root.

## Build and prepare

```sh
cargo build --locked --manifest-path runtime-spike/Cargo.toml --bins
SSESE_RUN_DIR="$(mktemp -d "${TMPDIR:-/tmp}/sese-walkthrough.XXXXXX")"
printf 'Run directory: %s\n' "$SSESE_RUN_DIR"
```

The new directory holds the database and checkpoint. The demo requires a database path that does not already exist.

## Run and review

```sh
runtime-spike/target/debug/sese-event-cycle-demo \
  runtime-spike/target/debug/event-worker "$SSESE_RUN_DIR/demo.db" \
  --fixture runtime-spike/fixtures/event-synthetic-v1.json \
  --card runtime-spike/fixtures/review-card-synthetic-v1.json \
  --followup-fixture runtime-spike/fixtures/event-followup-synthetic-v1.json \
  --followup-case post-promotion
```

Read the replay, observations and card. The separate follow-up fixture and its result are visible before the decision; it is not a blinded or independent holdout. Enter `approve` or `reject`. Approval changes the synthetic champion; choose `rollback` or `keep` at the next prompt. Rejection leaves it unchanged. Note the printed review ID; for a fresh walkthrough it is `1`. The remaining commands assume `approve` followed by `rollback`.

To try the other built-in synthetic candidate, use a new database and run `runtime-spike/target/debug/sese-event-cycle-demo runtime-spike/target/debug/event-worker "$SSESE_RUN_DIR/quarantine.db" --candidate quarantine`. This uses its built-in quarantine replay case; the throttle fixtures in the walkthrough above do not match it. Arbitrary candidate code remains unsupported without containment.

## Inspect and record an outcome

```sh
runtime-spike/target/debug/sese-outcome event-inspect "$SSESE_RUN_DIR/demo.db" 1
runtime-spike/target/debug/sese-outcome event-observations "$SSESE_RUN_DIR/demo.db" typed-cycle 0
runtime-spike/target/debug/sese-outcome event-status "$SSESE_RUN_DIR/demo.db" 1
runtime-spike/target/debug/sese-outcome event-followup-record "$SSESE_RUN_DIR/demo.db" 1 2
runtime-spike/target/debug/sese-outcome event-followup-inspect "$SSESE_RUN_DIR/demo.db" 1
runtime-spike/target/debug/sese-outcome event-resolve "$SSESE_RUN_DIR/demo.db" 1 validated demo-operator 'synthetic assertion'
runtime-spike/target/debug/sese-outcome event-export "$SSESE_RUN_DIR/demo.db"
runtime-spike/target/debug/sese-outcome event-history "$SSESE_RUN_DIR/demo.db" typed-cycle 0
```

In this fresh run, observation `2` is the post-promotion observation and receipt `1` records its comparison with the preselected case. Check the observation list and receipt rather than assuming these IDs in another database. The status starts as `pending`; the receipt does not resolve it. Resolution is a caller assertion, not independent validation. The export includes the receipt separately and remains synthetic and `training_ready: false`. Use `falsified` only when that is the intended synthetic assertion. Resolution does not promote a candidate.

## Optional: inspect conditional training examples

For a separate fresh demonstration, run the command below. It creates a new directory, prints a synthetic review, and requires an explicit `approve` or `reject` decision. After approval, choose `rollback` or `keep`; the demo records a synthetic validated fixture comparison and writes one demonstration-only SFT, DPO and RLVR example. Rejection writes no training rows.

```sh
runtime-spike/target/debug/sese-training-demo "$SSESE_RUN_DIR/training-demo" accept-synthetic-trust-v1
cat "$SSESE_RUN_DIR/training-demo/manifest.json"
cat "$SSESE_RUN_DIR/training-demo/sft.jsonl"
cat "$SSESE_RUN_DIR/training-demo/dpo.jsonl"
cat "$SSESE_RUN_DIR/training-demo/rlvr.jsonl"
```

The explicit prompt and preference pair come from `trace-fixture.json`; they are synthetic examples, not model output or a preference inferred from a decision. The demo uses fixed public test keys and one operator for all roles. The manifest, authority set, policy, reward rule and checkpoint are stored next to the database for inspection and reproduction, not independent custody. The saved examples are explicitly marked demonstration-only and ineligible for training because the keys are public and all roles are controlled by one process. The saved policy explicitly disallows training export, so the lower-level exporter rejects it. The policy gate is not cryptographic custody: anyone who replaces the policy and pin can make a different declaration. The previews do not establish model authorship, independent observation or real-world validity. Never use the demo keys for real evidence. A failed run may leave a partial new directory; choose a new directory for a retry.

## Retain and verify a checkpoint

```sh
runtime-spike/target/debug/sese-outcome checkpoint "$SSESE_RUN_DIR/demo.db" > "$SSESE_RUN_DIR/checkpoint.json"
SSESE_SEQ="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["seq"])' "$SSESE_RUN_DIR/checkpoint.json")"
SSESE_HASH="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["hash"])' "$SSESE_RUN_DIR/checkpoint.json")"
python3 runtime-spike/scripts/verify_ledger.py "$SSESE_RUN_DIR/demo.db" --seq "$SSESE_SEQ" --hash "$SSESE_HASH" --verify-event-cycle
```

The `--verify-event-cycle` option checks synthetic candidate links, typed-event replay, observations, reviews, outcomes, champion history, follow-up gates, training trace bindings, and stream receipts. It requires the checkpoint to cover the ledger head in its read snapshot. Inspect the individual counts and `event_streams_without_receipts`; older streams may lack receipts. The JSON continues to mark overall evidence verification incomplete. For later tamper detection, retain the checkpoint separately from the database and protect it; the local file created above is only a convenience copy. A checkpoint is not a signature, and these checks do not validate fixture quality, authenticate actors or prove worker isolation.

Run the automated suite with `cargo test --locked --manifest-path runtime-spike/Cargo.toml`. See [README.md](README.md) for failure inspection, guarded review resumption and implementation limits.
