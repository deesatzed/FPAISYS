# S-SESE runtime experiment

This experimental package includes synthetic scalar, window, and typed-event workflows, local champion transitions, and review records. Its typed-event ABI is a flattened core-Wasm experiment. The local worker has no OS sandbox; the experimental container path is not verified for untrusted artifacts. Do not use it with untrusted artifacts or sensitive data.

The synthetic container bridge forwards the original request bytes after checking that they exactly match the selected built-in and input values. This does not enable execution of unclassified or arbitrary candidates.

For a fixed-timestamp build experiment, add `--build-arg SOURCE_DATE_EPOCH=0` to the Docker build command below. The Dockerfile normalizes the worker executable's timestamp. On the tested Linux arm64 builder, two sequential uncached builds with that argument produced matching filesystem layers and inspected configuration but different image IDs. This is a local reproducibility observation, not image provenance or an authentication mechanism; Cargo dependencies still require network access on an empty build cache.

An experimental Linux typed-event worker image can be built locally from the repository root with `docker build -f runtime-spike/Dockerfile.worker -t sese-worker-experiment:local runtime-spike`. The Dockerfile pins its base-image indexes and the build uses `Cargo.lock`; it can download those bases and Cargo dependencies. Its Dockerfile-specific ignore file limits the context to the manifests, `src/`, and vendored Wasmi, including all files under those directories. This image starts the worker with a fail-closed Linux self-check of selected process, cgroup, root-filesystem and network conditions before it reads a guest request. The application bridge requires Docker to report that exact entrypoint, no additional command arguments, and the pinned image's PATH-only environment before starting a container; images with additional environment variables are rejected. Direct synthetic lifecycle experiments do not require this profile by default. Neither the image nor the Docker report is authenticated, so the check cannot establish containment or authorize untrusted candidates. See `../contracts/container-probe-results.md`.

`scripts/synthetic_container_lifecycle.py` is a built-in synthetic experiment. `run_synthetic` accepts `allow`, `throttle`, `quarantine` or `window-probe` and typed synthetic event and window tuples; it does not accept arbitrary Wasm. It returns the valid worker verdict and selected artifact digest, but does not prove that the selected image actually executed the artifact. It requires a locally available image selected by its full `sha256:` identity, an existing caller-selected `journal_dir`, and an explicit `docker_host` Unix socket URI such as `unix:///path/to/docker.sock`. It supplies that endpoint to each Docker command and removes inherited `DOCKER_*` overrides. Select a known local socket: a Unix socket path does not authenticate the daemon or prove it is local. The helper first writes an owner-only `not_created` preflight record; after read-only checks it durably changes that record to a pending intent with the daemon ID before attempting container creation. A valid preflight record does not block retries, while a pending intent does. After confirmed removal it atomically records the container ID, a valid-verdict or failed status, and the output digest when applicable. A valid verdict has not been compared with a replay fixture. Records omit the request itself and cannot independently reconstruct it; their hashes and Docker metadata are unauthenticated. Use a directory retained by the operator. The helper creates a container without starting it and checks Docker-reported restrictions before launch; mismatches fail closed and trigger cleanup. The report is unauthenticated and does not prove kernel enforcement. It limits the verdict to 16 bytes, and removes and checks the captured container ID before returning a result. It withholds the result and avoids issuing removal if a daemon mismatch is detected before cleanup. Use only a known local synthetic worker image. A missing ID or failed cleanup is reported as unconfirmed; inspect the retained pending record. A shared evaluation deadline covers journal-lock waiting, Docker preflight and worker output; process creation, receipt filesystem operations and cleanup can extend the elapsed time beyond it. Automatic recovery, daemon or host failure and ambiguous creation remain unresolved. The application uses this helper for separate synthetic container replay, but not for its authoritative shadow or promotion path; it is not a sandbox authorization.

To run a fixed built-in replay through the application on Unix, from `runtime-spike/` build the binaries and use an existing synthetic database and journal directory:

```sh
cargo build --locked --offline --bins
target/debug/sese-outcome event-builtin-register DB throttle
target/debug/sese-outcome event-container-replay DB throttle fixtures/event-synthetic-v1.json sha256:IMAGE_DIGEST unix:///PATH/TO/docker.sock JOURNAL /ABS/PATH/python3 /ABS/PATH/docker /ABS/PATH/synthetic_container_worker.py 40
target/debug/sese-outcome event-container-replay-inspect DB CONTAINER_EVENT_SEQ
```

The number is a per-case evaluation deadline in seconds, from 1 to 120. Optionally append a trusted 64-character lowercase worker SHA-256 after it. Before starting each container, the helper copies `/event-worker` as a bounded archive, requires a single regular file, and compares its bytes to that pin. Supply the pin from a separately trusted source: a digest obtained from the same untrusted Docker daemon does not authenticate the worker. The check depends on the daemon honestly returning the file and not changing it before start. Pinned runs use a v5 receipt recording the expected digest and whether the local check matched; older v4 receipts have no pin information. The receipt is a local assertion, not proof of the worker's origin or Docker's honesty, and does not authorize unclassified candidates or promotion. For `event-container-evaluate`, `event-container-observation`, and `event-container-shadow`, append `--worker-sha256 DIGEST`; when using a prepared review with `event-container-evaluate`, put its path before that flag. One pin applies to every worker execution in the operation. The helper caps its shared evaluation deadline at 60 seconds. The Rust supervisor allows an additional 30 seconds for cleanup; a stalled process, daemon or filesystem can still leave cleanup unconfirmed. A bridge failure may be reported only as `worker failed`, and its receipt does not identify the cause. The command records case inputs, expected and actual verdicts, errors, timestamps and successful lifecycle receipts in a separate ledger event. Inspection rechecks candidate and fixture/result consistency, reconstructs each captured receipt's request and output digests, and rejects duplicate container identities or changed reported daemon IDs. Older inconsistent records remain stored but fail inspection; historical receipt file permissions cannot be reconstructed from captured bytes. The returned `container_event_seq` is a ledger sequence, not an ordinary replay ID. Do not pass it to `event-pin`: a coincidentally equal ordinary replay ID could select a different record. These events are synthetic, execution is unauthenticated, and they do not authorize promotion. A timeout can leave a pending container: inspect the reported receipt and resolve cleanup explicitly. Use the repository bridge and trusted local interpreter and Docker CLI: they execute on the host. Their digests, the image and the daemon are not authenticated.

The experimental container shadow command requires a domain with a pinned `measured-v2` fixture and initialized built-in champion, plus a registered built-in shadow. With an existing private journal directory:

```sh
target/debug/sese-outcome event-container-shadow DB DOMAIN STREAM 128 allow throttle 100 42 3.5 0 sha256:IMAGE_DIGEST unix:///PATH/TO/docker.sock JOURNAL /ABS/PATH/python3 /ABS/PATH/docker /ABS/PATH/synthetic_container_worker.py 40
target/debug/sese-outcome event-container-shadow-inspect DB CONTAINER_EVENT_SEQ
```

Arguments after the built-in names are timestamp in nanoseconds, entity ID, metric and flags. The command uses one host-owned prior measured window for both separate containers and records comparisons in a distinct ledger stream keyed by domain and stream; use the same capacity on subsequent calls. It checks the champion and ledger head before committing. A failed champion attempt records the host-measured duration and error code 500 and does not start the shadow. A failed shadow attempt retains the successful champion receipt and records the champion's successful state update. Both return an error containing the failure event sequence and available receipt paths; inspect those paths because cleanup can be unresolved. Invalid input and a changed ledger do not advance state. These events are not ordinary observations, reviews or promotion evidence; never substitute their ledger sequence for an observation ID. Receipt and state checks establish consistency, not container, image or daemon authenticity. Authoritative review and promotion still use the local-worker path.

To compare a later verified measured-v2 observation without independently recreating its evolving window, run:

```sh
target/debug/sese-outcome event-container-observation DB DOMAIN APP_OBSERVATION_ID sha256:IMAGE_DIGEST unix:///PATH/TO/docker.sock JOURNAL /ABS/PATH/python3 /ABS/PATH/docker /ABS/PATH/synthetic_container_worker.py 40
target/debug/sese-outcome event-container-observation-inspect DB CONTAINER_EVENT_SEQ
```

