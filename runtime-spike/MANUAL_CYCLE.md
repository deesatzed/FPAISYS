# Manual synthetic event cycle

This walkthrough uses fixed built-in Wasm modules and synthetic data. Run it from the repository root in a shell. Use a trusted local installation of the binaries; the worker is not an OS sandbox. Do not run untrusted candidates or sensitive data. The commands below keep their variables in the current shell.

## Initialize and replay

```sh
cargo build --locked --manifest-path runtime-spike/Cargo.toml --bins
SSESE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/sese-manual.XXXXXX")"
SSESE_DB="$SSESE_DIR/cycle.db"
SSESE_CMD="runtime-spike/target/debug/sese-outcome"
"$SSESE_CMD" event-init "$SSESE_DB"
"$SSESE_CMD" event-builtin-register "$SSESE_DB" allow > "$SSESE_DIR/allow.json"
"$SSESE_CMD" event-builtin-register "$SSESE_DB" throttle > "$SSESE_DIR/throttle.json"
"$SSESE_CMD" event-builtin-replay "$SSESE_DB" throttle runtime-spike/fixtures/event-measured-synthetic-v1.json > "$SSESE_DIR/replay.json"
cat "$SSESE_DIR/replay.json"
SSESE_CHAMPION="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["candidate_id"])' "$SSESE_DIR/allow.json")"
SSESE_CANDIDATE="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["candidate_id"])' "$SSESE_DIR/throttle.json")"
SSESE_REPLAY="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["run_id"])' "$SSESE_DIR/replay.json")"
"$SSESE_CMD" event-replay-inspect "$SSESE_DB" "$SSESE_REPLAY"
```

Inspect the expected and actual case results. Continue only if `passed` is true. A failed replay is retained for inspection and cannot be selected as the fixture.

## Select the fixture and observe

```sh
"$SSESE_CMD" event-pin "$SSESE_DB" manual "replay:$SSESE_REPLAY" local-operator
"$SSESE_CMD" event-bootstrap "$SSESE_DB" manual "$SSESE_CHAMPION" local-operator
"$SSESE_CMD" event-builtin-observe "$SSESE_DB" manual manual-stream allow throttle measured-v2 1 42 3 4 > "$SSESE_DIR/first-observation.json"
"$SSESE_CMD" event-builtin-observe "$SSESE_DB" manual manual-stream allow throttle measured-v2 2 43 5 1 > "$SSESE_DIR/observation.json"
cat "$SSESE_DIR/observation.json"
SSESE_OBSERVATION="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["observation_id"])' "$SSESE_DIR/observation.json")"
"$SSESE_CMD" event-review-preview "$SSESE_DB" manual "$SSESE_REPLAY" > "$SSESE_DIR/preview.json"
cat "$SSESE_DIR/preview.json"
SSESE_SEQ="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["checkpoint"]["seq"])' "$SSESE_DIR/preview.json")"
SSESE_HASH="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["checkpoint"]["hash"])' "$SSESE_DIR/preview.json")"
```

Review the replay and both verdicts in the preview. The second observation's prior window includes the first event's metric and measured champion duration; those durations include process startup and are not performance benchmarks. Its checkpoint guards against intervening ledger changes; if it becomes stale, obtain and review a new preview. The following example card is synthetic; edit its three columns to reflect your own review before recording a decision.

## Record an explicit decision

```sh
SSESE_HUMAN='Review the synthetic event and its expected result.'
SSESE_BRIDGE='Compare the champion and shadow verdicts with the replay.'
SSESE_PROPOSAL='Consider the synthetic throttle candidate.'
printf 'Enter approve or reject: '
read -r SSESE_ACTION
"$SSESE_CMD" event-review-at "$SSESE_DB" manual "$SSESE_CANDIDATE" "$SSESE_REPLAY" "$SSESE_OBSERVATION" local-operator "$SSESE_ACTION" "$SSESE_HUMAN" "$SSESE_BRIDGE" "$SSESE_PROPOSAL" "$SSESE_SEQ" "$SSESE_HASH" > "$SSESE_DIR/review.json"
cat "$SSESE_DIR/review.json"
SSESE_REVIEW="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["review_id"])' "$SSESE_DIR/review.json")"
"$SSESE_CMD" event-inspect "$SSESE_DB" "$SSESE_REVIEW"
```

Only `approve` or `reject` is accepted. Recording approval does not itself promote the candidate. If you rejected it, skip the next block and inspect history below.

## Promote and optionally roll back

Run this block only after an approval and only if you want to promote the candidate. Enter `rollback` or `keep` at the prompt.

```sh
"$SSESE_CMD" event-promote "$SSESE_DB" manual "$SSESE_CHAMPION" "$SSESE_REVIEW" local-operator
"$SSESE_CMD" event-builtin-observe "$SSESE_DB" manual manual-stream throttle allow measured-v2 3 44 6 0
printf 'Enter rollback or keep: '
read -r SSESE_ROLLBACK
case "$SSESE_ROLLBACK" in
  rollback) "$SSESE_CMD" event-rollback "$SSESE_DB" manual "$SSESE_CANDIDATE" local-operator ;;
  keep) printf 'Champion retained.\n' ;;
  *) printf 'No rollback performed.\n' ;;
esac
```

## Inspect history and record a synthetic outcome

The outcome starts pending. Enter `validated`, `falsified`, or `skip`; any resolution is your synthetic assertion, not an independently observed real-world outcome. Pending records are excluded from the outcome export.

```sh
"$SSESE_CMD" event-status "$SSESE_DB" "$SSESE_REVIEW"
printf 'Enter validated, falsified, or skip: '
read -r SSESE_OUTCOME
case "$SSESE_OUTCOME" in
  validated|falsified) "$SSESE_CMD" event-resolve "$SSESE_DB" "$SSESE_REVIEW" "$SSESE_OUTCOME" local-operator 'synthetic assertion' ;;
  *) printf 'Outcome remains pending.\n' ;;
esac
"$SSESE_CMD" event-history "$SSESE_DB" manual 0
"$SSESE_CMD" event-export "$SSESE_DB"
"$SSESE_CMD" checkpoint "$SSESE_DB" > "$SSESE_DIR/checkpoint.json"
SSESE_SEQ="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["seq"])' "$SSESE_DIR/checkpoint.json")"
SSESE_HASH="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["hash"])' "$SSESE_DIR/checkpoint.json")"
python3 runtime-spike/scripts/verify_ledger.py "$SSESE_DB" --seq "$SSESE_SEQ" --hash "$SSESE_HASH" --verify-event-cycle
printf 'Artifacts: %s\n' "$SSESE_DIR"
```

The verifier reports the number of measured stream receipts checked; it reconstructs their state transitions, but does not validate the observations themselves. The export is synthetic and `training_ready: false`. Keep a checkpoint separately from the database for later comparison; the file above is only a local convenience copy and is not a signature. This walkthrough does not validate authority, fixture quality, isolation or production use.