The command uses that observation's exact prior window and event for two fixed built-ins and retains a separate experiment, guarded by the original ledger checkpoint and current champion generation. The source observation must belong to the current champion tenure. A failed attempt returns a failure event sequence and available receipt paths; a shadow failure also retains the successful champion receipt. A changed ledger or helper identity prevents the failure append. A failure record describes the host's attempt, not proof that a container started or was cleaned up. Neither successful nor failed experiments change the app stream or authorize promotion. Use a successful experiment's `container_event_seq` as the final argument to the advisory review preview below; it is not an application observation ID.

To run the experimental container replay and the matching observation comparison in one command, use an existing measured-v2 application replay and observation and the same fixture file:

```sh
target/debug/sese-outcome event-container-evaluate DB DOMAIN APP_REPLAY_ID APP_OBSERVATION_ID FIXTURE.json sha256:IMAGE_DIGEST unix:///PATH/TO/docker.sock JOURNAL /ABS/PATH/python3 /ABS/PATH/docker /ABS/PATH/synthetic_container_worker.py 40
```

The command checks the fixture and observation against the local review before launch, runs the fixed synthetic candidate's fixture and both sides of the source observation, and returns the compared preview with its current checkpoint. A failed container replay or comparison reports its recorded sequence where available; an interrupted or unsuccessful operation can leave experimental records or a pending cleanup receipt. This is an advisory comparison, not a decision or promotion gate. Generated and arbitrary artifacts remain unsupported.

For a later review in an existing synthetic domain, prepare the current replay and latest matching champion/shadow observation without writing a decision:

```sh
runtime-spike/target/debug/sese-outcome event-review-prepare cycle.db typed-cycle REPLAY_ID > prepared.json
runtime-spike/target/debug/sese-outcome event-review-prepared cycle.db prepared.json local-operator approve
```

The first command accepts an optional synthetic review-card JSON file after `REPLAY_ID`; otherwise it uses a neutral default card. Inspect the prepared evidence before choosing `approve` or `reject`. A changed ledger makes the preparation stale; rerun it. The preview shows the latest 100 domain observations and marks it when earlier observations are omitted; use `event-observations` to inspect older pages. This workflow uses the existing synthetic replay and does not enable arbitrary candidate execution.

If you started with `--prepare-only`, append the prepared JSON file path after the timeout and save the command's output. The command checks that the prepared review was current and refreshes it only if the two container events are the only intervening ledger changes and the review contents are unchanged. Pass the saved comparison JSON directly to `event-review-prepared DB comparison.json OPERATOR approve|reject`. The decision still uses the ordinary synthetic review evidence; the container comparison remains advisory. A changed review or any unrelated intervening ledger event rejects the refresh.

To compare stored experimental container results with a local review, use the distinct IDs printed by the corresponding commands:

```sh
target/debug/sese-outcome event-container-review-preview DB DOMAIN APP_REPLAY_ID APP_OBSERVATION_ID CONTAINER_REPLAY_SEQ CONTAINER_COMPARISON_SEQ
```

This read-only preview accepts either a container shadow stream event or a container observation experiment as its final sequence. It checks the domain's pinned fixture, candidate, current champion generation and tenure, event, prior measured state, verdicts, receipt bindings and distinct container identities in one database snapshot. Prior state must match exactly, including measured p99; independently advanced streams commonly differ. It returns the ordinary review preview, the two experimental records and the snapshot checkpoint. `container_comparison_kind` identifies which experiment was selected; `container_shadow_seq` remains a compatibility alias for its sequence. A mismatch returns an error, and the checkpoint can become stale after reading. `matched` means local results are consistent, not that execution or the daemon is authenticated. `worker_pin_summary` separately reports whether the retained receipts are all unpinned, consistently pinned, partially pinned, or use different pins; even a consistent pin does not verify its origin. The result does not authorize a review or promotion; use the ordinary review workflow for a decision.

The Python API `inspect_pending(path)` reports `not_created` for a valid preflight record as read; an active run may advance afterward. For a pending record it validates the owner-only record and queries the recorded socket read-only. It reports `observed` only when the reported daemon ID, container name, image and probe label match; legacy records without an explicit socket and other unverifiable results are `unknown`. An `unknown` result is not proof that no container exists. The endpoint and daemon ID are consistency checks, not authentication; a socket may be replaced or forwarded. Inspection does not delete or stop anything or authorize a cleanup action.

Cooperating uses of `run_synthetic` serialize on the journal directory and refuse a new launch if an existing synthetic receipt is pending, malformed, unsafe to read, or not a valid v4 or v5 removed record. Lock acquisition has its own deadline. This guard applies only to callers using the same unchanged directory and helper; another journal or a non-cooperating process bypasses it. It does not authenticate receipts or Docker. When the Rust bridge is denied before its own intent is written, the receipt path in its error can be absent; inspect the existing journal records rather than treating that path as proof of a container.

An operator can inspect a pending v4 or v5 receipt and, only after confirming the exact container is theirs to remove, explicitly request recovery:

```sh
python3 scripts/synthetic_container_recover.py RECEIPT --inspect-only --docker /ABS/PATH/docker
python3 scripts/synthetic_container_recover.py RECEIPT --remove-observed-container --container-id FULL_CONTAINER_ID --daemon-id DAEMON_ID --docker-host unix:///PATH/TO/docker.sock --image sha256:IMAGE_DIGEST --receipt-sha256 RECEIPT_DIGEST --actor OPERATOR --docker /ABS/PATH/docker
```

The second command **force-removes the selected container**. Copy all identities from a current `observed` inspection and verify them independently before running it. It rechecks those values, records an owner-only recovery intent, removes that exact ID, checks the reported daemon and absence, marks the original receipt removed with a failed result, and writes a completion bound to the final receipt digest. An `unknown` observation, changed identity or failed removal does not mark the receipt removed. New v2 recovery intents without a matching completion block the journal even if a receipt appears removed; a later completed recovery for the same identities resolves earlier attempts. The guard retries a directory sync before accepting visible records. Historical v1 recovery audits predate this completion binding and are not backfilled. An interrupted recovery or write failure still requires manual investigation; these filesystem checks do not prove power-loss durability. A recovery audit is a local caller assertion, not authentication, proof against a dishonest daemon, or promotion evidence. No recovery runs automatically.

For a complete synthetic typed-event walkthrough from build through inspection, outcome and checkpoint verification, see [ONBOARDING.md](ONBOARDING.md).

New local synthetic event replay inspections include `case_timings`: each executed case has wall-clock Unix-nanosecond start and finish values and a monotonic elapsed-nanosecond value. After a worker error, later cases are recorded as skipped with null timings; a verdict mismatch alone does not stop later cases. The timing event is written with the replay and linked to its ledger hash; `--verify-event-replays` independently checks the timing record's case IDs, parent link and skip ordering. Older runs without timing report `case_timings: null`; v1 timing records retain their original interpretation. These host measurements include worker supervision, are not benchmarks, and do not authenticate execution or establish a sandbox.

For a step-by-step cycle using separate operator commands and measured-v2 stream state, see [MANUAL_CYCLE.md](MANUAL_CYCLE.md).

For a shorter synthetic workflow, build the binaries and prepare replay and shadow evidence without an interactive prompt:

```sh
cargo build --locked --manifest-path runtime-spike/Cargo.toml --bins
runtime-spike/target/debug/sese-event-cycle-demo runtime-spike/target/debug/event-worker cycle.db --measured --prepare-only > prepared.json
python3 -m json.tool prepared.json
runtime-spike/target/debug/sese-outcome event-review-prepared cycle.db prepared.json local-operator approve
```

Use new paths for the database and output. Inspect the JSON before deciding; use `reject` instead of `approve` to leave the champion unchanged. Approval records the review and promotes in one database transaction; if promotion fails, neither change is committed. The command checks that the complete saved preview and checkpoint still match current evidence; regenerate the evaluation after a mismatch. Preparation and approval are synthetic only; they do not authorize untrusted artifacts or establish real-world outcomes.

From the repository root:

```sh
cargo build --locked --manifest-path runtime-spike/Cargo.toml --bins
cargo run --locked --manifest-path runtime-spike/Cargo.toml --bin sese-demo -- runtime-spike/target/debug/worker
cargo test --locked --manifest-path runtime-spike/Cargo.toml
```

Enter `approve` or `reject` at the prompt. Either choice records a synthetic review decision; neither changes a champion. The default database is in memory and disappears when the process exits. Pass a second argument such as `demo.db` to retain the SQLite records locally; inspect it with SQLite tooling. New structured reviews and their hash-chain entries are written in one transaction. Verification compares review rows and a digest of their replay and shadow evidence with ledger events. Reviews created before evidence digesting lack that binding and cause verification to fail; no backfill is performed. Use a fresh demo database for new runs. The chain detects some modifications but has no signature or external anchor. Measurements include process startup and are not performance benchmarks.

On macOS the supervisor uses `posix_spawn` with `POSIX_SPAWN_CLOEXEC_DEFAULT` and passes only explicitly configured standard streams; its worker path is used directly without a PATH search. On Linux the launcher marks descriptors numbered 3 and above close-on-exec in the child using `close_range`; it refuses to spawn a worker if that operation is unavailable or denied. This requires kernel and security-policy support for `CLOSE_RANGE_CLOEXEC`. Both use bounded nonblocking I/O (half-closed socket pairs on macOS, pipes on Linux), start the worker in a separate process group, and attempt to kill that group on completion or timeout while retaining the leader PID until cleanup. I/O handles close when supervision returns, even if an escaped descendant retains its end. Cleanup is best effort: descendants can escape the group and permissions can prevent termination. Other platforms retain deadline-only supervision and use I/O threads. None of these mechanisms provides filesystem or network isolation or authorizes untrusted code.

On macOS and Linux, the three bundled synthetic worker binaries set a hard CPU-time limit of at most 10 seconds before reading a request; they preserve any stricter inherited limits and exit if setup fails. This is per-process CPU time, not the supervisor's wall-clock deadline or a memory limit. The supervisor can run a caller-supplied executable, so it cannot assume that executable applies this limit. The Linux path has not been tested here. These controls do not establish an OS sandbox.

The Rust API can export resolved synthetic outcome evidence as JSONL. Pending records are excluded and ledger verification must pass. These inspection rows are explicitly `training_ready: false`. A separate conditional synthetic training export uses signed traces, outcomes and, for DPO, explicit preference pairs.

For a persistent demo database, outcome status and synthetic resolution can be inspected via:

```sh
cargo run --locked --manifest-path runtime-spike/Cargo.toml --bin sese-outcome -- status demo.db 1
cargo run --locked --manifest-path runtime-spike/Cargo.toml --bin sese-outcome -- resolve demo.db 1 falsified auditor 'synthetic observation'
cargo run --quiet --locked --manifest-path runtime-spike/Cargo.toml --bin sese-outcome -- export demo.db
```

The review ID is printed by the demo. Resolution is an explicit user assertion, not independently verified reality. The command opens existing databases only.

The experimental host keeps a bounded in-memory scalar event window. The current Wasm ABI passes only the current integer to guests, not the window, and the window is not restored after restart.

The experimental `evaluate_window_v1` accepts the previous window count and integer sum plus the current event. Separate supervised worker, host and replay paths exercise it and persist prior windows; the original demo uses the scalar ABI. Window replay and shadow records can be linked to an experimental structured review and synthetic promotion. Neither worker provides sufficient containment for untrusted modules.

Window reviews use a separate ID space. Use `window-status`, `window-resolve`, and `window-export` in place of the scalar commands for those records. Window JSONL exports are synthetic evidence and also marked `training_ready: false`.

To exercise the synthetic window review and retain its records:

```sh
cargo run --locked --manifest-path runtime-spike/Cargo.toml --bin sese-window-demo -- runtime-spike/target/debug/window-worker window-demo.db
```

The printed window review ID can be used with the `window-*` outcome commands. This preview does not promote candidates, and its synthetic fixtures do not prove real-world value or worker isolation.

Inspect a persisted window review and its replay cases and shadow snapshot as JSON:

```sh
cargo run --quiet --locked --manifest-path runtime-spike/Cargo.toml --bin sese-outcome -- window-inspect window-demo.db 1
```

Inspection checks ledger consistency in the same database snapshot and fails if bound evidence is altered. It does not establish an external trust anchor or authenticity.

The experimental Rust `DurableHost` API stores a named window and its observation atomically, using a generation check to reject stale writers. The scalar and window previews use the in-memory host; the fixed cycle demo uses `DurableHost`. Persistent state is not externally anchored or validated against direct database changes.

New window preview reviews retain the displayed three-column card with a ledger event. Older API-created reviews may have no card. New card-bearing review events require a matching card and card event; older review events do not. Without an external anchor, complete chain rewriting or removal remains undetectable.

An operator can print a ledger checkpoint with `sese-outcome checkpoint DB` and retain its sequence and hash independently of the database. Later, `sese-outcome verify-checkpoint DB SEQ HASH` verifies the current ledger and checks that exact historical event. A valid extension remains valid. This detects changes or deletion at or before the retained checkpoint, but not deletion of later events, forgery of the checkpoint itself, or lack of independent custody. It is not a signature.

An operator can explicitly select a synthetic fixture using `sese-outcome window-pin DB DOMAIN VERSION DIGEST ACTOR`, then check a replay using `sese-outcome window-pin-match DB DOMAIN RUN_ID`. Obtain the version and digest from reviewed replay evidence; pinning records a selection, not proof that the fixture is adequate or authorized for production. Pins cannot be replaced through this CLI. A matching pin is one prerequisite for synthetic promotion.

Synthetic champion metadata can be controlled explicitly after inspecting and pinning the fixture and recording a card-bearing approval:

```text
sese-outcome window-bootstrap DB DOMAIN INITIAL_CHAMPION_ID ACTOR
sese-outcome window-champion DB DOMAIN
sese-outcome window-promote DB DOMAIN EXPECTED_CHAMPION_ID REVIEW_ID ACTOR
sese-outcome window-rollback DB DOMAIN EXPECTED_CHAMPION_ID ACTOR
```

These commands operate on local synthetic state; the actor string is not authenticated. Promotion checks the selected fixture and evidence, but neither the CLI nor a passing test establishes containment, real-world fixture quality, or production readiness.

File-backed storage requests SQLite WAL; in-memory storage uses SQLite's memory journal. One local test verifies recovery from an uncommitted update when a child process exits abruptly. It does not establish power-loss durability or behavior on network filesystems.

`window-register DB ARTIFACT_PATH` reads a bounded regular file and stores its bytes under a SHA-256 identity with window-ABI metadata. Registration does not parse, validate, execute, or approve the artifact. Do not execute untrusted candidates: the worker is not an OS security boundary.

To exercise the fixed synthetic lifecycle, first build all binaries and then run:

```sh
cargo run --locked --manifest-path runtime-spike/Cargo.toml --bin sese-cycle-demo -- runtime-spike/target/debug/window-worker cycle-new.db
```

The database path must not already exist; omit it for an in-memory run. The demo selects its fixed synthetic fixture, asks for an explicit approval or rejection, and, after approval, asks whether to roll back or keep the synthetic champion. Outcomes remain pending until explicitly recorded. Use `sese-outcome window-inspect cycle-new.db 1` to inspect the review. This is a fixed demonstration and does not establish isolation or suitability for untrusted code.

The interpreter applies Wasmi 0.40.0 strict parser/compilation limits in addition to its 65,536-byte module cap, fuel and store limits. A locally patched Wasmi copy also presents unrepresentable memory-growth requests to the limiter; see `vendor/wasmi/LOCAL-PATCH.md`. The configured limiter traps the tested denied memory and table growth requests instead of returning Wasm’s usual `-1`, an intentional compatibility restriction. These limits can reject otherwise valid modules; they do not bound all host-process resources or provide OS isolation. The experimental `event_abi` module accepts typed window and event fields and validates verdicts. It is a flattened core-Wasm experiment, not WIT Component Model compatibility. A synthetic host and supervised worker connect it to replay, review, and promotion. Do not use it for untrusted modules.

For a separate local synthetic replay, build the binaries and initialize a new database, then register a built-in module and run a fixture:

```sh
runtime-spike/target/debug/sese-outcome event-init synthetic.db
runtime-spike/target/debug/sese-outcome event-builtin-register synthetic.db throttle
runtime-spike/target/debug/sese-outcome event-builtin-replay synthetic.db throttle runtime-spike/fixtures/event-synthetic-v1.json
runtime-spike/target/debug/sese-outcome event-replay-inspect synthetic.db 1
```

`event-init` refuses existing paths. Built-in names are `allow`, `throttle`, `quarantine`, and `window-probe`; registration does not execute them. The fixed `window-probe` returns an allow verdict with score `prior.mean + prior.count + prior.p99_us` and reason `prior.last_error`, to check that prior state reaches the guest. Replay accepts a name rather than arbitrary code, checks the registered artifact and uses the `event-worker` binary beside `sese-outcome`. Use a trusted local installation of both binaries. It records real case results and prints the run ID and pass status; a failed replay returns a nonzero exit code after printing its result. These commands do not create an observation, approve a candidate or provide a sandbox.

`event-pin` requires the explicit `replay:ID` form and a passed, verified ordinary replay. Bare numbers are rejected to make accidental use of a container ledger sequence less likely. The prefix is an operator assertion: relabeling a container sequence as a replay ID can still select a different replay with the same number. Bootstrap also rechecks that the selected replay passed, including for older databases whose failed selections remain available for inspection. A passing replay does not prove fixture quality or authorize untrusted execution.

To produce a shadow observation separately, register the `allow` built-in, pin the successful replay for a domain, and bootstrap `allow` as champion using its returned candidate ID. Then run:

```text
sese-outcome event-builtin-observe DB DOMAIN STREAM EXPECTED_CHAMPION SHADOW STATE_SEMANTICS TIMESTAMP_NS ENTITY_ID METRIC FLAGS
```

For example, after pinning and bootstrapping, use `sese-outcome event-builtin-observe synthetic.db demo stream allow throttle legacy-v1 1 42 3 4`. Both candidate names must be registered built-ins; the expected champion and selected semantics must match the domain. Each invocation handles one synthetic event, resumes its durable stream with capacity 32, prints its observation ID and results, and uses the sibling worker. The legacy path supplies `last_error=0`; `measured-v2` retains measured state. Worker failures are recorded and return a nonzero status; a stream may have been initialized before a later failure. Use `event-observations` and `event-review-preview` to inspect the evidence before an explicit decision.

Typed-event synthetic reviews use a separate ID space. Given an existing ledger-bound typed replay and observation, record and inspect an explicit decision:

```text
sese-outcome event-review DB CANDIDATE_ID REPLAY_ID OBSERVATION_ID ACTOR approve|reject HUMAN BRIDGE PROPOSAL
sese-outcome event-inspect DB REVIEW_ID
```

Quote card arguments containing spaces. Inspection includes ordered replay cases and both observation results. Approval requires a passed replay and complete results for the matching shadow candidate. These experimental commands do not promote a candidate, authenticate the actor, validate a fixture, or provide a sandbox. The `EventHost::process_domain` Rust API uses the selected synthetic champion and rejects a concurrent champion transition before committing its observation.

Typed-event outcome commands operate on the typed-event review ID:

```text
sese-outcome event-status DB REVIEW_ID
sese-outcome event-resolve DB REVIEW_ID validated|falsified ACTOR SOURCE
sese-outcome event-export DB
```

Resolution is a caller assertion about the decision, not independent validation. Typed replay inspection and outcome export report `runtime_provenance_verified: false`: recorded worker hashes do not establish the runtime's origin or trustworthiness. Prepared reviews created before this field was added must be regenerated. Export excludes pending reviews, verifies linked evidence, and labels records synthetic and `training_ready: false`. It does not assign a reward or generate SFT/DPO/RLVR training examples.

Resolved typed-event export rows include `synthetic_followup_receipts` for that review, or an empty array when none exist. Each included comparison is recomputed in the export's verified snapshot; invalid evidence or more than 1,000 receipts for a review causes export to fail. These synthetic comparisons are separate from the caller-asserted outcome and do not validate it. Pending reviews remain excluded.

`sese-outcome event-followup-check DB REVIEW_ID OBSERVATION_ID CASE_ID` compares a recorded post-promotion synthetic champion observation with a case from the fixture reviewed before that decision. It requires the matching reviewed candidate, domain, worker digest, and active promotion; exact event and prior-window values must match the predeclared case. The result distinguishes `match`, `verdict_mismatch`, `input_mismatch`, and `worker_error`. This is a repeat comparison against the same synthetic fixture, not an independent fixture or a real-world outcome. It does not record an outcome or authorize training.

For a separate preselected synthetic fixture, run the demo with its default event and add `--followup-fixture runtime-spike/fixtures/event-followup-synthetic-v1.json --followup-case post-promotion`. The demo executes and displays that fixture before review, records the selection, and reruns the reviewed fixture to retain the existing replay-freshness gate. After approval, inspect the post-promotion observation with `sese-outcome event-followup-selected-check DB REVIEW_ID OBSERVATION_ID`. The selected fixture must target the same candidate and state semantics, use the same recorded worker digest, have a different fixture digest, and select an input absent from the reviewed fixture. A selection recorded after the decision cannot support that decision's follow-up check. The lower-level `event-followup-pin DB DOMAIN REPLAY_ID CASE_ID ACTOR` selects a previously executed synthetic replay.

The selected follow-up replay must pass; promotion also rechecks existing selections so a previously recorded failed follow-up cannot authorize promotion. When a follow-up is selected, its ledger event must precede the approval used for promotion; if it was selected later, record a new review after inspecting it. Both fixtures and their results are visible before the decision and are caller-selected; this is not a blinded holdout, independent fixture authority, or a real-world outcome. The check records no outcome and does not establish training eligibility.

Use `sese-outcome event-followup-record DB REVIEW_ID OBSERVATION_ID` to persist a selected-case comparison as a synthetic receipt, then `sese-outcome event-followup-inspect DB RECEIPT_ID` to recompute and inspect it. The receipt and its review, observation and selection links are ledger-bound; recording the same comparison twice is rejected. A receipt is not an outcome record, an independently validated label, or training-ready data.

After resolving the outcome, `sese-outcome event-followup-outcome-check DB RECEIPT_ID CHECKPOINT_SEQ CHECKPOINT_HASH` recomputes the receipt and compares a post-receipt assertion with the synthetic result: `match` corresponds to `validated`, and `verdict_mismatch` to `falsified`. The output reports whether they agree; input mismatches and worker errors cannot support a status. The supplied checkpoint must cover the outcome. Agreement does not prove the operator used the receipt, fixture independence, checkpoint custody, real-world truth or training eligibility.

`sese-outcome event-signed-followup-check DB RECEIPT_ID BUNDLE_JSON AUTHORITIES_JSON AUTHORITIES_SHA256 CHECKPOINT_SEQ CHECKPOINT_HASH` additionally verifies signed trace and observer claims against that receipt. The outcome claim must use method `synthetic-followup-receipt-v1` and evidence reference `sese-followup:RECEIPT_ID:RECEIPT_LEDGER_HASH`; its signed timestamp must follow receipt creation. It checks the recorded outcome and recomputes the comparison in the same snapshot. This does not retain the bundle or establish authority custody, independence, real-world truth or training eligibility.

Replace `event-signed-followup-check` with `event-signed-followup-record` to atomically retain the checked canonical claims and signatures in an attestation receipt. To recheck later, use `event-signed-followup-recheck DB ATTESTATION_RECEIPT_ID FOLLOWUP_RECEIPT_ID BUNDLE_JSON AUTHORITIES_JSON AUTHORITIES_SHA256 CHECKPOINT_SEQ CHECKPOINT_HASH`; retain the original bundle and supply a checkpoint covering the attestation receipt. Recheck compares the bundle with stored bytes and recomputes the signed follow-up link. A generic attestation receipt alone does not prove that this follow-up check was performed.

Alternatively, `event-retained-followup-check DB ATTESTATION_RECEIPT_ID FOLLOWUP_RECEIPT_ID AUTHORITIES_JSON AUTHORITIES_SHA256 CHECKPOINT_SEQ CHECKPOINT_HASH` decodes and rechecks the retained canonical claims without the original bundle. The authority configuration and covering checkpoint still require independent retention. This inspection does not establish trust, independence or training eligibility.

Follow-up check and receipt JSON use v2 schemas and the field `records_outcome: false`; this describes what the comparison operation does, rather than claiming that the review has no separately recorded outcome. Consumers of the former v1 `outcome_recorded` field must update.

To retain a caller-asserted synthetic prompt and response, optionally with an explicitly supplied preferred/rejected pair, use:

```text
sese-outcome event-trace-record DB REVIEW_ID runtime-spike/fixtures/training-trace-synthetic-v1.json
sese-outcome event-trace-inspect DB TRACE_ID
```

The trace and its review link are ledger-bound and stored locally. The example shows the required actor and source fields; omit `preference` when no pair was recorded. The system does not infer a pair or execute a model. All fields, including the claim that the data is synthetic, are caller assertions: neither a validated outcome label nor a recorded pair makes the trace training-ready. Do not put sensitive data or credentials in trace files or the database.

If a synthetic trace's response is WebAssembly text, `sese-outcome event-trace-compare-wat DB TRACE_ID` parses it without executing it and compares the resulting binary SHA-256 with the reviewed candidate. Results distinguish `match`, `mismatch`, `invalid_wat`, and oversized output. A match is byte identity under the locked WAT parser, not proof of model authorship, semantic equivalence, safe execution, outcome truth, or training eligibility. Parsing itself is not an OS sandbox; use only known synthetic text.

`sese-outcome event-trace-eligibility DB TRACE_ID` explains the current separate SFT, DPO and RLVR blockers in a ledger-verified snapshot. All current trace records are ineligible: their origin and any resolved outcome are caller assertions. DPO additionally requires an explicit pair whose comparability is verified; typed-event RLVR also lacks a recorded reward derivation. SFT and DPO require a validated outcome, so a falsified outcome is separately flagged. This audit does not establish independently verified eligibility; the separate conditional exporter requires additional checks and explicit operator assumptions.

The Rust `attestation` module provides strict Ed25519 checks, bounded digest-pinned authority sets and canonical synthetic trace and outcome payloads bound to review, replay, candidate, fixture and ledger references. The formats are specified in [training-export-v1.md](../contracts/training-export-v1.md). The experimental `Storage::check_synthetic_attestation_links_in_set` resolves named authorities and matches signed claims to checkpointed records in one snapshot. It accepts the authority configuration digest and checkpoint from its caller; it does not establish their custody, verify observer independence or the observation evidence reference, authenticate outcome truth, or change export eligibility. Key custody, authenticated evidence import and revocation management remain unimplemented.

To check an existing synthetic signed bundle without storing it, run `sese-outcome event-attestation-check DB BUNDLE_JSON AUTHORITIES_JSON AUTHORITIES_SHA256 CHECKPOINT_SEQ CHECKPOINT_HASH`. Supply a separately protected authority digest and checkpoint; computing either from the same mutable files at inspection time does not establish independent custody. The command reads bounded regular files, verifies the supplied signatures and referenced records, and reports `training_ready: false`. It neither generates signatures nor checks the truth of the underlying assertions. See the contract for the required JSON fields and binary signing format; use synthetic data only.

Use `event-attestation-record` with the same arguments to atomically retain the canonical synthetic payloads, signatures and their ledger-bound receipt. `event-attestation-inspect DB RECEIPT_ID` verifies ledger bindings but reports `signature_rechecked: false`; `event-attestation-recheck DB RECEIPT_ID AUTHORITIES_JSON AUTHORITIES_SHA256` rechecks signatures against the same configuration digest. A changed configuration, including a revocation update, fails this recheck and requires separate review. Neither command proves authority custody, independence or outcome truth, and neither marks evidence training-ready.

To check a separately signed synthetic preference against the recorded explicit pair, use `sese-outcome event-preference-check DB PREFERENCE_JSON AUTHORITIES_JSON AUTHORITIES_SHA256 CHECKPOINT_SEQ CHECKPOINT_HASH`. It requires a configured preference-reviewer key and reports both `comparability_verified: false` and `training_ready: false`. A signed comparability explanation is an assertion; it does not establish that the pair is suitable for training.

Use `event-preference-record` with the same arguments to retain the signed synthetic claim and its ledger-linked receipt. `event-preference-inspect DB RECEIPT_ID` checks ledger bindings without rechecking the signature; `event-preference-recheck DB RECEIPT_ID AUTHORITIES_JSON AUTHORITIES_SHA256` rechecks the signature using the original pinned configuration. A changed configuration, including a revocation update, fails the digest match and requires separate review. The receipt does not establish comparability, reviewer independence or DPO eligibility.

Use `event-evidence-check DB ATTESTATION_RECEIPT_ID PREFERENCE_RECEIPT_ID AUTHORITIES_JSON AUTHORITIES_SHA256 CHECKPOINT_SEQ CHECKPOINT_HASH` to check the two retained receipts together in one verified snapshot. The supplied checkpoint must cover both receipt events; both must reference the same trace and authority configuration, and all three signatures are rechecked. The result remains synthetic and `training_ready: false`; it reports the caller-asserted outcome and does not establish authority custody, independence, comparability or outcome truth.

Use `event-evidence-policy-check DB ATTESTATION_RECEIPT_ID PREFERENCE_RECEIPT_ID AUTHORITIES_JSON AUTHORITIES_SHA256 POLICY_JSON POLICY_SHA256 CHECKPOINT_SEQ CHECKPOINT_HASH` to also check those signers against a separately pinned operator declaration. The JSON format and restrictions are in [training-export-v1.md](../contracts/training-export-v1.md). Matching a declaration is not proof of independence, policy custody or eligibility; this remains an inspection check and returns `training_ready: false`.

For trace and outcome evidence without a preference receipt, use `event-common-policy-check DB ATTESTATION_RECEIPT_ID AUTHORITIES_JSON AUTHORITIES_SHA256 POLICY_JSON POLICY_SHA256 CHECKPOINT_SEQ CHECKPOINT_HASH`. It verifies the ledger, receipt coverage, trace and outcome signatures and the corresponding source/observer declaration in one snapshot. The result remains synthetic and `training_ready: false`; it does not establish independent observation, policy or checkpoint custody, or outcome truth.

`event-reward-preview DB ATTESTATION_RECEIPT_ID AUTHORITIES_JSON AUTHORITIES_SHA256 POLICY_JSON POLICY_SHA256 RULE_JSON RULE_SHA256 CHECKPOINT_SEQ CHECKPOINT_HASH` applies a pinned synthetic rule to that checked, caller-asserted outcome. The example rule is `fixtures/synthetic-reward-rule-v1.json`; preserve its exact bytes separately with the supplied SHA-256 pin. The preview carries the rule version, digest, declared actor and source and returns `training_ready: false`. It does not record the rule in the database, establish rule custody, validate the outcome, or export an RLVR training row.

Use `event-evidence-export` with the same arguments to emit one JSONL inspection record containing the candidate and fixture, decision, caller-asserted trace and outcome, explicit preference pair, and canonical signed payloads and signatures. The exporter checks the same snapshot and emits nothing on a failed check. It is an inspection artifact, not an SFT, DPO or RLVR training export; it includes only resolved records with both matching receipts and remains `training_ready: false`. Keep it local and synthetic.

To compare an exported record with its source, use `event-evidence-verify DB EXPORT_JSONL AUTHORITIES_JSON AUTHORITIES_SHA256 CHECKPOINT_SEQ CHECKPOINT_HASH`. It reloads the referenced receipts, rechecks signatures in a verified database snapshot and compares the exact exported bytes. Preserve the original line and newline: reformatting also causes a mismatch. This check requires the database and original pinned authority set; it does not independently authenticate their custody or prove the assertions true. On macOS and Linux these bounded-file inputs are opened nonblocking before regular-file validation; other platforms retain the standard file-open behavior.

An operator can select an existing, ledger-bound typed-event replay as a domain's fixture and compare later runs against its exact version and digest:

```text
sese-outcome event-pin DB DOMAIN replay:REPLAY_ID ACTOR
sese-outcome event-pin-match DB DOMAIN REPLAY_ID
```

The selection cannot be replaced through these commands. It records a local choice, not independent fixture authority, and synthetic typed-event promotion requires a match.

Control typed-event synthetic champion metadata with explicit expected-current IDs:

```text
sese-outcome event-bootstrap DB DOMAIN CHAMPION_ID ACTOR
sese-outcome event-champion DB DOMAIN
sese-outcome event-promote DB DOMAIN EXPECTED_CHAMPION_ID REVIEW_ID ACTOR
sese-outcome event-rollback DB DOMAIN EXPECTED_CHAMPION_ID ACTOR
```

Promotion requires a selected fixture, passed matching replay, complete matching shadow observation and explicit approval. Evidence freshness is currently based on latest IDs, not elapsed time. These commands update metadata; `EventHost::process_domain` reads that metadata when processing. They do not provide containment.

Run the complete fixed synthetic typed-event cycle with a new local database:

```sh
cargo build --locked --manifest-path runtime-spike/Cargo.toml --bins
runtime-spike/target/debug/sese-event-cycle-demo runtime-spike/target/debug/event-worker typed-demo.db
```

The demo refuses an existing database. Before the decision prompt it prints each replay case's inputs, expected and actual verdicts, errors and pass status, followed by the synthetic champion/shadow comparisons. Failed replay cases are printed before the demo stops. Review the evidence, enter `approve` or `reject`, and, if approved, choose `rollback` or `keep`. The printed review ID can be inspected and resolved using `event-inspect` and `event-resolve` above. This exercises built-in synthetic modules only; neither the demo nor its worker is a containment boundary.

Choose the built-in candidate with `--candidate throttle` (default) or `--candidate quarantine`. For example, after building, run `runtime-spike/target/debug/sese-event-cycle-demo runtime-spike/target/debug/event-worker quarantine-demo.db --candidate quarantine`. Without `--fixture`, each candidate uses its corresponding fixed synthetic replay case. A supplied fixture must expect the selected candidate's results: the bundled `event-synthetic-v1.json` expects throttle, so it fails for quarantine. Candidate selection does not accept arbitrary code or provide a sandbox.

Before prompting, the typed-event demo assembles the displayed replay, observations, and ledger head from one verified database snapshot. It refuses to record the decision if that head changes while waiting, even for an unrelated ledger event. The check is performed in the same write transaction as the decision. The card text is supplied separately by the operator. This is a concurrency guard, not a signature or an external checkpoint. The older `event-review` command retains its existing unguarded behavior.

To resume from a retained database without executing a candidate, obtain a fresh preview and use its replay candidate ID, an observation ID, and checkpoint sequence and hash:

```text
sese-outcome event-review-preview DB DOMAIN REPLAY_ID
sese-outcome event-review-at DB DOMAIN CANDIDATE_ID REPLAY_ID OBSERVATION_ID ACTOR approve|reject HUMAN BRIDGE PROPOSAL SEQ HASH
```

Quote card fields containing spaces. The preview contains up to 100 domain observations and fails if there are more. The guarded decision requires the observation to be in that domain and fails if the ledger head has changed. A decision does not itself promote the candidate; use the separate promotion command when its gates are met. The checkpoint is caller-provided and does not prove a human read the preview.

For a retained database, `sese-outcome event-replay-inspect DB RUN_ID` retrieves a replay and its ordered cases even when the replay failed and no review exists. The demo prints the run ID before stopping. Inspection checks the ledger binding, fixture digest and stored result consistency in one snapshot; it does not rerun the guest or establish that expected answers are correct. Worker path and digest are recorded provenance, not attestation.

Inspect a typed-event fixture file without executing a candidate:

```sh
cargo run --locked --manifest-path runtime-spike/Cargo.toml --bin sese-fixture -- runtime-spike/fixtures/event-synthetic-v1.json
```

The example defines the `sese-event-fixture-file-v1` JSON shape. Use `legacy-v1` or `measured-v2` state semantics and decision codes 0–3. The inspection rejects invalid or duplicate fields, invalid cases, and files over 1 MiB; it prints the digest used by typed-event replay. This validates structure, not the quality or authority of expected answers.

To run the fixed synthetic demo guests against a fixture file, pass it after a new database path:

```sh
runtime-spike/target/debug/sese-event-cycle-demo runtime-spike/target/debug/event-worker fixture-demo.db --fixture runtime-spike/fixtures/event-synthetic-v1.json
```

For a `measured-v2` fixture add `--measured`; the demo rejects a mismatch before creating the database. It persists the supplied replay cases and stops before initializing a champion if replay fails. Rust callers can use `FixturePack::replay_synthetic` for known synthetic candidates; the APIs and workers are not a sandbox and must not be used for untrusted candidates.

Optionally add `--events runtime-spike/fixtures/events-synthetic-v1.json` to feed the demo a local synthetic event sequence. The file can contain 1–100 events and must be at most 64 KiB; the example shows the required fields. Events are processed in file order, each comparison is printed and persisted, and the review refers to the last observation. After promotion or rollback, the demo reuses the last event to demonstrate the changed champion. Inputs are caller-provided synthetic data, not a live or production feed; do not supply sensitive data.

Optionally add `--card runtime-spike/fixtures/review-card-synthetic-v1.json` to supply the human, bridge, and proposal text shown before the decision and recorded with it. The file is limited to 64 KiB and each nonempty field to 16 KiB. The text is an operator assertion; it does not change the fixed candidate or prove that the proposal describes its behavior. Approval or rejection remains a separate explicit prompt.

Inspect persisted domain observations in ID order with `sese-outcome event-observations DB typed-cycle 0`. The JSON page includes the input, prior state, champion and shadow results or errors, and measured durations. Pages contain at most 100 rows; if `next_after_id` is non-null, pass that value as the final argument for the next page. Each page is read from a ledger-verified snapshot and rejects unlinked selected observations. An external checkpoint is still needed to detect a full history rewrite or suffix removal, and timings include worker overhead.

Use `sese-outcome event-history DB typed-cycle 0 [LIMIT]` to page through domain reviews, caller-asserted outcomes, and champion bootstrap, promotion and rollback events in ledger sequence. The optional limit is 1–100 (default 100); pass `next_after_seq` as the next cursor when it is non-null. Each page is verified in its own snapshot and includes that snapshot's ledger head, so compare heads when a consistent multi-page view matters. The head is not independently protected and actors and outcomes are not authenticated. Use `event-inspect` for full review evidence.

New durable typed-event streams record ledger-linked state receipts from creation onward. Verification checks the receipt sequence, linked observations, and current stored window. Streams created before schema v25 have no receipts and cannot be reopened by the durable host; their data is preserved and no recovery or backfill command is currently provided. A protected external checkpoint remains necessary to detect a complete history rewrite or removal of a final suffix.

`event-register DB ARTIFACT_PATH` stores a bounded typed-event artifact and reports `executed: false` and `validated: false`. It also parses the bytes on the host, without instantiating the module, to report whether imports are absent and the event export has the expected signature. An incompatible artifact is still stored for inspection. A compatible structural check is not replay, proof of runtime behavior, or authorization to execute; parser and host security are not established by this check. Both artifact intake commands require a nonempty regular file of at most 65,536 bytes; on macOS and Linux they open nonblocking so a named pipe is rejected without waiting for a writer. The ABI label is the selected intake route; the current worker is not a verified sandbox.

To generate a local candidate from an explicit numeric threshold, run `sese-outcome event-propose-threshold DB 3.5`. This compiles and stores an event-v1 Wasm artifact intended to throttle when the event metric exceeds the threshold, and to allow it otherwise; the score is the metric and the reason is the event flags. The output includes its digest and input threshold. This is a deterministic rule compiler, not an autonomous or model-generated proposal. It does not execute or validate the artifact. Newly stored candidates are unclassified and rejected by the current synthetic replay path; existing identities retain their prior classification. Preserve the returned proposal information separately; it is not yet a ledger-bound provenance record. Candidate execution remains unavailable until a suitable boundary is established.

Alternatively, `sese-outcome event-propose-from-fixture DB runtime-spike/fixtures/threshold-training-synthetic-v1.json` derives a threshold from labeled allow and throttle examples. It rejects overlapping labels, unsupported decisions, or expected scores and reasons that the template cannot represent. It atomically stores the candidate and an event containing the exact input fixture and derived identity; inspect and recompute that link with `sese-outcome event-proposal-inspect DB PROPOSAL_SEQ`. The source fixture is stored in the local database, so use synthetic data. This is caller-supplied provenance, not authenticated authorship; the general ledger verifier checks the chain but does not independently recompute proposal semantics. The reported fit is a static check on the same training cases, not execution or held-out validation. Use separate evaluation cases before drawing conclusions about a proposed rule.

For the separate measured-state synthetic path, add `--measured` after a new database path (or immediately after the worker path for an in-memory run):

```sh
runtime-spike/target/debug/sese-event-cycle-demo runtime-spike/target/debug/event-worker measured-demo.db --measured
```

The demo prints its state semantics. Measured durations include supervisor and process startup overhead and are not performance benchmarks. Measured and legacy streams and replay identities remain separate.

Typed-event promotion requires a domain-scoped observation produced by `process_domain`; an unscoped observation remains inspectable but cannot authorize promotion. Observation domain is ledger-bound, and review inspection and outcome export retain it. Older observations migrate as unscoped rather than receiving a guessed domain.

A typed-event host window is scoped by its first committed observation: either one named domain or explicitly unscoped processing. Reopening a durable stream derives that scope from its ledger-linked observations. A window cannot later be used by a different domain or switch between scoped and unscoped processing; create a separate stream instead. Older unscoped observations are not assigned a guessed domain.

New unclassified typed-event identities registered through `event-register` are not eligible for the storage-backed synthetic replay, host, or promotion paths. Fixed test fixtures use the Rust `register_synthetic_event` API, which records a caller assertion and must only be used for known synthetic bytes. Existing unclassified typed-event artifacts remain stored but are not silently reclassified. This guard does not authenticate the caller: low-level raw-byte execution APIs and workers remain available for synthetic tests; none are a sandbox.

`window-register` also stores new unclassified identities without authorizing the storage-backed synthetic window path. Fixed synthetic tests use `register_synthetic_window`; its caller assertion is not authentication or containment. Previously stored window artifacts without a synthetic record remain inspectable but are not silently reclassified or executable through that path.

The storage-backed scalar runtime likewise requires a caller-asserted synthetic record. `register` is non-executing intake; fixed test modules use `register_synthetic_scalar`. Pre-v31 unclassified scalar artifacts are preserved but are not silently reclassified. These classifications are accidental-use guards, not independent attestations.

Classification is associated with the exact artifact identity. Registering bytes that already have a synthetic record returns that identity and does not revoke its existing classification; intake itself does not establish or alter it.

New typed-event replay and observation records include a canonical worker-file path and SHA-256 digest, linked to the ledger. The file is read before and after the operation; a detected change rejects the result. Typed-event promotion requires matching recorded worker digests. Older records without worker provenance remain inspectable but cannot authorize promotion. These checks are provenance aids, not attestation: a file can change between reads, a wrapper can execute other code, and no trusted worker or sandbox is established. Worker-file reads occur outside the worker deadline.

For typed-event promotion, the replay and observation worker digests must also match the worker recorded by the replay used to select the domain fixture. A fixture selection made from older evidence without worker provenance cannot authorize promotion. This binds a local selection; it does not establish that the selected worker is trusted.

Typed-event replay and host execution recheck the captured worker file immediately before each run and before persisting the operation. `process_domain` also checks that the selected worker's recorded digest matches the fixture-selection worker before execution and again when committing the observation. These are consistency checks only; they do not attest the executable or prevent replacement between file reads and process launch.

The experimental container replay can run a validated fixture against a fixed synthetic built-in. Build `sese-fixture` with `cargo build --locked --offline --bin sese-fixture`, create a private, empty journal directory, then from `runtime-spike/` run:

```sh
python3 scripts/synthetic_container_replay.py fixtures/event-synthetic-v1.json --inspector target/debug/sese-fixture --candidate throttle --image sha256:IMAGE_DIGEST --docker-host unix:///PATH/TO/docker.sock --journal-dir /PATH/TO/JOURNAL
```

Supply an existing local worker image digest and socket. The helper rejects images with declared volumes and fails closed if it cannot inspect that setting. The command prints case comparisons and exits nonzero for a mismatch or execution failure. The journal contains owner-only replay reports and separate per-container lifecycle receipts. A running case links to its receipt before Docker is contacted, so the existing read-only `inspect_pending` helper can investigate it after interruption. Reports are updated after each case; `pending` or `running` after interruption does not mean a pass. These experimental records are not saved in the application's ledger, authenticated, or automatically recovered. The inspector, image and daemon are caller-selected and unauthenticated. This experiment is synthetic and cannot authorize promotion or establish isolation for untrusted code.

To compare fixed synthetic champion and shadow candidates in separate containers with the same input, run:

```sh
python3 scripts/synthetic_container_shadow.py allow throttle --event 1 42 3.0 4 --window 0 0.0 0 0 --image sha256:IMAGE_DIGEST --docker-host unix:///PATH/TO/docker.sock --journal-dir /PATH/TO/JOURNAL
```

The owner-only shadow report records both verdicts and lifecycle receipt paths. An execution failure leaves a failed report; the comparison does not update host state or the application ledger and cannot authorize promotion. The same image, daemon and isolation limitations apply.

To compare those completed reports with an application's review preview, use:

```sh
python3 scripts/container_review_compare.py DB DOMAIN REPLAY_ID OBSERVATION_ID REPLAY_REPORT SHADOW_REPORT --outcome-cli target/debug/sese-outcome
```

The command checks fixture and candidate identities, inputs and verdicts, and prints the application preview's checkpoint. It also checks the linked owner-only lifecycle receipts against the recorded requests and verdicts, including confirmed-removal status and distinct container IDs. These checks do not authenticate the reports, receipts, image, daemon or checkpoint; the result is explicitly ineligible for promotion. The preview command opens the application database using its normal writable/migration path, and its checkpoint may become stale after it is read.

Add `--record-actor NAME` to append a caller-asserted link to the SHA-256 digests of the two report files at that exact preview checkpoint. Inspect the returned sequence with `target/debug/sese-outcome event-container-assert-inspect DB SEQ`. The ledger event is not a review, replay or promotion gate; the lower-level assertion command accepts caller-provided digests without validating the reports. Retain the reports and receipts separately: the event hashes the report files, not the referenced receipt contents. A changed ledger head rejects the assertion and requires a fresh comparison. Recording the assertion advances the ledger, so obtain a new review preview before recording a decision.

For a single synthetic operation that takes the application preview, runs the matching built-in fixture and champion/shadow inputs in containers, checks the results and records that assertion, use:

```sh
python3 scripts/container_review_run.py DB DOMAIN REPLAY_ID OBSERVATION_ID FIXTURE.json --outcome-cli target/debug/sese-outcome --inspector target/debug/sese-fixture --image sha256:IMAGE_DIGEST --docker-host unix:///PATH/TO/docker.sock --journal-dir /PATH/TO/JOURNAL --actor NAME
```

Only the fixed synthetic built-ins are supported. The operation records an assertion only after all comparisons and receipt checks pass and the original checkpoint is still current. It captures the exact report and receipt JSON bytes in a bounded ledger payload and leaves a bundle in the journal. The `event-container-assert-inspect` command returns the captured bundle; the assertion is not a review or promotion decision. The lower-level capture command accepts caller-provided JSON and does not itself verify execution. A crash can leave a pending lifecycle receipt, and daemon, image and report authenticity remain unverified.

To recheck a captured bundle without the original journal files, retain a checkpoint at or after its assertion and run:

```sh
python3 scripts/verify_container_capture.py DB ASSERTION_SEQ --seq CHECKPOINT_SEQ --hash CHECKPOINT_HASH
```

The verifier opens the database read-only, checks the chain and checkpoint, replay and observation bindings, and consistency of the captured reports and receipts. It reuses the comparison code and cannot recheck historical file permissions, authenticate execution, establish checkpoint custody, or authorize promotion. Digest-only assertions have no bundle to verify.

An independent Python 3 standard-library verifier can check the ledger rows against a separately retained checkpoint:

```sh
python3 runtime-spike/scripts/verify_ledger.py demo.db --seq CHECKPOINT_SEQ --hash CHECKPOINT_HASH
```

It opens SQLite read-only, recomputes the hash chain and checks that checkpoint. By default it does not verify database records referenced by ledger events. Add `--verify-synthetic-candidates` to check caller-asserted synthetic candidate records, artifact digests and their ledger links across the three ABIs. Its JSON still reports `evidence_bindings_checked: false` because other evidence bindings are not independently verified. It does not authenticate actors, signatures or checkpoint custody. Use the application verifier as well for its supported evidence bindings. The Rust test suite invokes `python3` for this independent check.

For the first synthetic event cycle, `--verify-event-cycle` combines `--verify-synthetic-candidates`, `--verify-event-followups`, and `--verify-event-stream-receipts` in one invocation. It requires a checkpoint covering the ledger head in its read snapshot. Check the reported per-category counts, missing worker records, and `event_streams_without_receipts`. It verifies recorded links and transitions within the supplied checkpoint's threat model; it does not authenticate the worker, operator, fixture, checkpoint custody, or real-world outcomes, and overall `evidence_bindings_checked` remains false.

All synthetic outcome exports identify `outcome_evidence` as `caller_asserted`. Scalar and window evidence rows include their recorded fixture version, digest, and replay environment, as well as `reward_derivation: validated=1;falsified=-1`. New scalar and window replay records state that the runtime is unverified: callers supply the worker, which may not contain Wasmi. Older records may name a Wasmi version without verification; their exports retain that historical label and set `runtime_provenance_verified` to false, as do new exports. The reward is a mapping of the caller's status, not a verified real-world reward. The recorded fixture digest is ledger-bound; these exports do not independently recompute it. Event evidence rows do not assign a reward. None of these rows are marked training-ready.

Add `--verify-event-replays` to independently recompute the ledger-bound evidence digest for typed-event replay runs and their ordered stored cases. This check rejects missing replay links and changed bound fields. Add `--verify-event-fixtures` to also recompute fixture identities and check exact stored case outcomes. Neither option validates expected answers, re-executes guests, verifies worker provenance, or checks other evidence types. The overall `evidence_bindings_checked` result remains false.

Add `--verify-event-workers` to check ledger links for recorded typed-event worker paths and digests. The output reports both checked records and `event_workers_unrecorded`; a nonzero unrecorded count means some legacy rows have no worker provenance. This does not read the historical executable or establish its trustworthiness, and does not replace the replay or observation evidence checks.

Add `--verify-event-observations` to recompute typed-event observation evidence, including candidate bytes and any domain, and verify its ledger link. The output reports `event_observation_bindings_checked`; missing or duplicate links fail the check, including for unlinked legacy observations. This checks recorded bytes, not the correctness of observations or execution. Use the worker option separately for recorded worker provenance. Other evidence remains unchecked independently.

Add `--verify-event-reviews` to check typed-event review fields and resolved outcome records against their ledger events. It also checks all typed replay and observation bindings because reviews refer to them. Pending reviews have no outcome event; the JSON reports review and resolved-outcome counts separately. The checks do not authenticate the operator, validate a caller-asserted outcome, verify worker provenance, or check all evidence types.

Add `--verify-event-champions` to reconstruct typed-event bootstrap, promotion, and rollback history and compare it with the stored champion and generation. This also checks typed replay, observation, review, and outcome bindings. The verifier checks that each promotion references an approval for its candidate; it does not independently re-evaluate every promotion policy, fixture selection, worker provenance, or stream receipt. It does not authenticate the checkpoint or operator.

Add `--verify-event-traces` to check caller-asserted synthetic trace and explicit-pair fields against their ledger events and linked reviews. This also checks typed replay and observation bindings. The JSON reports `event_trace_bindings_checked`; it does not establish model authorship, pair comparability, outcome truth, or training eligibility. Older databases must be migrated to schema v33 before this option can inspect them.

Add `--verify-event-followups` to check typed-event fixture and follow-up selections, receipt bindings, and the recomputed comparison and chronology. It also verifies typed replay fixtures, observations, reviews, champion history and recorded worker provenance links; counts are reported separately, including unrecorded workers. It also audits current follow-up promotion gates against historical transitions: selected replays present at promotion must have passed and been selected before the approving review. `event_followup_promotion_gates_checked` counts those applicable promotions; a selection recorded after a promotion does not retroactively change that promotion. Historical transitions that violate the current gates cause this optional audit to fail, even if an older version accepted them. The option requires schema v35. It checks consistency with the supplied checkpoint, not worker trust, fixture independence, actor identity or real-world outcome truth; overall evidence verification remains incomplete.

Add `--verify-event-attestations` to independently check v36 synthetic attestation receipts and, when present in v37, preference receipts, including stored checkpoint references and parent ledger links. This also checks typed replay, observation, review, outcome and trace bindings. It reports `attestation_signatures_checked: false`: the Python verifier does not check signatures, authority custody or outcome truth. Use the separate Rust recheck with the original pinned authority set for signature verification.

Add `--verify-event-signatures AUTHORITIES_JSON --authority-sha256 DIGEST` to verify every retained trace, outcome and preference signature with the optional Python `cryptography` package. Supply the exact authority bytes and a separately retained SHA-256 pin; the checkpoint must cover every signature receipt, and all receipts must reference that configuration. The result reports a signature count and `authority_key_strength_strictly_checked: false`. This option also independently decodes every retained trace, outcome and preference payload and compares its references, recorded fields and asserted timestamp ordering to the database; the report includes a payload count. It does not verify that an outcome evidence-reference points to an independently observed fact. This PyCA check is independent of the Rust serializer, but has not been shown equivalent to Rust's strict verifier and does not implement its weak-key rejection or establish authority custody, timestamp authenticity, signer independence or outcome truth. Combine `--verify-event-signatures` with `--verify-event-followups` to also match signed `synthetic-followup-receipt-v1` claims to independently recomputed follow-up receipts. The report counts checked claims and separately counts other outcome methods not checked; neither count establishes real-world observation. The core verifier does not require `cryptography` unless this option is used.

Add `--verify-event-export EXPORT_JSONL` to compare a synthetic inspection export with those database records and the supplied checkpoint. The checkpoint must cover both receipts. This check rejects duplicate fields and changed values, but permits JSON reformatting. It decodes the canonical signed payloads and checks their references, recorded fields and timestamp ordering; it reports `attestation_payload_record_links_checked: true` and `attestation_signatures_checked: false`. It does not establish signature validity, authority or checkpoint custody, timestamp authenticity, comparability, or outcome truth.

Run the standalone verifier's Python tests from the repository root with `python3 -m unittest discover -s runtime-spike/scripts -p 'test_*.py'`.

Add `--verify-event-stream-receipts` to verify legacy and measured stream receipt fields and observation links, reconstruct their state transitions, and compare receipt-backed streams with current state. The output reports separate receipt counts. Legacy streams created before receipts existed may be present without a verifiable history; measured streams require receipts. This also checks typed-event observation bindings. A retained checkpoint is still necessary, and overall evidence verification remains incomplete. Run the independent transition tests with `python3 -m unittest discover -s runtime-spike/scripts -p 'test_stream_history.py'`.

## Conditional synthetic training export

For a guided local example with fixed public test keys and explicit approval, see [the synthetic training walkthrough](ONBOARDING.md#optional-inspect-conditional-training-examples).

`event-training-export` accepts one retained signed follow-up attestation and its follow-up receipt. It rechecks the ledger, signatures, pinned authority set and policy, covering checkpoint and synthetic fixture comparison. The pinned policy must explicitly set `training_export_allowed` to `true`; omitted or false denies export. Updating the policy changes its digest and requires a new pin. Record the trace before resolving the outcome; its signed capture time must not be later than the signed observation time. SFT and DPO require a validated outcome; DPO also requires a matching signed preference receipt and reviewer declaration. RLVR requires a pinned reward rule.

```sh
sese-outcome event-training-export DB sft ATTESTATION_ID FOLLOWUP_ID AUTHORITIES_JSON AUTHORITIES_SHA256 POLICY_JSON POLICY_SHA256 - - CHECKPOINT_SEQ CHECKPOINT_HASH accept-synthetic-trust-v1
sese-outcome event-training-export DB dpo ATTESTATION_ID FOLLOWUP_ID AUTHORITIES_JSON AUTHORITIES_SHA256 POLICY_JSON POLICY_SHA256 PREFERENCE_ID - CHECKPOINT_SEQ CHECKPOINT_HASH accept-synthetic-trust-v1
sese-outcome event-training-export DB rlvr ATTESTATION_ID FOLLOWUP_ID AUTHORITIES_JSON AUTHORITIES_SHA256 POLICY_JSON POLICY_SHA256 RULE_JSON RULE_SHA256 CHECKPOINT_SEQ CHECKPOINT_HASH accept-synthetic-trust-v1
```

On success, stdout contains one JSONL row and stderr a structured inclusion report; failures emit no row and report the reason on stderr. This command opens the database read-only and requires schema v37; migrate older databases separately before exporting. The literal acceptance is a caller assertion. Operators must independently assess and retain the pins and trust declarations. Output is synthetic and explicitly conditional: custody, independence, model origin, real-world outcomes and DPO comparability are not verified. See [the training export contract](../contracts/training-export-v1.md) for limits and remaining verification.
