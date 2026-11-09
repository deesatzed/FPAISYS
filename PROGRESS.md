# Progress

- Inspected GOAL.md, AGENTS.md and the workspace: four source sketches, no implementation or Git repository. Available tools: rustc 1.98.0, cargo 1.98.0, Python 3.15.2.
- Added proposed versioned behavioral contract and implementation sequence. The contract identifies unresolved canonical encoding, evidence freshness, runtime isolation and exact export schema decisions; these are not implemented or verified.
- Next: assess runtime feasibility and failure containment, define executable contracts and migrations, then implement and test the full local workflow. No runtime, demo, or test suite exists yet.
- Compared wasmi, Wasmtime and mlua using their API documentation and checked local availability. Added `contracts/runtime-assessment.md`; chose wasmi provisionally for a bounded spike, not as a validated isolation boundary. Standalone runtimes and cached wasmi/mlua were absent. Next: implement and test the worker and limits using synthetic test modules, then build persistence and governance.
- Built `runtime-spike` with pinned wasmi 0.40.0 and lockfile. The first test failed against the fail-closed stub, then passed after implementation. `cargo test --locked --manifest-path runtime-spike/Cargo.toml`: 7 synthetic tests pass (execution, imports, infinite loop, initial and growing memory, fresh guest state, malformed/oversized input). `cargo fmt --check` passes after formatting.
- This is an in-process experiment using a scalar placeholder ABI, not a safe worker or app. Parser resource limits, process containment, timeouts, crash/missing responses, host state, replay, persistence, governance and exports remain open. Never use this spike to run untrusted artifacts. Dependencies downloaded into the local Cargo cache; clean checkout verification remains outstanding.
- Added experimental child worker and supervisor with cleared environment, bounded request/response, deadline/kill and failure handling. The first supervisor success test failed against a stub before implementation. `cargo test --locked --manifest-path runtime-spike/Cargo.toml` now passes 12 tests, including worker failure, absent response and zero deadline; formatting check passes.
- Worker process is not an OS sandbox and does not enforce filesystem/network denial or process-wide memory limits. Deadline test exercises zero timeout, not all races or descendant process behavior. Current public API and scalar ABI are experimental; no untrusted artifacts should be run. Full containment, replay, storage and operator workflow remain open.
- Added an initial Rust SQLite candidate store, transactional schema initialization, rejection of unknown or unversioned nonempty databases, SHA-256 identities and digest verification on reads. Storage tests verify restart, distinct identities and schema rejection; `cargo test --locked --manifest-path runtime-spike/Cargo.toml --test storage` passes 4 tests. The earlier complete suite passed 14 tests before the two additional storage checks; formatting passes.
- Storage is an initial experimental component, not the complete migration strategy or application. WAL, tamper testing, concurrent promotion, replay records, telemetry and append-only governance are not implemented. No untrusted execution or readiness claim is warranted.
- Added an experimental scalar replay runner that executes each synthetic case in the worker, hashes the ordered fixture content, and transactionally stores run and per-case results in a v1-to-v2 SQLite migration. Empty/duplicate cases fail closed; worker errors are distinct from mismatches. The complete suite passed 18 tests before adding the positive/worker-error case; the focused replay suite now passes 3 tests.
- Replay is not a promotion gate or the full event/state ABI; it assumes synthetic provenance and does not validate untrusted execution containment. Migration from an actual preexisting v1 database, durable timestamps, interrupted runs, and replay evidence freshness need further verification.
- Verified migration from an actual v1 SQLite file preserves candidates, a failed migration rolls back schema/version changes, modified candidate bytes are rejected, and replay case details persist across database reopen. Full `cargo test --locked --manifest-path runtime-spike/Cargo.toml` passes 23 tests; formatting check passes.
- These are local synthetic persistence tests, not a security audit or proof of crash recovery, WAL operation, containment, or promotion correctness. The goal remains incomplete.
- Added a standalone experimental append-only decision/outcome ledger with length-prefixed UTF-8 event encoding, SHA-256 links, transactional append and verification before append. Focused tests cover unknown and duplicate outcome resolution and event modification. The full suite passes 25 tests and formatting check passes.
- Ledger is not integrated with candidate/replay evidence, promotion, authorization or the versioned storage schema. It uses synthetic provenance and treats unresolved decisions as pending implicitly. It does not enforce append-only access against direct SQLite writes, detect a fully rewritten/truncated chain without a trusted external anchor, or provide signatures. It cannot yet support approval or export claims.
- Added synthetic champion/shadow comparison with separate worker invocations, persisted results, errors and observed durations in microseconds (including process overhead); versioned storage migration adds the observation table. Tests show divergent or failing shadow cannot replace the returned champion result, and divergence is unknown when either side fails. Full suite passed 27 tests before the explicit unknown-divergence refinement; focused shadow tests and formatting pass afterward.
- This is not the host-owned event/window ABI, does not select or promote champions, and has no live telemetry or production isolation. The comparison is sequential and latency includes worker startup; it is not a runtime benchmark. Untrusted execution remains unsupported.
- Added an interactive synthetic preview connecting registration, replay, shadow comparison and an explicit approve/reject ledger entry. It performs no promotion and defaults to in-memory storage; an optional database path persists records. `runtime-spike/README.md` documents commands and limits. The initial CLI test failed against a stub; full locked suite now passes 28 tests and formatting check passes.
- The preview is not the requested finished app: review evidence references are descriptive text, approval does not enforce gates or change a champion, outcomes/exports are not exposed, and containment remains unverified. A persistent database must be selected explicitly to retain the demo records.
- Reproduced a supervisor deadline bug: a child that exits while a descendant holds stdout caused a 1.08-second wait despite an 80 ms deadline. Replaced blocking thread joins with deadline-bounded channel waits; the regression test now passes. Full locked suite passes 29 tests and formatting passes.
- The fix bounds the caller's wait but does not terminate descendants; blocked I/O threads can remain alive until inherited pipes close. Process-group cleanup and OS-level isolation are still required before untrusted execution. The application goal remains incomplete.
- Added structured SQLite review records with candidate, replay and shadow foreign keys. Approval requires matching candidate evidence, a nonempty passing replay with consistent per-case results, and completed shadow/champion results; failed replay may still be rejected. The synthetic preview uses the new check. Full locked suite passes 31 tests and formatting passes.
- Structured reviews and the separate hash ledger are not atomic or cryptographically linked. Evidence freshness, current champion binding, approved fixture authority, promotion, concurrency and tamper-resistant evidence are not implemented; a review is not permission to promote. These are synthetic checks, not containment validation.
- New structured reviews now append a hash-linked event in the same SQLite transaction (schema v5); invalid existing chains abort and roll back the review. The preview no longer writes a separate decision entry. Full locked suite passes 32 tests, including commit/rollback and formatting checks.
- Existing v4 reviews are not backfilled. Chain verification currently checks event bytes and links, not whether mutable review/evidence rows still match the event payload. External anchoring, evidence authority/freshness, promotion and containment remain unresolved; no release or security claim is supported.
- Strengthened storage ledger verification to compare every structured review to exactly one matching event, including actor, action, candidate, evidence IDs and timestamp; orphan events and missing or duplicate links fail closed. New reviews check existing integrity inside the transaction. A test first demonstrated that changing the actor was undetected, then passed after the fix; full locked suite passes 32 tests and formatting passes.
- Replay/shadow content is not hashed into the ledger, pre-v5 unlinked reviews fail verification rather than being backfilled, and full rewrite/truncation remains undetectable without an external anchor. Promotion, provenance exports, and worker isolation remain incomplete.
- Bound new review events to a versioned SHA-256 digest of typed, length-delimited replay run, ordered case and shadow observation values. Verification recomputes the digest and fails closed on missing or changed evidence. Regression test showed replay modification previously passed verification; it now detects both replay and shadow modifications. Full locked suite passes 32 tests and formatting passes.
- Compatibility limitation: earlier v5 review events lack evidence digests and now fail verification; no unverified backfill is attempted. Candidate artifact bytes are verified at registration/read, not by ledger verification. Digesting records does not establish fixture authority, trusted capture, authenticity, or protection against complete rewriting without an external anchor. Promotion and containment remain incomplete.
- Added schema v6 review outcomes: unresolved reviews report pending; one validated or falsified resolution requires actor and source and commits with a linked ledger event in the same transaction. Verification compares outcome rows to events. Tests cover pending, duplicate resolution, and outcome-source tampering; full locked suite passes 32 tests and formatting passes.
- These are synthetic records with caller-supplied sources, not independently validated observations. Outcomes are not exposed by the preview or used for exports yet. Promotion, full ABI and worker isolation remain incomplete.
- Added a snapshot-consistent JSONL export of synthetic resolved outcome evidence with candidate/replay/shadow IDs, actor, source, timestamp and explicit +1/-1 reward. It checks ledger integrity and candidate digest and excludes pending records. Focused tests cover pending exclusion, validated and falsified rewards, and refusal after outcome tampering; formatting passes. Full suite passed 32 tests before the last additional export assertions, and the focused review suite passed afterward.
- Export rows explicitly state training_ready=false: the current data lacks complete prompts, human final outputs and preference pairs. SFT/DPO and actual training-ready RLVR export remain unimplemented. The output is synthetic and its outcomes are caller-supplied, not independently validated. A failed intermediate test-edit script made no file changes and was corrected.
- Local macOS is Darwin arm64 and `/usr/bin/sandbox-exec` exists. Attempting a nested restrictive profile failed with `sandbox_apply: Operation not permitted`; an escalated retry terminated with exit 134 and no diagnostic. Neither establishes worker isolation; investigate in an appropriate host environment. No untrusted code was executed.
- Added a CLI for existing databases to inspect pending/resolved status, record a synthetic outcome with actor and source, and emit JSONL. Cross-process test exercises demo -> pending -> falsified -> export, while missing databases are not created. Focused CLI suite passes 3 tests; full suite has not been rerun since this change. The export remains explicitly not training-ready.
- Ledger evidence verification now requires the replay, champion and shadow artifact references to exist and match their stored SHA-256 identities and expected experimental runtime/ABI. Candidate digest is rechecked inside the review transaction. A regression test first showed champion artifact tampering was undetected, then passed after the fix. Full locked suite passes 35 tests and formatting passes.
- This detects these local inconsistencies, not a full database rewrite or trusted capture; the chain has no external anchor. Process isolation, production-safe execution, promotion, full event ABI and training-ready exports remain incomplete.
- Added a bounded, host-owned synthetic scalar event window and connected the demo to host-driven champion/shadow comparison. Events enter the window only after an observation is stored; a missing candidate leaves it unchanged. Full locked suite passes 38 tests; formatting passes.
- The scalar Wasm ABI still receives only the current integer, not the host window; the in-memory window does not survive restart. This is not the specified full event/window ABI or production host and does not enable untrusted execution or promotion.
- Added an experimental in-process Wasm window entry point (`evaluate_window_v1`) accepting prior window count, exact i64 sum, and current scalar event; it shares fuel, memory and import restrictions with the existing interpreter. Tests cover actual snapshot-dependent execution, empty/oversized windows, incorrect signatures, imports and infinite loops. Full locked suite passes 41 tests and formatting passes.
- This is not yet wired into the supervised worker, host, replay, or persisted ABI metadata; it is not the full WIT event/window contract. It is not a security boundary and must not execute untrusted code. Existing demo continues to use the scalar ABI.
- Added a separate bounded window-worker protocol and `supervise_window`; a real Wasm guest receives the prior window aggregate and current event across the child process. Tests cover valid execution, wrong ABI, oversize windows and truncated requests. Existing scalar protocol remains intact. Full locked suite passes 44 tests and formatting passes.
- The worker is still not an OS sandbox and is not wired into host/replay or persisted candidate ABI metadata. The event/window interface remains an experimental aggregate, not the complete WIT contract. Do not execute untrusted modules.
- Connected the experimental window worker to a host path that records the exact prior window, event, results, errors and durations in schema v7. Window artifacts use distinct ABI metadata; tests verify snapshot-dependent output, persisted bytes and rejection of ABI relabeling. Full locked suite passes 46 tests and formatting passes.
- Window observations are not yet replay or review evidence and host windows remain in memory across process lifetime only. The demo and replay still use scalar ABI. Worker containment, full WIT contract, promotion and training-ready exports remain incomplete.
- Closed an ABI boundary defect: scalar replay and shadow comparison now verify that each registered artifact has scalar ABI metadata before executing it. A synthetic module exporting both entry points demonstrated that a window-registered artifact previously passed scalar replay; the regression test now rejects it in replay and both shadow slots. Full locked suite passes 47 tests and formatting passes.
- This does not implement window-aware replay/review, production containment, promotion, or the full event ABI. Existing records produced before ABI enforcement have not been revalidated or migrated.
- Added window-aware replay with versioned fixture hashing, bounded/unique cases, real worker execution and transactional per-case persistence (schema v8). Tests cover actual window inputs, persisted snapshots, failing guest execution, duplicate cases and wrong ABI. Full locked suite passes 50 tests and formatting passes.
- Window replay and observations remain separate from the structured review/ledger and cannot authorize promotion. The worker is not isolated for untrusted code; the full WIT contract, durable host state, promotion and training-ready exports remain incomplete.
- Added schema v9 structured window reviews with matching-candidate checks, passing replay and complete shadow requirements for approval, and atomic hash-ledger entries bound to window evidence and candidate digests. Verification detects changed window replay inputs. A failed guest cannot be approved but may be rejected. Full suite passed 51 tests before the final extra negative assertions; the focused window suite passes afterward, and formatting was applied.
- Window review is still experimental: it has no user-facing flow, outcome integration, fixture authority or freshness policy, current champion binding, promotion, or external ledger anchor. Containment remains unresolved. These checks do not establish release readiness.
- Added schema v10 window-review outcomes with pending/validated/falsified transitions, actor and source, and an atomic hash-chain event. Ledger verification checks linked outcome rows and detects altered sources. Tests cover pending, successful resolution and duplicate rejection; full locked suite passes 51 tests and formatting passes.
- Outcome sources are user assertions on synthetic data. Window outcomes are not yet exposed through the CLI or export, and do not authorize promotion. Worker containment, full event ABI, durable host state and fixture authority remain open.
- Added separate window-outcome status, resolution and JSONL export CLI commands. Window exports exclude pending outcomes, verify the ledger and artifact digest, identify the window schema and remain explicitly synthetic/not training-ready. Tests exercise pending exclusion, cross-process resolution/status/export and positive provenance. Full locked suite passed 51 tests before the final cross-process resolution assertion; focused window tests and formatting pass afterward.
- The window path still has no operator-facing candidate/replay/review flow, promotion, fixture authority or production containment. The export lacks complete training examples and is not SFT/DPO/RLVR training data.
- Added an interactive synthetic window preview that registers fixed examples, executes replay and two host events, shows the prior window and comparison, and records an explicit review. Optional database persistence supports existing window-outcome commands. Tests cover rejection and EOF without a review. Full locked suite passes 53 tests and formatting passes.
- This preview is not a complete operator workflow: it does not accept arbitrary fixtures or candidates, perform promotion/rollback, restore host state, or provide OS containment. Its actor and outcomes are local synthetic assertions, not production or user validation.
- Added `window-inspect` for structured review, replay case inputs/results, shadow snapshot and outcome status from a single ledger-verified SQLite snapshot. A cross-process test confirms stored synthetic inputs and detects evidence tampering; full locked suite passes 54 tests and formatting passes.
- Inspection does not yet expose three-column prose or outcome source, and it is not an external authenticity guarantee. Full app, promotion, fixture authority, durable host state and worker containment remain incomplete.
- Added an experimental named `DurableHost` window (schema v11). Its state and comparison observation commit in one SQLite transaction; a generation-and-snapshot compare-and-swap rejects stale writers. A focused test verifies restart, capacity mismatch, stale rejection and no stale observation. Full locked suite passes 55 tests and formatting passes.
- The existing preview still uses the in-memory host. Durable stream state is not hash-chained or protected from direct database manipulation, and process crash recovery and truly concurrent independent processes have not been tested. The host still uses synthetic aggregate ABI; containment, promotion and full WIT remain incomplete.
- Window preview now records the displayed three-column card in the same transaction as its structured review and hash-linked card event (schema v12); `window-inspect` returns the card. Test first showed the card was absent, then verified saved text and rejection of modified card content. Full locked suite passes 55 tests and formatting passes.
- Legacy review calls may still omit cards; prior cards are not backfilled. A card and its final ledger event can be removed together without detection when no external anchor exists. Outcome source is still absent from inspection; promotion, containment and trusted fixture authority remain incomplete.
- New card-bearing window reviews now use a distinct hash-chained event kind that requires a matching card and card event. A regression test reproduced undetected removal of a card and its final event, then verified detection for newly created reviews. Full locked suite passes 56 tests and formatting passes.
- Existing legacy review events remain compatible and cannot acquire this guarantee retroactively. An attacker able to rewrite the complete chain or remove a whole trailing review still cannot be detected without an independent anchor. Promotion, fixture authority and containment remain incomplete.
- Added operator-supplied external ledger checkpoints: creation verifies the database and returns the current nonempty head; verification checks both current ledger integrity and the retained sequence/hash. A cross-process test verifies a valid extension and detects removal of the entire anchored review chain. Full locked suite passes 57 tests and formatting passes.
- A checkpoint only has value if retained independently and protected by the operator. It does not authenticate an actor, detect removal after its position, or establish trusted custody. Fixture authority, containment, promotion, and the full app remain incomplete.
- Added an explicit local window fixture pin per domain and a comparison API that recomputes a replay pack digest from its persisted inputs. Pins cannot be replaced through this API. Schema v14 preserves explicit case ordinals; tests verify distinct packs, rejected replacement, VACUUM stability and changed-case detection. Full locked suite passes 59 tests and formatting passes.
- These pins are not ledger-bound, externally authenticated, exposed to the CLI, or enforced by review/promotion; direct database writes can alter them. No claim of trusted fixture authority or promotion eligibility is made. Earlier case ordinals are reconstructed from their prior row order during migration; provenance of that historical order is not independently verified. Containment and the full app remain incomplete.
- New fixture pins now commit with a linked ledger event in one transaction (schema v15 with stable pin IDs); verification detects changed, missing or duplicate pin links. Existing v14 pins are preserved but fail verification because they have no event; no unverified backfill is performed. Tests cover pin tampering and legacy fail-closed migration. Full locked suite passes 61 tests and formatting passes.
- Pin records identify an operator selection, not independent authority or fixture quality. Pin matching is not yet a promotion gate and is not performed in a single transaction with promotion. Legacy pins require an explicit recovery workflow. No external checkpoint automatically protects against full rewrite or removal; containment and promotion remain incomplete.
- Exposed explicit fixture pin and exact-match commands through the outcome CLI. Pin matching and recomputation now use a single SQLite read transaction. Cross-process tests cover a selected pack, a different pack and a missing pin. Full locked suite passes 62 tests and formatting passes.
- This makes an operator selection reviewable but does not authenticate the actor or establish fixture quality. Pins are not yet enforced in promotion and cannot be replaced through the CLI. Promotion, containment and full event ABI remain incomplete.
- Added experimental synthetic champion metadata and hash-linked bootstrap/promotion transitions (schema v16). Promotion uses one immediate transaction to verify the ledger, current champion, matching card-bearing approval, latest candidate replay/observation/review, completed evidence and recomputed match to the selected fixture pin. Tests cover missing pin, nonexistent/rejected/stale review, stale champion and a successful metadata transition. Full locked suite passes 63 tests; formatting passes. A migration test fixture initially retained newer tables; corrected the fixture and reran the suite successfully.
- This does not mount or activate a runtime slot, implement rollback, authenticate actors, establish fixture authority, enforce wall-clock freshness, or validate OS containment. Global latest-ID rules are provisional and can reject otherwise relevant domain evidence. Concurrent independent-process behavior and crash recovery remain untested. No production promotion is supported.
- Added explicit rollback of the most recent synthetic metadata promotion (schema v17). The transaction verifies ledger integrity, expected current champion, previous artifact and immediate predecessor, then appends a linked rollback event. Tests cover successful rollback, stale target, duplicate rollback, persisted champion on reopen and detection of modified champion state. Full suite passed 63 tests before the added restart/tamper test; focused promotion tests pass afterward and formatting passes.
- This is still metadata only, not runtime activation or a production rollback. No automatic fallback, OS isolation, crash-recovery test, concurrent independent-process test, actor authentication or fixture authority is established.
- Connected `DurableHost::process_domain` to the current synthetic champion. It verifies champion metadata, executes the selected artifact, and checks the champion ID and generation again in the same transaction as the window/observation update. A test demonstrates actual synthetic output changes after promotion and returns after rollback. Full locked suite passes 64 tests and formatting passes.
- The generation guard is implemented but a real concurrent promotion-during-execution race has not yet been exercised. No production slot, live traffic, OS isolation, authenticated actor or full event ABI exists; worker containment and fixture authority remain unresolved. The synthetic path is not suitable for untrusted modules.
- Added a deterministic concurrency test: a synthetic worker waits while a second SQLite connection rolls back the champion, then the host rejects the stale generation without committing a new observation or advancing its window. Full locked suite passes 65 tests and formatting passes.
- The synchronization worker is a test shell script, not an isolated Wasm workload; this verifies one rollback overlap and does not prove all concurrent or crash interleavings. Runtime containment, authenticated authority, production activation and complete app requirements remain open.
- Exposed synthetic champion initialization, inspection, promotion and rollback commands with explicit domain, expected champion, review ID and actor. A cross-process test walks a pinned synthetic fixture through approval, bootstrap, promotion, stale retry rejection and rollback. Full locked suite passes 66 tests; formatting passes.
- These commands update only local synthetic metadata. Actor strings are not authenticated; fixture quality, OS isolation, live production execution and user value remain unverified. Complete app goal remains active.
- Added synthetic promotion negative-path tests for a mismatched fixture pin, a newer failed replay, and modification of review-bound replay evidence through another SQLite connection. Each leaves the recorded champion unchanged; the latter is exercised through the CLI. Full locked suite passes 68 tests and formatting passes.
- These tests do not establish fixture authority, actor authentication, production isolation, crash recovery, or completeness of all gate interleavings. The goal remains active.
- File-backed `Storage` now requests SQLite WAL after schema migration and fails if WAL is unavailable; in-memory SQLite remains in memory mode. Added a child-process crash test that leaves an uncommitted champion update and verifies the previous champion and ledger after reopening. The test first observed DELETE mode, then passed after the change. Full locked suite passes 69 tests and formatting passes.
- This covers one process-exit interruption in the local environment, not power loss, committed-write durability, network filesystems, all crash points or full recovery. Worker isolation and other goal requirements remain incomplete.
- Added `window-register DB ARTIFACT_PATH` for bounded local artifact intake. It stores bytes and returns identity with explicit `executed=false` and `validated=false`; it does not run or parse the artifact. A test registers an infinite-loop Wasm without executing it and rejects oversize input. Full locked suite passes 70 tests and formatting passes.
- Registration does not prove ABI conformance, provenance, safety or review eligibility, and user-supplied code must not be executed until containment is verified. Full event ABI, fixture authority, controlled runtime and training-ready exports remain incomplete.
- Added an injected-write-failure integration test at two actual promotion transaction points: transition insertion and ledger insertion. Both leave the champion and history unchanged, and promotion succeeds after removing the injected failure. Full locked suite passes 71 tests and formatting passes.
- The test exercises SQLite statement failures, not process termination at those precise points, power loss, or production deployment. Worker containment, authenticated authority and full app completion remain open.
- Window review inspection now includes outcome actor, source and timestamp when resolved, with null details while pending. The cross-process inspection test first failed on the missing source and passes after the change; full locked suite passes 71 tests and formatting passes.
- These values are caller-supplied synthetic assertions, not independently verified outcomes. Production containment, complete event ABI and training-ready exports remain open.
- Added a fixed synthetic cycle demo joining fixture selection, bootstrap, replay, durable host comparison, explicit review, promotion, and optional rollback. Tests demonstrate changed synthetic output after approval, unchanged champion on rejection, rollback behavior and refusal to use an existing database. Full locked suite passes 74 tests and formatting passes.
- The demo uses built-in synthetic modules, a caller-specified worker and local unauthenticated actor. Outcomes remain pending until explicitly recorded. It is not arbitrary candidate intake/replay, full WIT, verified containment, production readiness or validation of user value. The broader goal remains incomplete.
- Copied the runtime source without its build directory to `/private/tmp/sese-source-check.OTL28V` and ran `cargo test --offline --locked` with a fresh target: all 74 tests passed using the existing local Cargo cache. From that copy, built the binaries, ran the documented approve/rollback synthetic cycle and inspected its persisted review; results were champion 5 -> 7 -> 5 and pending outcome with ledger verification true.
- This was a source copy, not a clean Git checkout: this directory has no Git repository. Dependency acquisition from an empty cache, relocated production packaging, independent user verification and all untrusted-code containment remain unverified. Updated README statements that had become stale as synthetic promotion and DurableHost were added. The goal remains incomplete.
- Using the fresh source copy from the previous check, an empty Cargo cache and a separate empty target directory, `cargo test --locked` fetched dependencies and passed all 74 tests on Darwin arm64 with Cargo/rustc 1.98.0. The current runtime source and lockfile match that copy; only README wording changed afterward.
- This establishes dependency acquisition and test execution for this machine, not a clean Git checkout (none exists), packaging or relocated launch, worker containment, or independent operator validation. The goal remains incomplete.
- Added an experimental in-process core-Wasm event ABI mirroring the sketched window/event fields and a validated decision/score/reason verdict. Tests cover actual guest calls, full-width unsigned values, nonfinite input/output, invalid decision codes, forbidden imports and fuel exhaustion. Full locked suite passes 78 tests and formatting passes.
- This is a flattened core-Wasm experiment, not WIT Component Model compatibility, supervised execution, host/replay integration or a containment boundary. It must not execute untrusted modules; no latency or security claim is established. The full goal remains active.
- Added an experimental bounded event-worker protocol and supervised call for the typed event ABI; the parent revalidates response length, decision and finite score. Tests cover real guest round trip, invalid guest verdict, missing response and truncated request, while existing scalar/window supervisor tests remain green. Full locked suite passes 80 tests and formatting passes.
- This worker is not OS-isolated or WIT Component Model compatible, and the event ABI is not wired into host, replay, review or promotion. It must not run untrusted code. Full goal remains incomplete.
- Added an experimental typed-event host and schema v18 observations. It computes snapshots from a bounded prior-event window (synthetic latency in microseconds; mean and nearest-rank p99 rounded up), evaluates champion and shadow through the event worker, and stores inputs, results, errors and elapsed times. Tests cover real prior-state input, persistence, wrong ABI/nonfinite rejection and shadow failure. Full locked suite passes 83 tests and formatting passes.
- Event windows are in-memory, `last_error` is supplied by the caller, and observation JSON is not linked to replay/review or the ledger. The selected latency interpretation and p99 rule are implementation assumptions, not validated workload semantics. This is not WIT component compatibility or OS containment; untrusted execution remains disallowed.

### Typed-event synthetic replay

Added typed-event replay for fixed synthetic core-Wasm modules. Packs use an ordered, versioned SHA-256 encoding of case IDs, inputs, and expected verdicts; finite floating-point values are compared by bits, including signed zero. Schema v19 atomically stores run metadata and ordered per-case input, output, and errors. Records identify the environment as synthetic. The tests cover successful and failed verdicts, signed zero, traps, invalid fixtures, wrong ABI, digest stability, and migration through the existing suite.

Verification: `cargo test --locked --manifest-path runtime-spike/Cargo.toml` passed (86 tests); `cargo fmt --manifest-path runtime-spike/Cargo.toml -- --check` passed. Typed-event replay is not integrated into review, promotion, or a verified containment boundary. Continue to execute only fixed synthetic artifacts; the event window remains in memory.

### Typed-event replay read-back

Added `Storage::verify_event_replay` to read one SQLite snapshot, validate candidate artifact identity and ABI, reconstruct ordered typed cases, recompute the fixture digest, and check encoded finite floats, verdicts, errors, and run/case result consistency. Tests cover signed zero and worker errors as well as inconsistent edits to the recorded result, fixture ID, or displayed float. The focused test was first run before the API existed and failed to compile; after implementation, the full locked suite passed 87 tests and formatting passed.

This verifies internal consistency only. The digest and outcomes are stored in the same database and an attacker can rewrite them coherently; replay results are not yet ledger-bound, externally anchored, authenticated, or used by typed-event review/promotion. Verification does not re-execute a candidate or prove fixture quality. Untrusted execution remains prohibited because OS containment is not verified; the overall goal remains open.

### Typed-event replay ledger binding

New typed-event replay runs now append a hash-linked event in the same SQLite transaction as the run and cases. Its domain-separated evidence digest covers run metadata, ordered case data and results, and candidate bytes and ABI metadata. Dedicated replay verification requires the link and checks the ledger; general storage verification also checks links that exist. A focused test first demonstrated that a coherent result rewrite passed the old consistency check, then passed after ledger binding. Tests also cover a missing link and an injected ledger-write failure rolling back the entire replay. The full locked suite passed 89 tests; formatting passed.

Pre-existing typed-event runs without links fail dedicated verification and are not silently backfilled; general storage verification permits those legacy unlinked rows. Removing a final event, rewriting the entire chain, or deleting a run and its final event requires an independently retained checkpoint to detect; the chain does not authenticate actors. This synthetic path still lacks review/promotion integration and verified OS containment, and must not run untrusted code. The overall goal remains incomplete.

### Durable typed-event window

Added an optional named persistent stream to the experimental typed-event host (schema v20). It restores bounded latency samples on reopen and commits the next window and observation in one immediate SQLite transaction. A generation and prior-byte comparison rejects stale writers; in-memory state changes only after a successful commit. Stored values are checked for size, finite range and capacity on load. Existing in-memory mode remains available.

Tests cover restart, stale-writer rejection, capacity mismatch and an injected observation-write failure that leaves both stored and in-memory state unchanged. The focused restart test first failed because the durable API was missing. The complete locked suite passed 91 tests and formatting passed.

This remains a synthetic core-Wasm experiment: stream state and observations are not ledger-bound or integrated with typed-event promotion, `last_error` remains caller supplied, and concurrency and crash behavior have only the tested coverage. OS containment is unverified and untrusted artifacts must not be executed. The full goal remains open.

### Typed-event observation ledger binding

New typed-event observations append a hash-linked ledger event atomically with the observation and any durable window update. The evidence digest covers recorded prior state, event, both results/errors, timings, timestamp, and both artifact records; artifact SHA-256 and ABI are rechecked before binding. Dedicated verification requires a link and verifies the chain. General storage verification checks existing observation links but permits legacy unlinked rows; dedicated verification rejects them.

A focused test first failed because observation verification was absent. Tests now detect an altered shadow error and missing link, and an injected ledger-insert failure rolls back the observation and durable stream update. The full locked suite passed 93 tests and formatting passed.

The durable stream itself is not bound to the ledger, and observation binding alone does not establish review eligibility, authenticate actors, or prove recorded data came from a trustworthy environment. Complete chain rewrites or final-event removal require an independent checkpoint to detect. Verification scans linked history and performance at scale has not been measured. Typed-event review/promotion, OS containment and the full goal remain incomplete; do not execute untrusted artifacts.

### Typed-event decision review

Added schema v21 and an explicit synthetic typed-event review with a three-column card. A single transaction verifies the ledger, recomputes replay consistency, requires linked replay and shadow evidence for the same candidate, and appends a decision event bound to both evidence hashes. Approval requires a passing replay and complete champion/shadow results; rejection can record failed evidence. `event-review` and `event-inspect` expose this path in the outcome CLI, including ordered replay cases and the shadow snapshot. Review creation does not promote anything.

Tests cover matching and mismatched candidates, missing card fields, failed replay and execution, explicit rejection, cross-process recording/inspection, and detection of a changed card. The initial focused test failed because the API did not exist; the full locked suite now passes 97 tests and formatting passes.

Typed-event outcomes, fixture authority, evidence freshness policies, promotion/rollback and integration with the running champion remain absent. Review actors are unauthenticated; ledger rewriting or final truncation requires an independently retained checkpoint to detect. No OS containment or permission to execute untrusted artifacts is established. The goal remains open.

### Typed-event outcomes and evidence export

Added schema v22 for one explicit validated or falsified outcome per typed-event review. An outcome records actor, source, timestamp and a ledger link to the decision in one transaction. Review inspection shows pending or resolved status with provenance. The CLI now exposes `event-status`, `event-resolve`, and `event-export`. The JSONL export excludes pending reviews, verifies bound evidence, includes candidate, fixture, decision and outcome provenance, and labels every row synthetic and `training_ready: false`; it does not fabricate rewards or training pairs.

A focused test first failed due to missing APIs. Tests now cover pending exclusion, required provenance, duplicate resolution rejection, resolved export, cross-process use, and rejection of a modified source. The full locked suite passed 98 tests and formatting passed.

Outcome labels are unauthenticated user assertions, not independently observed real-world validation. Typed-event fixture authority, promotion/rollback, SFT/DPO/RLVR training eligibility, and verified containment remain incomplete. Untrusted artifacts must not be executed; the goal remains open.

### Typed-event fixture selection

Added schema v23 for an explicit local typed-event fixture selection per domain. Selection requires a ledger-bound, internally verified replay and stores its version, digest, actor and replay reference with a ledger event. A same-snapshot comparison verifies later replay evidence and checks the exact selected identity. CLI commands `event-pin` and `event-pin-match` expose selection and comparison; a selection cannot be replaced through this API.

The initial focused test failed because the APIs were absent. Tests cover matching and different packs, missing and duplicate pins, cross-process use and altered-pin detection. The full locked suite passed 99 tests and formatting passed.

This records operator choice, not authenticated authority or fixture quality. Typed-event reviews do not yet enforce the pin and typed-event promotion/rollback do not exist. The chain needs an independently retained checkpoint against full rewrite or final truncation. OS containment and the complete goal remain open; untrusted artifacts must not be executed.

### Typed-event champion metadata

Added schema v24 and explicit local typed-event bootstrap, promotion and rollback with CLI commands. Promotion atomically checks ledger integrity, expected current champion, approved review, complete matching shadow evidence, latest candidate evidence and the domain's exact fixture pin. Transitions are hash-linked and verification reconstructs champion metadata. Rollback returns to the immediately preceding promotion's champion. These operations change metadata only.

Tests cover missing pin, rejection, stale expected champion, promotion, rollback and a newer failed replay blocking promotion. The focused promotion test first failed because the API was absent. The full locked suite passed 100 tests before the additional stale-replay test; the updated focused event-review suite passes eight tests, and formatting passes.

The typed-event host does not yet consume this champion metadata or protect against concurrent transitions during execution. Latest-ID freshness is global to the candidate, not a time or domain policy. Actor and fixture authority are unauthenticated, and ledger history needs an independent checkpoint. OS containment and the complete goal remain open; do not run untrusted artifacts.

### Typed-event host champion activation

Connected `EventHost::process_domain` to the current synthetic champion. It reads a verified champion ID and generation before evaluation, then checks both again in the same immediate transaction that saves the observation and durable stream update. A transition during execution causes the commit to fail without advancing the host's in-memory or durable state. The existing explicit-candidate process method remains available for synthetic experimentation.

Tests demonstrate changed synthetic guest verdicts after promotion and rollback. A controlled worker/second-connection race rolls back during execution and verifies that the stale observation is not inserted and stream generation remains unchanged. The full locked suite passed 102 tests and formatting passed.

The race uses a synthetic shell worker and covers one rollback interleaving, not all failure or concurrency scenarios. Domain selection and actor identity remain unauthenticated; no real workload or user validation is implied. OS containment is unverified and untrusted artifacts must not run; the complete goal remains open.

### Typed-event operator demonstration

Added `sese-event-cycle-demo` for a fixed built-in synthetic typed-event lifecycle: actual replay, fixture selection, durable champion/shadow comparison, displayed review card, explicit approve/reject, promotion and optional rollback. A new database path is required when persistence is requested; existing files are refused. The existing outcome CLI can inspect, resolve and export the resulting review.

Tests cover approval/rollback, rejection, missing input, persistent inspection/resolution/export and refusal to reuse a database. The initial focused test failed because the binary did not exist. The full locked suite passed 105 tests and formatting passed. A local scripted `approve`/`rollback` run printed champion allow -> throttle -> allow and ledger valid; this was a synthetic automation run, not independent operator validation.

This demo does not establish containment, fixture authority, real-world performance, or user value. Actor strings and outcome assertions remain unauthenticated. No untrusted artifact execution is authorized; the broader goal remains open.

### Current source reproducibility check

Copied the current `runtime-spike` source and lockfile without `target` to `/private/tmp/sese-current-check.tgMt4E`. With a new empty Cargo cache and separate target directory, `cargo test --locked` fetched dependencies and passed all 105 tests on Darwin arm64, rustc 1.98.0 and Cargo 1.98.0. From that copy, a scripted typed-event `approve`/`rollback` cycle produced allow -> throttle -> allow and ledger valid. `event-inspect` reopened the persistent database and showed the approval, pending outcome, ordered passing case and both recorded verdicts.

This is a source-copy check, not a clean Git checkout: the workspace has no Git repository. It is not independent human onboarding, a relocated production package, a security or isolation validation, or a real workload. The same containment, authority and training-data limitations remain; the goal is open.

### Durable typed-event stream integrity

Added schema v25 ledger-linked receipts for newly created durable typed-event streams. Each observation, resulting window and receipt commit atomically; verification checks receipt links, generation sequence, preceding window, observation metric and current stored state. Opening a stream checks ledger integrity. Pre-v25 streams without receipts are preserved but fail closed on durable reopen; no unverified backfill or recovery path is provided.

The focused test first demonstrated that a direct valid-float window edit went undetected. After the change it is detected by ledger verification and reopen. The full locked suite passed 106 tests before an added legacy-migration test; all nine updated event-host tests pass, including preservation and rejection of an unlinked legacy stream. Formatting passes.

General ledger verification permits legacy streams with no receipts; their durable open explicitly rejects them. External checkpoints remain necessary against coherent full rewrites or suffix removal. Verification scans history and scalability has not been measured. No sandbox or untrusted execution authorization is established; the goal remains open.

### Typed-event non-executing intake and isolation inventory

Added `event-register` to store bounded typed-event artifact bytes in an existing database, returning a full identity and explicit `executed: false` / `validated: false`. The ABI label is the chosen intake route, not a checked interface. A test first failed because the command was absent, then verified that a looping synthetic module is stored without execution and oversized input is rejected. The full locked suite passed 108 tests and formatting passed.

Read-only local checks found Docker CLI 29.5.2 but no daemon socket, and Podman 6.1.4 with no configured machine or reachable VM. These checks do not assess container security. Existing sandbox-exec attempts failed in this environment. Registration does not authorize running an artifact, and the project still lacks a verified containment boundary; the overall goal remains open.

### Worker process-group cleanup

On macOS and Linux, the supervisor now starts workers in a separate process group, observes leader exit using `waitid` without reaping, and attempts to kill the group before releasing the leader PID. This addresses a known deadline leak while avoiding PID reuse during group cleanup. Other platforms retain the earlier deadline-only behavior. A pinned direct `libc` dependency was added for these two targets.

A controlled test first showed a same-group descendant writing a marker after the deadline. It now passes without the marker. The first implementation rejected successful workers when macOS returned `EPERM` for group cleanup; handling was adjusted to keep cleanup best effort, and the focused supervisor tests and full locked suite pass (109 tests). Formatting passes.

This does not establish containment: descendants can escape the group, permission failures may prevent cleanup, and inherited pipes held by escaped processes may leave detached I/O threads alive. The test covers one shell descendant on this macOS host, not Linux or adversarial process trees. No untrusted execution is authorized; the overall goal remains open.

### Promotion failure paths and zero deadline

Added a typed-event integration test starting with a passing replay, matching shadow observation and approval, then recording a newer failed replay caused respectively by a zero deadline, unsuccessful worker exit or wasmi fuel exhaustion. Each case rejects promotion and retains the current champion. The unsuccessful worker test uses `/usr/bin/false`; it is not evidence of a signal crash or arbitrary OS failure.

The zero-deadline check previously depended on whether the worker exited before the first poll. It now rejects before spawning on both supervisor implementations. A marker-worker test verifies no child starts for a zero deadline. The full locked suite passed 111 tests and formatting passed.

These are synthetic tests, not a verified containment boundary or exhaustive failure audit. Same-group cleanup is best effort; actors and fixture authority remain unauthenticated. Untrusted execution is not authorized and the goal remains open.

### Typed-event promotion atomicity and telemetry gap

Added a file-backed integration test that injects a ledger-insert failure during typed-event promotion. It verifies unchanged champion and transition history, then succeeds after removing the failure, reopens the database, and detects a direct champion-metadata edit. The full locked suite passes 112 tests; formatting passes. This tests a SQLite statement failure, not power loss or all interruption points.

Re-read `bld1.md` and documented a material telemetry discrepancy in `contracts/v1.md` and `IMPLEMENT.md`: the sketch tracks metric mean and measured champion latency p99 separately, total event count and last nonzero host error, while the current synthetic host derives both statistics from event metric, reports bounded-window count and takes last error from the caller. Percentile rules differ too. Existing v25 state must not be silently relabeled as measured latency. Versioned reconciliation and migration remain required, as do verified containment and the other open goal items.

### Measured typed-event state experiment

Added a versioned `SES2` state primitive with separate bounded metric and champion-duration samples, total observation count, floor-index p99 and last nonzero host error. `MeasuredEventHost` exercises it with fixed synthetic workers and records elapsed champion-attempt time (including process startup and failures); state changes only after observation persistence. Measured observations are explicitly tagged, and the existing review API rejects them rather than mixing them with legacy replay semantics.

Focused tests first failed because the new APIs were absent. Tests now cover signed metrics, separate p99, retention and count, failure error propagation, encoded round trip, malformed encoding, and rejection of measured observations by the legacy review path. The full locked suite passed 116 tests and formatting passed.

This path is in memory only and is not connected to durable streams, replay fixtures, promotion or the operator demo. Old metric-only receipts are not reinterpreted. Durations include supervisor overhead and are not performance benchmarks. Verified containment and the full goal remain open; untrusted execution is not authorized.

### Durable measured-event state

Added schema v26 with separate named `SES2` measured streams and hash-linked receipts. A receipt binds the previous and next state to the observation; verification reconstructs the next state from the observation's measured champion duration, metric and error, checks the supplied prior snapshot, and compares the current stream. Updates, observations and receipts commit atomically, with a generation and prior-state check against stale writers. Existing v25 metric-only streams are not converted.

Tests cover restart, stale writers, changed-state detection and rollback when receipt insertion fails. The focused test first failed because durable measured streams did not exist. The first full run exposed a v24 migration test fixture that retained v26 tables; after correcting that fixture, the full locked suite passed 118 tests and formatting passed.

This measured path is not yet connected to replay semantics, review, promotion or the operator demonstration; legacy review continues to reject it. Elapsed duration includes supervisor startup and is not a performance benchmark. The ledger needs external checkpoints, and OS containment remains unverified; no untrusted execution is authorized.

### Replay state semantics

Added schema v27 and a separate measured-v2 replay API. Legacy replay identities retain their existing digest and ledger encoding; measured fixtures use a distinct digest domain, and their ledger evidence binds the semantics. Review checks that replay and observation semantics match, inspection and outcome export identify the version, and measured reviews are barred from promotion until the measured host is integrated with champion activation.

Tests cover successful measured review, rejection of either cross-version pairing, distinct fixture digests, migration of an existing v26 legacy replay without changing its identity, and detection of altered semantics. The full locked suite passed 120 tests before the final export-field addition; the updated 14-test event-review suite passes afterward, and formatting passes.

Measured replay currently accepts caller-provided fixed snapshots; it does not reconstruct them from state history. Measured champion activation, an operator demo for this path, containment and other goal requirements remain incomplete. Untrusted execution remains prohibited.

### Measured-domain champion activation

Connected `MeasuredEventHost::process_domain` to the selected synthetic champion and its generation. A domain's state semantics are derived from its ledger-bound fixture pin; promotion requires matching replay semantics, and each host rejects domains using the other state version. The measured host checks the champion generation again when committing its observation and durable state, so a concurrent transition fails without advancing that state.

Tests cover a measured synthetic replay, review, promotion and rollback with guest verdict changes; cross-version host rejection in both directions; and a controlled rollback during measured-worker execution. The full locked suite passed 122 tests and formatting passed. The race uses a synthetic shell worker and covers one interleaving.

Measured replay cases still use caller-supplied snapshots, and the documented operator demo remains on legacy semantics. Durations include process overhead; no real-world performance or user validation is implied. OS containment, authenticated authority and the remaining goal requirements are unverified; untrusted execution remains prohibited.

### Measured-state operator demonstration

Extended the typed-event cycle demo with `--measured`, selecting measured-v2 replay, a separate durable measured stream, matching review, champion promotion and optional rollback. Legacy behavior remains the default. Tests cover the measured approval/rollback path, persisted review semantics and stream generation, and rejection without promotion. The focused test first failed because the option was absent. The full locked suite passed 124 tests and formatting passed.

A scripted local measured run printed allow -> throttle -> allow and ledger valid. It used built-in synthetic modules and automated choices, not an independent operator or real workload. Measured replay still uses fixed caller-supplied snapshots; durations include process overhead. OS containment, authenticated authority and the broader goal remain incomplete; untrusted artifacts must not be executed.

### Domain-scoped typed-event promotion evidence

Added schema v28 with an optional observation domain. `process_domain` records that domain in the observation and its ledger digest; promotion requires an exact domain match and uses domain-scoped latest observation/review checks. Inspection and outcome export retain the domain. Existing observations migrate as unscoped, remain inspectable, and cannot authorize promotion; no provenance is invented. Candidate replay freshness remains global by candidate.

The new cross-domain test first demonstrated that an alpha observation could promote beta when both shared a champion and candidate. After the fix, beta remains unchanged and alpha can promote. A file-backed check also detects an edited observation domain. The full locked suite passed 125 tests before the additional domain-tamper assertion; the focused updated test passes and formatting passes.

This fixes a local evidence-scoping bug, not actor authentication or fixture authority. An external checkpoint remains necessary against coherent ledger rewrite or final-suffix loss. OS containment and other goal requirements remain open; untrusted execution is prohibited.

### Typed-event window domain scope

A focused test demonstrated that the same durable window could process alpha and then beta, carrying alpha's prior events into beta. Both legacy and measured typed-event hosts now bind their window to the first committed scope: one domain or explicit unscoped processing. Durable hosts recover scope from ledger-linked stream observations and recheck it inside the commit transaction; in-memory hosts also reject scope changes. Existing unscoped histories remain unscoped rather than being assigned a guessed domain.

Tests cover cross-domain rejection before and after reopening, measured-state rejection of cross-domain and unscoped reuse, and unchanged state on refusal. The full locked suite passed 127 tests and formatting passed.

This prevents reuse through these host APIs but does not authenticate domain names or provide a protected external ledger anchor. Older histories with mixed scopes fail to reopen rather than being silently repaired. OS containment and the broader goal remain incomplete; untrusted execution is prohibited.

### Typed-event intake execution guard

Added schema v29 caller-asserted synthetic candidate records, bound to the ledger. `event-register` / `register_event` remain non-executing intake and do not authorize storage-backed typed-event replay, host execution or promotion. Fixed test modules use the explicitly named `register_synthetic_event` API. An existing unclassified artifact cannot be silently reclassified; pre-v29 artifacts remain stored and inspectable but cannot use this synthetic execution path without a separate recovery/design decision.

The focused test first failed because the synthetic API did not exist; it now confirms untrusted intake cannot be replayed or reclassified. The first full run exposed an old test that expected an empty ledger despite the new registration event; its baseline was corrected. A simulated pre-v26 migration fixture was updated to reconstruct the historical chain without new registration events. The full locked suite then passed 128 tests, including preserved legacy replay evidence, and formatting passed.

This is an accidental-use guard, not proof an artifact is synthetic or a security boundary: the caller can assert synthetic status, raw-byte execution APIs and workers remain available for synthetic tests, and scalar/window APIs do not have this guard. Ledger authority is unauthenticated and requires external checkpoints. Verified containment and the full goal remain open; untrusted execution is prohibited.

### Window intake execution guard

Added schema v30 caller-asserted synthetic window candidate records, linked to the ledger. `window-register` and `register_window` remain non-executing intake; storage-backed synthetic replay, host execution, promotion and rollback require a synthetic record. Existing unclassified artifacts remain stored and are not silently reclassified. Built-in demos and tests use the explicitly named `register_synthetic_window` API.

The focused test first failed because the synthetic API did not exist. It now verifies untrusted intake cannot be replayed or reclassified. The full locked suite passed 129 tests before an additional CLI-to-replay assertion; that focused cross-process test passes afterward, and formatting passes.

This is an accidental-use guard, not an authenticated claim or sandbox. The caller can assert synthetic status, raw-byte execution APIs and scalar paths remain available, and legacy unclassified windows require a separate recovery/design decision. Verified containment and the complete goal remain open; untrusted execution is prohibited.

### Scalar intake execution guard

Added schema v31 caller-asserted synthetic scalar candidate records, linked to the ledger. `register` now stores bytes without authorizing the storage-backed synthetic scalar runtime; fixed demos and tests use `register_synthetic_scalar`. Scalar replay, host execution and review require a synthetic record. Existing unclassified artifacts are preserved and cannot be silently reclassified.

A focused test first failed because the synthetic scalar API did not exist; it now verifies intake cannot be replayed or reclassified. The full locked suite passed 130 tests and formatting passed.

All three storage-backed experimental paths now distinguish intake from caller-asserted synthetic use, but this is not authentication or containment. Raw-byte execution APIs and worker binaries remain available for fixed synthetic tests, and callers can assert synthetic status. Old unclassified artifacts have no recovery workflow. Verified isolation and the full goal remain open; untrusted execution is prohibited.

### Synthetic classification integrity checks

Added cross-ABI tests for synthetic candidate record and ledger-event atomicity, forged record detection, deleted record detection, and behavior when intake bytes exactly match an existing synthetic identity. All three caller-asserted classifications fail ledger verification if their record or link is altered, and an injected ledger write failure rolls back candidate and classification insertion. Intake of already-classified identical bytes returns the existing identity without changing its classification; README wording now states that exception explicitly.

The full locked suite passed 133 tests and formatting passed. These tests establish local consistency under the stated threat model, not caller authenticity or resistance to a complete chain rewrite. The low-level raw-byte APIs and workers remain unsuitable for untrusted artifacts. Verified containment and the full goal remain open.

### Typed-event worker provenance

Added schema v32 worker path and SHA-256 provenance to new typed-event replay and observation records. The worker file is read before and after execution; detected changes reject the operation without saving its evidence. Separate ledger events bind provenance to the replay or observation, preserving old ledger encodings. Inspection shows both worker identities, and outcome export includes both hashes. Typed-event promotion now requires present, matching replay and observation worker digests; older records without them remain inspectable but cannot promote.

Tests cover actual provenance inspection, worker-file changes during replay, altered stored hashes, migration preserving older unclassified provenance, and rejecting a review whose replay and observation used different worker files. The mismatch test first showed promotion succeeding, then passed after the gate. The full locked suite passed 135 tests before the final export-field assertion; that focused test passes afterward and formatting passes.

A digest is not worker attestation: the file can change between reads, wrappers can execute other code, and caller-selected workers remain untrusted. File hashing is bounded by size but occurs outside the worker deadline; no end-to-end latency or containment claim is made. Raw-byte APIs and scalar/window paths do not have this provenance feature. Verified isolation and the full goal remain open; untrusted execution is prohibited.

### Fixture-selected worker gate

Typed-event promotion now requires the replay and shadow worker digest to match the worker recorded by the replay used for the domain's fixture selection. The existing ledger-bound fixture pin already references that replay, so no new schema or implicit worker selection was needed. Fixture selections based on older replay evidence without worker provenance cannot authorize promotion.

A focused test first showed that matching replay and shadow results from a wrapper worker could promote despite a different fixture-selection worker. After the gate, promotion fails and the champion remains unchanged. The full locked suite passed 136 tests and formatting passed. An initial edit script failed before changing source; the corrected edit and tests succeeded.

This records a local operator selection, not trusted executable attestation. A wrapper can execute different code, file replacement between checks remains possible, and the worker is not isolated. No untrusted execution is authorized; the full goal remains open.

### Fixture-selected worker in domain processing

Typed-event domain hosts now require the worker digest selected by the domain's fixture before evaluation and recheck it together with champion generation inside the observation commit transaction. Both legacy and measured hosts reject a different worker without advancing state. Direct synthetic APIs remain separate and do not claim this domain gate.

A focused test first reproduced successful domain processing with a different wrapper worker. After the gate it is rejected for both state versions. Existing rollback-race tests were updated to select a fixed wrapper before arming it, and assert the specific stale-champion failure; two older mismatch tests now create unscoped evidence and assert the promotion's specific worker rejection. The first full run exposed those outdated tests; after updating them, the full locked suite passed 137 tests and formatting passed.

The selected digest is caller-chosen provenance, not executable attestation. Wrappers and file replacement between reads remain possible, and the worker is not isolated. No untrusted execution is authorized; the goal remains open.

### Independent ledger-chain verification

Added a standalone Python 3 standard-library verifier that opens a database read-only, recomputes the S-SESE ledger hash chain and checks a separately supplied checkpoint. It reports the current head and explicitly marks evidence bindings as unchecked. Tests cover a valid checkpoint, valid extension, missing checkpoint, modified ledger event, UTF-8 byte lengths, and a linked database-row change that the independent chain-only verifier intentionally does not detect while the Rust storage verifier does.

The focused test initially failed because the script was absent. The full locked suite subsequently reported all 139 tests passed; formatting passes. The suite now invokes `python3` for these tests.

This is independent chain verification, not an independent verifier of every database evidence binding, a signature, or proof of trusted checkpoint custody. Without an external retained checkpoint a coherent rewrite or suffix loss can go undetected. Verified containment and the overall goal remain open; untrusted execution is prohibited.

### Outcome export provenance

Added `outcome_evidence: caller_asserted` to scalar, window and typed-event synthetic outcome exports. Scalar/window rows now also state their existing reward mapping (`validated=1;falsified=-1`) explicitly; event rows continue to assign no reward. All remain `training_ready: false`. This preserves the source sketch's distinction between an observed outcome and an operator assertion rather than labeling assertions as verified reality.

The focused scalar export assertion initially failed because the provenance field was absent. Tests now check provenance and mapping on falsified scalar, validated window and typed-event records. The full locked suite passed 139 tests and formatting passed.

No SFT, DPO or RLVR training-ready export or independent outcome validation is established. Source and actor remain caller supplied; verified containment and the broader goal remain open.

### Independent synthetic-candidate binding checks

Extended the standalone Python chain verifier with optional `--verify-synthetic-candidates`. Within the same read transaction it verifies each scalar, window and event synthetic candidate's artifact digest, ABI, runtime, timestamp and ledger link, including missing or duplicate links. It reports how many classifications it checked while retaining `evidence_bindings_checked: false` for all other evidence.

The focused test first failed because the option did not exist. Tests now check all three candidate types and reject altered classification timestamps and artifact bytes. The full locked suite passed 140 tests and formatting passed.

These are caller-asserted classifications, not proof that artifacts are safe or synthetic. The independent script still does not verify replay, observations, decisions, outcomes or stream bindings; the application verifier covers its implemented bindings. External checkpoint custody, verified isolation and the full goal remain open.

### Independent typed-replay binding checks

Extended the standalone Python verifier with optional `--verify-event-replays`. It independently reconstructs typed replay evidence digests for legacy-v1 and measured-v2 records, checks candidate bytes and ABI, ordered stored case fields, and the corresponding ledger link. It fails closed on missing links. The output reports the number of replay bindings checked while retaining `evidence_bindings_checked: false` for the remaining evidence types.

The focused test first failed because the option did not exist. It now checks both state semantics and detects an altered case result. The full locked suite passed 141 tests and formatting passed.

This verifies that stored replay fields match their ledger event, not that fixture identities were independently recomputed, expected answers are correct, a worker is trustworthy, or execution occurred as claimed. Observation, review, outcome and stream bindings are not independently checked by this script. Containment and the complete goal remain open; untrusted execution is prohibited.

### Independent typed-fixture consistency

Added `--verify-event-fixtures` to the standalone verifier. It first checks typed replay ledger bindings, then independently recomputes versioned fixture digests and checks ordered cases, finite float representations, exact verdict comparisons, errors and run pass status. It reports the number of fixture cases checked while continuing to mark overall evidence verification incomplete.

The focused test first failed because the option did not exist. It now exercises legacy and measured semantics, full-width unsigned IDs, signed-zero mismatch, a worker-error case, and direct rejection of a forged stored pass result. The full locked suite passed 141 tests and formatting passed.

This checks stored internal consistency, not the correctness of expected answers or actual guest execution. It does not independently verify worker, observation, decision, outcome or stream bindings. Verified containment and the full goal remain open; untrusted execution is prohibited.

### Independent typed-worker provenance and test isolation

Added optional `--verify-event-workers` to the standalone verifier. It checks typed replay and observation worker-path/digest ledger links and reports both checked and unrecorded legacy rows. Tests cover replay and observation provenance, changed digest detection, and explicit reporting of a legacy row without provenance. This does not attest or read the historical executable.

The first full run failed with a disk I/O error in `candidate_guard`. Investigation reproduced the error and showed two parallel tests had generated the identical timestamp-based temporary database path. A per-process atomic suffix removed that collision; 30 parallel stress runs passed, followed by the full locked suite passing 142 tests. Formatting passes.

The independent verifier still does not verify all observation, review, outcome or stream bindings. Worker digests remain caller-selected provenance, not trusted execution. External checkpoint custody, OS containment and the full goal remain open; untrusted execution is prohibited.

### Independent typed-observation binding checks

Added `--verify-event-observations` to the independent read-only verifier. It recomputes observation evidence over stored fields, both candidate artifacts and the optional domain, and checks the corresponding ledger event and timestamp. Missing or duplicate links fail closed, including unlinked legacy observations. Worker provenance requires the separate worker option, and overall evidence verification remains incomplete.

The focused test initially failed because the option was absent. It now verifies a recorded observation and rejects changes to its prior state and domain. The full locked suite passed 142 tests; formatting was checked separately.

These checks establish consistency with the retained checkpoint, not correct execution, trusted workers, or checkpoint custody. Independent review, outcome and stream verification and OS containment remain open; do not execute untrusted artifacts.

### Independent typed-review and outcome bindings

Added `--verify-event-reviews` to the independent verifier. It checks typed review fields and resolved outcome records against their ledger events, including links to the underlying replay, observation and review hashes. It also checks all typed replay and observation bindings. Pending reviews remain distinct from resolved outcomes, and the output reports each count separately.

The focused test initially failed because the option was absent. It now covers pending and validated reviews and rejects changed proposal and outcome-source fields. The full locked Rust suite passed 142 tests.

This checks binding consistency against the supplied checkpoint; it does not authenticate actors, validate asserted outcomes, verify worker provenance unless separately requested, or cover every evidence type. Trusted containment and the complete goal remain open; do not execute untrusted artifacts.

### Independent typed-champion history

Added `--verify-event-champions` to reconstruct typed-event bootstrap, promotion and rollback transitions, verify their ledger links and compare the resulting champion and generation with stored state. The check also verifies replay, observation, review and outcome bindings and requires each promotion's referenced review to approve the promoted candidate. Updated stale README text to reflect the existing typed-event workflow.

The new integration test initially failed because the option was absent. It exercises the synthetic approval and rollback demo, checks three transitions, and rejects changes to the stored generation or transition actor. The full locked Rust suite passed 143 tests; formatting passes.

This is not a complete independent enforcement audit of promotion policy, fixture pins, worker provenance, or stream receipts. Actors and checkpoints are not authenticated. Containment and the complete goal remain open; do not execute untrusted artifacts.

### Fixture provenance in scalar and window exports

Extended the existing synthetic scalar and window outcome JSONL rows with the recorded fixture version, digest and replay environment. Export reads those fields from the same ledger-verified transaction and requires the replay to reference the reviewed candidate. Existing schema identifiers are unchanged; the new fields are additive.

The focused scalar test first failed because fixture fields were absent. Tests now compare exported digests to stored replay values and confirm altered fixture digests prevent export for both paths. The full locked Rust suite passed 143 tests; formatting passes.

The fixture digest is recorded and ledger-bound but is not independently recomputed by these export paths. Outcomes are caller assertions, not independently observed reality; the rows remain synthetic and not training-ready. SFT/DPO/RLVR training exports, verified containment and the full goal remain open.

### Typed-event fixture files

Added a versioned JSON fixture format, a bounded non-executing `sese-fixture` inspection command, and an example under `runtime-spike/fixtures/`. Parsing rejects duplicate or unknown fields, invalid cases, unknown state semantics, non-finite numbers, empty packs and files above 1 MiB. The Rust `FixturePack::replay_synthetic` routes parsed cases to the corresponding legacy or measured replay implementation; this remains limited to known synthetic candidates and is not a sandbox. Serde derive is now a direct dependency and the lockfile was updated offline.

The focused tests exercised inspection and malformed inputs, and matched the parsed digest to persisted real synthetic replay evidence under both state semantics. The documented inspection command returned one case with `executed: false`. The full locked Rust suite passed 146 tests; a subsequent focused empty-pack assertion also passed. Formatting passes.

This is fixture preparation, not an operator command to run arbitrary candidates. Fixture validation does not establish expected-answer quality or authority. No container service was available and previous nested macOS sandbox attempts did not establish containment; the overall goal remains open and untrusted execution is prohibited.

### Operator fixture in the synthetic cycle

The typed-event demo now accepts `--fixture FILE`, validates the bounded fixture before creating its database, and runs its cases against the fixed synthetic candidate. It records the supplied fixture version and cases, then continues through the existing fixed-event shadow comparison and explicit review. A fixture with state semantics inconsistent with `--measured` is rejected before database creation; a failed replay is recorded and stops before champion initialization. The fixture reader is shared with the inspection command.

The new integration test initially failed because the demo lacked the option. It now exercises an operator-supplied fixture through approval and rollback, verifies persisted version and case ID, confirms mismatched semantics leave no database, and checks that a failing replay produces no champion or review. The full locked suite passed 147 tests; formatting passes.

The demo guests and shadow event remain fixed synthetic examples. This does not validate fixture authority, provide isolation, or authorize untrusted candidate execution. Training-ready exports, OS containment, and the full goal remain open.

### Local synthetic event sequence

The typed-event demo accepts `--events FILE` for a bounded, versioned JSON sequence of 1–100 caller-provided synthetic events. It validates the whole file and mode-specific metric range before database creation, processes events in file order, prints and persists each champion/shadow comparison, and uses the final observation for review. The post-promotion and rollback demonstrations reuse the final event. An example and command guidance are in the runtime README.

The integration test initially failed because the option was absent. It now confirms two events are persisted in order, the second observes host-owned prior state, the review links the final observation, and an invalid legacy metric is rejected without creating a database. The full locked suite passed 148 tests; formatting passes.

The input is a finite synthetic file, not a live event feed; the demo guests remain fixed. This does not authenticate data origin or validate fixture quality, and no untrusted execution is authorized. Containment, training-ready exports and the full goal remain open.

### Inspecting persisted event comparisons

Added `event-observations DB DOMAIN AFTER_ID` to inspect domain-scoped typed-event observations in ID order. It returns at most 100 rows with inputs, prior state, candidate IDs, results or errors, timings and a continuation cursor. Each page uses one ledger-verified transaction and individually rejects returned observations without valid evidence bindings. The README documents the command and checkpoint limitations.

The extended integration test first failed because the command did not exist. It now checks two persisted inputs and prior state, a cursor starting after the first observation, and rejection after an observation is altered. The full locked suite passed 148 tests; formatting passes. The 101-row continuation boundary has not been exercised by this test.

This inspection does not authenticate actors or checkpoints, establish trusted execution, or replace complete stream-history verification. Untrusted candidates remain prohibited; the full goal is open.

### Evidence shown before the synthetic decision

The typed-event demo now prints the candidate identity and each replay case's input, prior window, expected verdict, actual verdict or error, and pass status before review. It also prints each event's input, prior state, and actual champion and shadow verdicts or errors. Floating-point values include their bit patterns. Failed replay evidence is printed before the demo stops and cannot initialize a champion.

The focused integration test first failed because replay evidence was not displayed. It now checks a supplied fixture's expected and actual verdicts appear before the decision prompt, confirms the observation shows both verdicts, and checks that a failed replay displays the mismatch. The full locked suite passed 148 tests; formatting passes.

The review remains a local synthetic demonstration with fixed guests and caller-selected fixtures, not independent validation or a security boundary. Verified containment, training-ready exports and the overall goal remain open.

### Inspecting failed typed replays

Added `event-replay-inspect DB RUN_ID` to inspect persisted typed-event replays independently of a review. It returns candidate and fixture identities, worker provenance, ordered inputs, expected and actual verdicts or errors, and pass status. It checks ledger linkage, fixture digest and case consistency in one SQLite snapshot, including failed runs.

The focused integration test initially failed because the command was absent. It now retrieves a failed replay without a review and rejects an altered case result. Additional coverage inspects both a passing replay and a worker-error replay. The full locked suite passed 148 tests; formatting passes.

Inspection does not rerun code, validate expected answers, authenticate workers or protect a checkpoint. Verified containment, training-ready exports and the complete goal remain open; untrusted candidate execution remains prohibited.

### Operator-supplied synthetic review card

The typed-event demo accepts `--card FILE` for a bounded, versioned JSON card containing human, bridge and proposal text. The card is validated before database creation, displayed before the explicit decision, and stored with that decision. The default card remains available; an example is included. The text is an operator assertion and does not change or validate the fixed candidate.

The integration test initially failed because the option was absent. It now verifies that the supplied fields are shown before the prompt and retained in the inspected review, and that an empty field is rejected before creating a database. The full locked suite passed 149 tests; formatting passes.

This does not authenticate the operator or prove that the proposal accurately describes the candidate. Verified containment, training-ready exports and the complete goal remain open; untrusted execution remains prohibited.

### Review-time ledger concurrency guard

The typed-event demo now captures a verified ledger head before its explicit decision prompt and records the review only if that exact head remains current in the same immediate transaction as the decision. Any intervening ledger event, including an unrelated one, rejects the decision and leaves the operator to inspect again. The existing review API and CLI remain unchanged; a new guarded API is available to callers that explicitly supply a head.

A two-process integration test first reproduced the race: another process appended a synthetic identity while the prompt waited, and the demo recorded approval and promoted. With the guard, the test confirms the specific stale-review error and no review or promotion. The full locked suite passed 150 tests; formatting passes.

This is an optimistic concurrency check, not a signature or protected checkpoint. Evidence was printed before the head capture and is not yet presented from a single immutable review snapshot. OS containment, training-ready exports and the full goal remain open; untrusted execution is prohibited.

### Verified single-snapshot decision preview

Added a bounded typed-event review preview that reads replay evidence, domain observations and the ledger head in one verified SQLite snapshot. The demo displays that snapshot and its replay and comparison details before the card and decision prompt, then uses its head with the existing guarded review transaction. It rejects a preview with more than 100 domain observations or a mismatch with the completed demo cycle. Failed replays remain inspectable and stop before the preview.

The integration test initially failed because no review snapshot was shown. It now checks the displayed replay, observation and checkpoint against persisted evidence; the existing two-process race test verifies that a later ledger extension blocks the decision. The full locked suite passed 150 tests; formatting passes.

The card is separately supplied operator text, the checkpoint is not authenticated or externally protected, and this only changes the synthetic demo workflow. OS containment, training-ready exports and the complete goal remain open; do not execute untrusted candidates.

### Resume guarded review without rerunning a candidate

Added `event-review-preview DB DOMAIN REPLAY_ID` and `event-review-at ... SEQ HASH` to the outcome CLI. The first assembles the verified bounded preview; the second records an explicit decision only if the exact ledger head remains current and the selected observation belongs to the preview domain. It rejects domains with more than 100 observations and does not itself promote. The older unguarded `event-review` remains available and is identified as such in the README.

The extended stale-review integration test initially failed because the commands did not exist. It now obtains a fresh preview after a concurrent ledger change, confirms the old head and wrong domain are rejected, then records a rejection with the new head without adding a promotion. The full locked suite passed 150 tests; formatting passes.

The caller-supplied head is a concurrency guard, not proof that an operator read the preview, an authenticated identity or an externally protected checkpoint. No untrusted execution is authorized; containment, training-ready exports and the complete goal remain open.

### Reproducible synthetic walkthrough

Added `runtime-spike/ONBOARDING.md` with a sequential build, fixture/event/card inputs, explicit review and rollback, inspection, outcome assertion, export and independent checkpoint verification. The main runtime README links to it. It explains checkpoint custody and the limits of synthetic assertions and worker isolation.

Ran the documented executable sequence on a fresh temporary database with scripted `approve` and `rollback` input. It showed a pending review, recorded a synthetic validated assertion, and exported a row marked `training_ready: false`. The independent verifier returned `chain_valid: true`, `checkpoint_match: true`, three champion transitions, four observations, five worker links and `evidence_bindings_checked: false`. The shell run exited 0; its local artifacts are in `/var/folders/w6/bt1vjqpn7v5dynzljb0jk_300000gn/T/sese-walkthrough.yUvhiv`. `cargo build --locked --manifest-path runtime-spike/Cargo.toml --bins` succeeded. Tool versions: Cargo 1.98.0, rustc 1.98.0, Python 3.15.2. The full 150-test suite passed in the preceding implementation turn; it was not rerun for this documentation-only change.

This was a scripted local demonstration, not an independent fresh operator or a clean Git checkout. The checkpoint remained in the temporary run directory and had no independent custody. OS containment, training-ready exports and the complete goal remain open; untrusted execution is prohibited.

### Caller-asserted synthetic training traces

Schema v33 adds typed-event trace records linked to a review and ledger event. `event-trace-record` accepts a bounded JSON file with an explicit synthetic prompt, response, actor and source; an optional preference must include distinct preferred and rejected texts and its own source. `event-trace-inspect` returns the stored record and current outcome while marking provenance `caller_asserted_synthetic` and `training_ready: false`. The system does not infer a preference or execute a model. The contract and README state that these records do not by themselves qualify for SFT, DPO or RLVR training export.

The focused test first failed because the CLI commands did not exist. Tests now cover an explicit pair, no pair, invalid or incomplete pairs, empty input, missing review, rollback on an injected ledger failure, pending and caller-asserted validated outcomes, and changed-trace detection. A v32-to-v33 migration test preserves an existing candidate. The first full suite exposed three older migration fixtures that rewind the schema without dropping the new table; those test reconstructions were corrected. The subsequent full locked suite passed 152 tests; formatting passes.

Recorded text, pair comparability, synthetic status and outcome remain caller assertions, not verified model behavior or observed reality. No training-ready SFT/DPO/RLVR export is implemented, the independent Python verifier does not yet verify these trace bindings, and verified containment and the full goal remain open. Do not store sensitive text or execute untrusted artifacts.

### Independent synthetic-trace verification

Added `--verify-event-traces` to the standalone Python verifier. It recomputes each trace's ledger payload and review link, checks explicit pair structure and UTF-8 byte limits, and also verifies typed replay, observation, review and outcome bindings. It reports the number of trace bindings checked while retaining `evidence_bindings_checked: false` for other evidence. The option requires schema v33.

The focused test initially failed because the option was absent. It now verifies a trace with an explicit pair and a trace without one, including a non-ASCII prompt, and rejects altered preference text, changed response text and a deleted trace with an orphaned ledger event. The full locked suite passed 152 tests; formatting passes.

Matching stored fields to a retained checkpoint does not authenticate the actor, source, model response, preference comparability or outcome. No training-ready export or verified worker containment is established, and the complete goal remains open.

### Synthetic trace-to-candidate comparison

Added `event-trace-compare-wat DB TRACE_ID`. For a caller-asserted synthetic trace, it verifies the stored ledger state, interprets the response as WAT, and compares the resulting binary digest to the reviewed candidate. It reports `match`, `mismatch`, `invalid_wat` or oversized output and explicitly reports `executed: false` and `training_ready: false`. The comparison is derived on demand; it is not an independently attested receipt.

The focused test first failed because the command did not exist. It now covers an exact matching synthetic response, different valid WAT, non-WAT text, unchanged replay count, and rejection when ledger-bound trace data is altered. The full locked suite passed 152 tests; formatting passes.

Byte identity under the locked parser does not prove model authorship, semantic equivalence, fixture quality or outcome truth. Parsing is not an OS sandbox; use only known synthetic text. Training-ready exports, verified containment and the complete goal remain open.

### Current-schema training eligibility audit

Added `event-trace-eligibility DB TRACE_ID`, a versioned, ledger-verified audit that reports distinct SFT, DPO and typed-event RLVR blockers. It distinguishes pending, caller-asserted validated and caller-asserted falsified outcomes; reports missing or unverified preference comparability for DPO; and reports the missing typed-event reward derivation for RLVR. All current synthetic traces remain ineligible because provenance and resolved outcomes are caller assertions. The contract explicitly scopes this as negative gates, not a complete positive eligibility policy.

The focused test initially failed because the command was absent. Tests now cover pending, validated-as-asserted and falsified outcomes, present and absent preference pairs, separate format blockers and rejection after trace tampering. The full locked suite passed 153 tests; formatting passes.

No positive training eligibility or SFT/DPO/RLVR training export has been implemented. Independent outcome provenance, trusted worker containment and the complete goal remain open; untrusted execution is prohibited.

### Post-promotion synthetic fixture comparison

Added `event-followup-check DB REVIEW_ID OBSERVATION_ID CASE_ID`. In one ledger-verified snapshot it checks that a passing fixture and approval preceded an active promotion, that the observation used the reviewed candidate, domain and recorded worker digest after that promotion, and that its input and prior window match a predeclared fixture case. It compares the actual champion verdict to that case and distinguishes match, verdict mismatch, input mismatch and worker error. The check is read-only, records no outcome and leaves training eligibility unchanged.

The integration test initially failed because the command did not exist. It now runs a fixed synthetic candidate through approval and rollback, compares a post-promotion observation to a predeclared case, rejects pre-decision and post-rollback observations and altered evidence, and confirms no outcome is created. A focused comparator test covers verdict mismatch, worker error and signed-zero input differences. The full locked suite passed 155 tests; formatting passes.

This is a repeated comparison against the same caller-selected synthetic fixture, not an independent fixture or observed real-world outcome. The worker digest is provenance, not attestation. Positive training eligibility, verified containment and the complete goal remain open; untrusted execution is prohibited.

### Preselected separate synthetic follow-up case

Schema v34 adds ledger-bound selections of a separate synthetic follow-up replay case for a domain and candidate. Selection requires matching candidate, state semantics and recorded worker digest; the fixture digest must differ and its selected input must not duplicate a reviewed fixture input. The selected check requires the selection event to precede the review, then compares the later post-promotion observation with that case. The review preview includes the selected fixture and its prior replay. The demo can accept a separate fixture and case, and the CLI can select and check an existing synthetic replay.

The new integration test initially failed because the demo lacked the option. Once wired in, promotion failed because the separate replay became the newest candidate replay. Tracing the gate showed that rerunning the reviewed fixture after selection preserves the existing freshness rule without excluding failed replays; the focused test and the existing newer-failed-replay test then passed. Additional tests reject renamed duplicate inputs, late selections and altered pin records, and exercise v33-to-v34 migration with existing traces. The full locked suite passed 157 tests; formatting passes.

The follow-up fixture is executed and visible before review, selected by the same local operator and not a blinded or independently authoritative holdout. Its result records no outcome and grants no training eligibility. The standalone Python verifier does not yet check follow-up selection bindings. Verified containment and the complete goal remain open; untrusted execution is prohibited.

### Durable synthetic follow-up receipt

Schema v35 adds a ledger-linked receipt for the preselected synthetic follow-up comparison. `event-followup-record` recomputes the selected comparison and atomically records its review, observation, selection and result; duplicate receipts are rejected. `event-followup-inspect` verifies the ledger links and recomputes the comparison before returning it. The receipt does not resolve an outcome or grant training eligibility.

The focused integration test initially failed because the commands were absent. It now covers recording and reopening a receipt, duplicate rejection, altered-status detection, rollback after an injected ledger-write failure, and a reconstructed v34-to-v35 migration preserving the selection. The full locked suite passed 157 tests; formatting passes.

This remains a caller-selected, non-blinded synthetic fixture comparison, not a real-world outcome or an authenticated receipt. The standalone Python verifier does not yet independently check selection or receipt bindings. Verified containment, positive training eligibility and the complete goal remain open; untrusted execution is prohibited.

### Independent follow-up receipt verification

Added `--verify-event-followups` to the standalone verifier. It checks typed fixture-selection, follow-up-selection and receipt ledger bindings; validates the linked replay fixtures, observations, reviews, champion history and recorded worker links; checks pre-decision and post-promotion chronology and distinct inputs; and independently recomputes the comparison. Separate counts are reported and overall `evidence_bindings_checked` remains false.

The focused integration test initially failed because the option was absent. It now verifies a stored receipt against a retained checkpoint and rejects altered receipt and selection records. A separate comparator test covers match, verdict mismatch, worker error, signed-zero input mismatch and measured-state parsing. The full locked suite passed 158 tests; formatting passes.

This establishes consistency with the supplied checkpoint for the implemented checks, not trusted workers, independent fixture authority, an authenticated actor or a real-world outcome. The synthetic fixtures remain visible before review. Positive training eligibility, verified containment and the complete goal remain open.

### Outcome exports with synthetic follow-up evidence

Resolved typed-event outcome evidence rows now include their linked synthetic follow-up receipts, with each comparison recomputed in the export's verified snapshot. Pending reviews remain excluded; missing receipts produce an empty array, and invalid evidence or more than 1,000 receipts for one review fails the export. Caller-asserted outcome provenance remains separate from the synthetic comparison, and every row remains `training_ready: false`.

The focused integration test first failed because the export omitted receipts. It now checks pending exclusion, inclusion after a caller assertion, fixture and comparison provenance, empty arrays for reviews without receipts, and fail-closed export after receipt tampering. The full locked suite passed 158 tests; formatting passes.

The follow-up check and receipt output schemas changed to v2: `records_outcome: false` replaces the ambiguous `outcome_recorded: false`, so consumers of v1 must update. This describes the comparison operation, not whether a separate outcome exists. The evidence remains non-blinded synthetic data and does not validate real-world outcomes or establish training eligibility. Verified containment and the complete goal remain open.

### Follow-up evidence walkthrough

Updated the onboarding walkthrough to select the separate synthetic follow-up case before review, record and inspect the post-promotion receipt, export it with a caller-asserted outcome, and independently verify its ledger bindings. The instructions identify the observation and receipt IDs for a fresh run and explain that the fixture is visible before review.

Built the locked binaries and ran the synthetic sequence on a fresh temporary database, with scripted approval and rollback. The comparison matched, the resolved export included one receipt and remained `training_ready: false`, and the independent verifier reported a valid chain and checkpoint, one follow-up selection and one receipt. The checkpoint remained beside the database; this is not independent custody. Inspection commands and the documented verifier options also succeeded. No full suite was rerun for this documentation-only change.

This scripted run is not a fresh-operator study or a clean checkout. It does not establish independent outcomes, positive training eligibility, worker isolation, or completion of the goal.

### Independent stream receipt binding checks

Added `--verify-event-stream-receipts` to the Python verifier. It checks legacy and measured receipt records against their ledger payloads, including the prior and next state byte digests and referenced observation event hashes, and checks observation bindings. It reports separate receipt counts and rejects missing, duplicate, orphaned or altered links.

The focused test initially failed because the option was absent. It now exercises both stream types using synthetic demo executions and rejects altered state bytes and deleted receipt rows. The full locked suite passed 159 tests; Rust formatting was applied.

This option verifies record bindings, not state-transition semantics or the current stream state; the application verifier performs its supported state checks separately. A protected external checkpoint is required for the stated tamper-detection scope. Verified containment, positive training eligibility and the full goal remain open. Docker's daemon remains unavailable and Podman has no configured machine; no untrusted candidate was executed.

### Independent stream history reconstruction

Extended the Python stream receipt option to reconstruct legacy metric windows and measured SES2 states from receipt sequences and observations, including bounded rollover, measured duration, snapshot values and last-error behavior. It compares receipt-backed histories with current state and reports `event_streams_without_receipts` for preserved legacy streams that lack receipt history. Measured streams without receipts fail verification.

The integration test initially exposed that a changed current generation passed the prior binding-only check. It now rejects that change for both formats and reports a legacy stream without receipts. Two focused Python tests exercise rollover, measured failure and subsequent success, and invalid transitions. The full locked Rust suite passed 159 tests before the final reporting-field change; the updated integration test and both Python tests passed afterward. Rust formatting was applied.

This verifies recorded transition consistency against the supplied checkpoint, not observation authenticity, worker isolation, checkpoint custody or real-world outcomes. Positive training eligibility, verified containment and the full goal remain open.

### Follow-up export snapshot verification

Removed repeated whole-database verification from nested follow-up receipt and replay inspection during outcome export. The public check, receipt inspection and recording operations now verify their transaction snapshot before calling private snapshot-only helpers; export verifies once before iterating. Each comparison and replay binding is still recomputed in that snapshot, and receipt recording retains its transactional ledger insertion checks.

Focused follow-up and review tests passed. The follow-up integration test now also checks that corrupted receipt evidence blocks check, inspect, record and export, with export producing no partial output. The full locked Rust suite passed 159 tests after the change; Rust formatting was applied. No performance benchmark was run, and other per-row evidence checks remain.

Verified containment, positive training eligibility and the complete goal remain open; no untrusted candidates were executed.

### Temporary Git snapshot and clone verification

Created a temporary Git repository containing the current `runtime-spike` source, fixtures, lockfile, tests and documentation, then cloned it into a separate directory. The temporary commit was `65beb1570978f11c7bdbd619a1ec4a3e077ed73f`; the clone initially had no changes. From that clone, the documented locked binary build and full Rust suite passed (159 tests), as did the two Python stream-history tests. Cargo 1.98.0, rustc 1.98.0 and Python 3.15.2 were used, with the existing Cargo dependency cache.

The cloned binaries also completed the synthetic walkthrough with scripted approval and rollback. The resolved export contained a matching follow-up receipt and remained caller-asserted and not training-ready. The independent verifier reported a valid chain and checkpoint, one follow-up receipt, four stream receipts and no legacy streams without receipts. The checkpoint was kept beside the temporary database, not under independent custody.

The snapshot and clone are at `/var/folders/w6/bt1vjqpn7v5dynzljb0jk_300000gn/T/sese-clone-proof.NsKPVc`; the demo artifacts are at `/var/folders/w6/bt1vjqpn7v5dynzljb0jk_300000gn/T/sese-clone-demo.Zk9WsS`. This is a locally constructed snapshot, not a checkout of a maintained repository or release. It does not re-test an empty dependency cache, establish independent operator usability or verify containment. The full goal remains open.

### Empty-cache checkout build and demonstration

Cloned temporary source snapshot `65beb1570978f11c7bdbd619a1ec4a3e077ed73f` again into `/var/folders/w6/bt1vjqpn7v5dynzljb0jk_300000gn/T/sese-empty-cache.PwqdSq`. With newly created empty Cargo home and target directories, Cargo 1.98.0 downloaded the locked dependencies from crates.io, built all binaries, and passed the full 159-test Rust suite. The two Python stream-history tests also passed under Python 3.15.2. The separate build directory required using its binary paths for the demonstration.

Using those newly built binaries, the scripted synthetic approval and rollback, follow-up receipt, asserted outcome and export succeeded. The independent verifier reported `chain_valid: true`, `checkpoint_match: true`, one follow-up receipt and four legacy stream receipts, with overall evidence bindings still incomplete. The export remained caller-asserted and not training-ready. Demo artifacts are at `/var/folders/w6/bt1vjqpn7v5dynzljb0jk_300000gn/T/sese-empty-cache-demo.OlqyfT`.

This verifies dependency acquisition and execution on this machine for a locally constructed snapshot, not a maintained release checkout, a fresh human operator, independent checkpoint custody, worker isolation or positive training eligibility. No untrusted candidate was executed; the goal remains open.

### Domain decision and transition history

Added `sese-outcome event-history DB DOMAIN AFTER_SEQ [LIMIT]` for a ledger-ordered, domain-scoped view of synthetic reviews, caller-asserted outcomes and champion bootstrap, promotion and rollback transitions. Pages contain 1–100 entries and include a cursor and the verified snapshot's ledger head. The onboarding and runtime README describe its use and multi-page limitations.

The focused integration test exercises approval, rollback, pending and resolved outcomes, cursor paging, domain filtering, invalid bounds and fail-closed behavior after outcome tampering. The full locked Rust suite passed 160 tests; Rust formatting was applied.

Each page is a separate snapshot; its head is not an independently protected checkpoint, and the view does not authenticate actors or assertions. Docker and Podman remain unavailable for containment verification on this host. No untrusted candidate was executed; positive training eligibility and the complete goal remain open.

### Bounded Unix supervisor pipe handling

Reproduced a supervisor resource leak with a controlled macOS test worker: after the leader exited, a descendant escaped the process group and retained stdout; the supervisor timed out, but its detached reader kept the pipe open. The test failed with the descendant reporting `open`. Replaced Unix supervisor I/O threads with bounded nonblocking pipe operations in the deadline loop, so pipe handles close on return. Process-group cleanup remains best effort.

The focused regression now reports `closed`; an additional test covers a worker that does not read a large request. The full locked Rust suite passed 162 tests on macOS arm64, including the existing worker, timeout and promotion failure tests. Rust formatting was applied. The shared Linux path was not exercised on Linux.

Closing parent pipes does not kill escaped descendants, establish OS isolation, attest a worker, or authorize untrusted artifacts. Other platforms retain threaded supervision. Containment, positive training eligibility and the full goal remain open.

### Proposed positive training-export gates

Added `contracts/training-export-v1.md` to specify a proposed positive synthetic path for SFT, DPO and RLVR separately: source-attested trace bytes, independently configured outcome authority, explicit comparable DPO pairs, a versioned RLVR reward rule, trusted checkpoint input, provenance fields and positive/negative verification cases. The contract distinguishes key control from truth and independence and does not infer real-world outcomes from synthetic fixture comparisons.

This is documentation only; no attestation, trust configuration, positive eligibility implementation, training JSONL serializer or new training-ready data was added. The proposal still needs concrete key custody, revocation and source-adapter decisions and implementation. Docker's daemon remains unavailable and Podman has no configured machine. No untrusted candidate was run; verified containment and the full goal remain open. No tests were rerun for this documentation-only change.

### Evidence signature primitive

Added a standalone Rust Ed25519 verifier with a versioned, length-delimited message binding authority ID, authority role, payload length and SHA-256 digest. The verifier rejects weak public keys, invalid signatures, role mismatches, revoked caller-provided authorities and invalid input bounds. Pinned `ed25519-dalek` 2.2.0 and updated the lockfile; no private keys are stored by this code.

The focused test first failed because the module did not exist. It now passes an RFC 8032 signature vector and rejects altered messages, signatures, authority IDs, roles and revoked authorities. The full locked Rust suite passed 164 tests on macOS arm64; Rust formatting was applied. The initial dependency fetch could not write the sandbox-protected standard Cargo cache; an approved fetch completed successfully.

This is a primitive only: authority configuration is supplied by the caller, and no trusted store, signed evidence import, revocation management, outcome authentication, positive training eligibility or training export is implemented. A signature proves neither independence nor truth. Verified containment and the full goal remain open; no untrusted candidate was executed.

### Typed synthetic attestation payloads

Added versioned canonical encodings and role-specific signature checks for synthetic trace and outcome claims. The payloads bind exact text, source, method, timestamp, review and replay IDs, candidate and fixture digests, and the review ledger hash. They cannot encode non-synthetic claims. The byte layout is documented in `contracts/training-export-v1.md`.

The focused test first failed because the typed payloads did not exist. It now verifies signed synthetic claims and rejects changed content, review, replay, fixture, ledger hash, role and outcome, as well as malformed references and timestamps. The full locked Rust suite passed 165 tests; additional focused reference-tampering assertions passed afterward, and formatting passes.

These are signed caller-provided claims: the code does not check references against the database, verify observation timing, import or persist signatures, establish authority custody or independence, or grant training eligibility. There is no typed preference attestation. Worker containment and the complete goal remain open.

### Checkpointed synthetic attestation links

Added a read-only Rust check that verifies typed trace and outcome signatures with caller-provided authorities, requires different public keys, and matches their review, replay, candidate, fixture, trace and outcome fields against a ledger-verified database snapshot and a supplied checkpoint covering the trace and outcome. It checks asserted time ordering against recorded times and recomputes replay provenance. The response explicitly leaves authority configuration, observer independence and outcome truth unverified, with `training_ready: false`.

The focused test initially failed because the API was absent. It now accepts matching signed synthetic claims and rejects stale or mismatched checkpoints, changed recorded fields, invalid and revoked signatures, falsified status mismatch, pre-decision observation time and database tampering. The full locked Rust suite passed 166 tests before the final tampering assertion; the updated focused test passed afterward. Rust formatting was applied.

This does not authenticate checkpoint custody, establish authority independence, verify the observation evidence reference, import or persist attestations, or implement training export. Signatures and local time ordering do not prove observation truth. Worker containment and the full goal remain open; no untrusted candidate was executed.

### Digest-pinned authority configuration

Added a bounded `sese-authority-set-v1` JSON parser that checks exact bytes against a caller-supplied SHA-256 digest. It rejects unknown fields or roles, duplicate IDs or public keys, weak keys, invalid bounds and mismatched pins; lookup excludes revoked signers. A new Rust link-check entry point resolves named trace and outcome signers from this set before applying the existing checkpointed claim checks. It reports the configuration digest while retaining `authority_config_verified: false` and `training_ready: false`.

The parser and integration tests first failed because the APIs were absent. Focused tests now cover positive resolution and rejection of invalid configuration, altered pins, role mismatches, unknown and revoked signers. The full locked Rust suite passed 167 tests; Rust formatting was applied.

The caller still provides the bytes, digest and checkpoint; their independent custody and the authorities' real-world independence are not established. No authority file loader, attestation persistence, signed preference, positive training export or OS containment is implemented. No untrusted candidate was executed; the complete goal remains open.

### Synthetic attestation inspection CLI

Added `event-attestation-check DB BUNDLE_JSON AUTHORITIES_JSON AUTHORITIES_SHA256 CHECKPOINT_SEQ CHECKPOINT_HASH`. It loads bounded JSON inputs, rejects unknown fields and malformed lowercase hexadecimal signatures or pins, resolves the configured signers, and checks signed synthetic claims against checkpointed records. The bundle format is documented in `contracts/training-export-v1.md`; the CLI does not create signatures or store the bundle. As with other commands, opening an older database can run schema migrations.

The focused integration test first failed because the command did not exist. It now verifies a signed synthetic bundle across processes and rejects a changed claim, wrong configuration pin or checkpoint, malformed signature, unknown field and oversized bundle without partial output. The full locked Rust suite passed 167 tests; Rust formatting was applied.

The caller supplies all authority and checkpoint inputs, so custody and independence remain unverified. The check does not validate outcome truth, persist attestations, implement signed preferences or grant training eligibility. Worker containment and the complete goal remain open; no untrusted candidate was executed.

### Synthetic preference attestation and inspection

Added a domain-separated synthetic preference payload binding the common review and replay reference, trace ID and ledger hash, prompt, explicit preferred and rejected responses, source, comparability explanation and review time. A role-specific signature check and `event-preference-check` CLI compare the signed claim to the stored pair and a supplied checkpoint. They report `comparability_verified: false` and `training_ready: false`; they do not combine the preference with a trusted trace or outcome for DPO export.

Focused tests initially failed because the payload, record-link check and CLI were absent. They now reject modified prompts, pairs, trace links, signatures and time ordering; the CLI checks a pinned reviewer configuration. The first full-suite run failed intermittently in an unrelated supervisor regression: a repeated run exposed a marker-file creation/write race. The test was changed to wait for nonempty contents without changing its `closed` assertion. A second full run again reported that test failed, but its failure details were truncated; 15 subsequent supervisor-suite runs and a captured full rerun passed. The captured full locked suite passed 168 tests; formatting was applied. The second intermittent failure has not been conclusively explained.

The preference remains a synthetic signed assertion, not independent proof of comparability or reviewer identity. Attestation persistence, positive training eligibility, worker containment and the complete goal remain open; no untrusted candidate was executed.

### Supervisor regression handshake

Inspected the retained artifacts from the unexplained supervisor-test failure: its result file was missing, so the controlled descendant had not completed its reporting step. The exact reason remains unproven. Replaced the test's fixed startup/sleep timing with a readiness signal after the descendant leaves the process group and a release signal after the supervisor returns. The child has a bounded self-exit if release never arrives. The test still requires a deadline failure and a closed parent pipe.

The revised test passed five consecutive runs on the current implementation. As a negative control, the same test was run in a separate temporary checkout of the earlier threaded supervisor and failed on the intended assertion (`open` instead of `closed`). The full locked suite passed 168 tests; output was captured in `/private/tmp/sese-handshake-suite.log`. Formatting was applied. This reduces test timing ambiguity but does not prove the precise cause of the earlier missing result.

The supervisor still cannot terminate descendants that escape its process group, and this check is not an OS sandbox. Positive training eligibility, verified containment and the complete goal remain open; no untrusted candidates were executed.

### Durable synthetic attestation receipts

Schema v36 adds ledger-linked receipts retaining canonical synthetic trace and outcome payloads, their signatures, signer IDs, authority-configuration digest and the supplied checkpoint. The record operation verifies the signatures and record links within the same write transaction, then commits the receipt and ledger event atomically. Inspection checks ledger bindings but explicitly does not recheck signatures; a separate recheck uses the caller-supplied, digest-matching authority set. Both keep `training_ready: false`. The CLI exposes record, inspect and recheck commands.

The focused test initially failed because receipt APIs were absent. It now covers a v35-to-v36 migration with existing evidence, injected ledger-write rollback, duplicate rejection, reopen, payload tampering, deleted-receipt detection and cross-process record/inspect/recheck. Historical migration fixtures were updated to reconstruct their actual prior schemas. The full locked Rust suite passed 168 tests; output is in `/private/tmp/sese-attestation-receipt-suite.log`, and formatting was applied.

The original authority set remains external and must be retained to recheck signatures. A changed configuration, including revocation, fails the original digest match and requires separate review. The receipt does not establish checkpoint custody, reviewer independence, observation truth or training eligibility. The standalone Python verifier does not yet check receipt bindings. Signed preference persistence, positive training exports, verified worker containment and the full goal remain open; no untrusted candidate was executed.

### Independent attestation receipt binding checks

Added `--verify-event-attestations` to the standalone Python verifier. It independently recomputes each synthetic receipt's payload and signature hashes, checks its ledger binding and checkpoint chronology, and verifies the related typed replay, observation, review, outcome and trace bindings. Its result explicitly reports `attestation_signatures_checked: false` and leaves overall evidence verification incomplete.

The focused integration test first failed because the option was absent. It now accepts a checkpointed receipt and rejects altered payload bytes, altered signature bytes, a changed checkpoint reference and a deleted receipt. The full locked Rust suite passed 168 tests; captured output is in `/private/tmp/sese-independent-attestations.log`, and formatting was applied.

The Python check does not cryptographically verify signatures or authenticate authority configuration, checkpoint custody, independence or outcome truth. Signature rechecking remains a separate Rust operation requiring the original digest-pinned authority set. Signed preference persistence, positive training eligibility, verified worker containment and the full goal remain open.

### Durable synthetic preference receipts

Schema v37 adds an atomic ledger-linked receipt for a checked synthetic preference payload and signature, reviewer ID, pinned authority digest and checkpoint. The CLI can record, inspect and recheck it; inspection does not recheck signatures, while recheck requires the original digest-pinned authority set. The independent Python `--verify-event-attestations` option now also verifies preference-receipt bindings and reports a separate count. It does not verify signatures.

The focused tests first failed because the receipt and verifier APIs were absent. They now cover v35/v36 migrations, injected ledger-write rollback, duplicate rejection, reopening, altered payload and signature, deleted-receipt detection, and cross-process record/inspect/recheck. Historical migration fixtures reconstruct their earlier schemas. The full locked Rust suite passed 168 tests; after adding an explicit v36 Python compatibility assertion, the updated focused test passed. Formatting was applied. Full output is in `/private/tmp/sese-pref-receipt-suite.log`.

The receipt preserves a synthetic signed assertion; it does not establish comparability, reviewer independence, authority or checkpoint custody, or DPO eligibility. Changed authority configurations, including revocations, fail the original digest match and require separate review. Positive training export, verified worker containment and the full goal remain open; no untrusted candidate was executed.

### Combined synthetic evidence check

Added `event-evidence-check` and a Rust API that check one retained trace/outcome attestation receipt together with one preference receipt in a single ledger-verified snapshot. The supplied checkpoint must cover both receipt events, both receipts must reference the same trace and authority-configuration digest, and all three role-specific signatures are rechecked. The result reports the outcome as caller asserted and explicitly leaves custody, independence, comparability, truth and training eligibility unverified.

Focused tests initially failed because the combined API and CLI were absent. They now cover valid combined synthetic evidence, a stale checkpoint, missing receipt, changed configuration, altered receipt and a separately valid preference receipt for a different trace. The full locked Rust suite passed 168 tests before the final tampering assertion; the updated focused test passed afterward. Formatting was applied; full output is in `/private/tmp/sese-evidence-package-suite.log`.

This is an inspection check, not a DPO export or a positive eligibility decision. Authority and checkpoint custody, real observer independence and outcome truth remain external and unverified. Worker containment and the complete goal remain open; no untrusted candidate was executed.

### Synthetic signed-evidence inspection export

Added `event-evidence-export` for one resolved synthetic trace/outcome and preference package. In one verified snapshot it applies the combined receipt, checkpoint and signature checks, then exports candidate and fixture provenance, decision, caller-asserted trace and outcome, explicit pair, and exact canonical signed payloads and signatures. The JSONL row is labeled `inspection_only`, synthetic and `training_ready: false`; failed checks emit no row.

The focused test first failed because the export API was absent. It now checks the row and exact signed bytes, successful CLI output, stale-checkpoint rejection without partial output, and rejection of a separately valid preference receipt for another trace. The full locked Rust suite passed 168 tests before the final exact-byte assertions; the updated focused test passed afterward. Formatting was applied; captured full output is in `/private/tmp/sese-signed-evidence-export.log`.

This is not SFT, DPO or RLVR training export and does not establish model authorship, authority custody, reviewer independence, comparability or outcome truth. Positive training eligibility, verified worker containment and the full goal remain open; no untrusted candidate was executed.

### Verification of synthetic evidence exports

Added `event-evidence-verify` to recompute an exported synthetic inspection record from the checkpointed database and pinned authority configuration, recheck signatures, and compare exact bytes. It requires the original database and authority set; reformatted JSON or a changed newline is rejected. A successful result remains `training_ready: false` and does not establish checkpoint or authority custody or outcome truth.

The independent Python ledger verifier now accepts `--verify-event-export` and compares the inspection record with ledger-bound database rows and the supplied receipt-covering checkpoint. It rejects altered values, duplicate keys and incorrect types while permitting JSON reformatting. It explicitly reports that signatures and signed-payload semantics were not checked; it does not establish authority or checkpoint custody or outcome truth. The focused integration test and full `cargo test` suite passed.

The focused test first failed because the command was absent. It now accepts the original export and rejects changed content, wrong pins or checkpoints, multiple rows, missing newline and oversized input without partial output. The test also exposed a FIFO open blocking before regular-file validation; the macOS/Linux bounded-file helper now opens nonblocking and the FIFO regression passes on macOS. The full locked Rust suite passed 168 tests, with output in `/private/tmp/sese-evidence-export-verification.log`; formatting was applied. The Linux path was not exercised on Linux.

This compares against the same application's database and serializer, not an independent export verifier. It is not a training export or proof of outcome truth. Positive training eligibility, verified worker containment and the complete goal remain open; no untrusted candidate was executed.

### Independent canonical attestation checks

The Python `--verify-event-export` path now independently decodes the canonical trace, outcome and preference payloads and compares their references, recorded fields and timestamp ordering against the verified snapshot. It rejects malformed fields, invalid encodings, trailing bytes and inconsistent records. Its result reports `attestation_payload_record_links_checked: true` and continues to report `attestation_signatures_checked: false`; it does not authenticate timestamps, signatures, custody, independence, comparability or outcome truth. The prior paragraph describes the earlier Rust-only verifier; the separate Python export verifier now exists.

The focused integration assertion first failed because this check was absent, then passed. Four Python verifier tests and the full `cargo test --locked` suite passed; full Rust output is in `/private/tmp/sese-payload-check-suite.log`. `cargo fmt --check` passed. Positive training eligibility and OS containment remain open; no untrusted candidate was executed.

### Synthetic authority relationship declarations

Added a bounded, exact-digest-pinned synthetic trust-policy input bound to the authority configuration. It records an operator and the asserted relationships between configured trace, outcome and optional preference authorities. The policy parser rejects unknown or revoked roles, ambiguous source/observer pairs, missing explanations, changed pins and mismatched configuration. `event-evidence-policy-check` rechecks the retained signed evidence and covering checkpoint in one database snapshot and then matches the signers to a policy declaration. The result remains `training_ready: false` and explicitly does not establish policy custody, independence, comparability or outcome truth.

Focused parser and integration tests initially failed because the API and CLI were absent; they now pass, including undeclared-reviewer and wrong-policy-pin rejection. The full `cargo test --locked` suite passed; output is in `/private/tmp/sese-trust-policy-suite.log`. `cargo fmt --check` passed. This does not implement positive SFT, DPO or RLVR eligibility or provide an OS containment boundary; those and the full goal remain open. No untrusted candidate was executed.

### Common evidence without a preference pair

Added `event-common-policy-check` and a matching Rust API. They check a retained trace/outcome receipt, signature keys and operator source/observer declaration against a caller-pinned authority set, policy and receipt-covering checkpoint in a single ledger-verified snapshot. They do not require or infer a preference pair. Results remain caller-asserted synthetic evidence and `training_ready: false`; policy custody, independence, timestamp authenticity and outcome truth are not established.

The focused test failed first for the missing API and CLI, then passed with a recorded trace that has no preference pair, as well as stale-checkpoint and modified-signature rejection. The full `cargo test --locked` suite passed; output is in `/private/tmp/sese-common-policy-suite.log`, and formatting was applied. Positive training eligibility, format-specific exports and verified OS containment remain open. No untrusted candidate was executed.

### Pinned synthetic reward rule

Added a bounded, digest-pinned `sese-synthetic-reward-rule-v1` document and `event-reward-preview`. The supported version maps validated to +1 and falsified to -1, rejects pending or unknown statuses, and carries the exact rule digest, version and declared actor/source alongside the checkpointed signed-evidence inspection. A fixture illustrates the format. The caller must retain the rule file and pin separately; the rule is not stored in the database. The result is a preview of a caller-asserted synthetic outcome, remains `training_ready: false`, and does not establish custody or outcome truth.

Focused tests first failed for the missing rule, API and CLI. They now verify both signs, pending/unknown statuses, unsupported versions and mappings, invalid provenance, wrong pins, stale checkpoints and no output on CLI failure. The full `cargo test --locked` suite passed; output is in `/private/tmp/sese-reward-rule-suite.log`, and formatting was applied. This is not a positive RLVR training export; positive eligibility, format-specific exports and verified OS containment remain open. No untrusted candidate was executed.

### Follow-up and outcome consistency

Added `event-followup-outcome-check` to recompute a preselected synthetic fixture receipt and compare its result with a caller-asserted outcome recorded later. The check requires a caller-supplied checkpoint covering both events; `match` corresponds to `validated`, and `verdict_mismatch` to `falsified`. It reports agreement or contradiction and rejects pending outcomes, stale or incorrect checkpoints, and comparisons that cannot support a status. It does not establish that the operator used the receipt, fixture independence, checkpoint custody, real-world outcome truth or training eligibility.

The focused test first failed for the missing command. Focused tests now cover agreement, contradiction, pending outcomes and wrong or stale checkpoints. The full `cargo test --locked` suite passed; output is in `/private/tmp/sese-followup-outcome-suite.log`, and formatting was applied. Positive training exports and verified OS containment remain open; no untrusted candidate was executed.

### Signed synthetic follow-up evidence

Added `event-signed-followup-check` and a Rust API that verify a signed trace and outcome claim against a recomputed, preselected synthetic follow-up receipt in one checkpointed database snapshot. The observer's signed method and evidence reference must identify the exact receipt and its ledger hash; the claimed observation time must follow receipt creation and precede outcome recording, and the signed status must match the actual fixture comparison. The command does not retain the supplied bundle. It does not authenticate timestamp, authority or checkpoint custody, establish observer or fixture independence, prove a real-world outcome, or authorize training.

The focused test first failed for the missing API and command. It now covers a matching synthetic claim, altered receipt reference, unrelated method, premature observation time, incorrect checkpoint or authority pin, contradictory signed status and altered signature. The full `cargo test --locked` suite passed; output is in `/private/tmp/sese-signed-followup-suite.log`, and formatting was applied. Positive training export and verified OS containment remain open; no untrusted candidate was executed.

### Retained signed follow-up claims

Added `event-signed-followup-record` to validate the signed follow-up link and atomically store its canonical claims, signatures and ledger-linked attestation receipt. `event-signed-followup-recheck` requires the original bundle and a checkpoint covering that receipt; it compares the exact retained claim bytes and signatures and recomputes the follow-up in a single verified snapshot. The shared attestation receipt format does not itself prove that the follow-up check occurred, so consumers must run the recheck. The result remains synthetic and `training_ready: false`.

The focused test failed first for the missing API and commands, and later exposed a CLI failure to compare the bundle trace ID; all now pass. Coverage includes injected ledger-write rollback, duplicate rejection, restart, changed payload, stale checkpoint, wrong bundle trace ID and CLI recording/rechecking. The full `cargo test --locked` suite passed; output is in `/private/tmp/sese-signed-followup-record-suite.log`, and formatting was applied. Operator and fixture independence, authority/checkpoint custody, positive training export and verified OS containment remain open; no untrusted candidate was executed.

### Direct recheck of retained follow-up claims

Added strict Rust decoding of canonical synthetic trace and outcome claims and `event-retained-followup-check`. It reconstructs the claims from a ledger-verified attestation receipt, checks the supplied authority-set digest and receipt-covering checkpoint, verifies signatures and recorded fields, and recomputes the signed follow-up link in one snapshot. Unlike the bundle comparison command, it does not require the original bundle. The authority configuration and checkpoint still require independent retention; the result remains `training_ready: false`.

Focused tests first failed for the missing decoder, API and CLI. They now cover canonical round trips, truncation, trailing bytes, invalid status, UTF-8, hash and field length, retained-payload tampering, stale checkpoints and cross-process rechecking. The full `cargo test --locked` suite passed; output is in `/private/tmp/sese-retained-followup-suite.log`, and formatting was applied. This does not establish independence or positive training eligibility, implement format-specific training exports, or provide verified OS containment. No untrusted candidate was executed.

### Temporary Git clone and operator walkthrough

The workspace has no Git repository, so a temporary Git repository was constructed from the current project files, excluding build outputs and caches, and cloned locally. The first clone exposed untracked Python cache files; `runtime-spike/.gitignore` now ignores `__pycache__/` and Python bytecode. A second clone of the updated temporary snapshot (`376d04fab5f1a312bc856947f19dca7a67d6b4b6`) built with `cargo build --locked --manifest-path runtime-spike/Cargo.toml --bins`, passed the documented `cargo test --locked --manifest-path runtime-spike/Cargo.toml` suite (175 tests, zero failures), and passed four Python verifier tests. Runtime source in the clone matched the current workspace excluding build and cache outputs; `git status --short` was empty after the checks.

The documented synthetic walkthrough was scripted from that clone: approve, promote, roll back, inspect, record a follow-up, assert an outcome, export and verify a checkpoint. The export remained caller asserted and `training_ready: false`; the checkpoint was retained only in the temporary run directory. Build and test logs and per-command walkthrough outputs are under `/private/tmp/sese-git-check.6ydEJd/`. Tool versions: rustc/cargo 1.98.0, Python 3.15.2, Git 2.56.0, Darwin arm64. Dependencies came from the existing local cache.

This verifies a clean clone of a locally constructed snapshot, not a clean clone of an authoritative upstream repository or an independent human onboarding study. Positive training exports, authority custody and verified OS containment remain open. No untrusted candidate was executed.

### Operator selection of built-in synthetic candidates

The typed-event cycle demo now accepts `--candidate throttle|quarantine`, defaulting to the existing throttle candidate. Each choice runs a fixed built-in synthetic Wasm module against its corresponding default replay case, and the review, shadow comparison, promotion and rollback show the selected behavior. User-supplied fixtures still run unchanged; an incompatible fixture fails replay before champion initialization. Unknown candidate names fail before database creation. Arbitrary user code remains unsupported and must not be run without the missing containment boundary.

The focused test first failed because selection was absent. It now covers quarantine replay, approval, promotion and rollback; a mismatched throttle fixture; rejection in measured mode without promotion; and an unknown choice without creating a database. The full `cargo test --locked` suite passed; output is in `/private/tmp/sese-candidate-choice-suite.log`, and formatting was applied. Documentation includes a separate synthetic candidate walkthrough. Positive training exports and verified OS containment remain open; no untrusted candidate was executed.

### Separate synthetic initialization and replay commands

Added `event-init`, `event-builtin-register` and `event-builtin-replay` so an operator can initialize a new database and run a real fixture against a named fixed synthetic candidate across separate processes. The demo and commands share the same built-in module registry. Initialization refuses an existing path; replay requires the exact registered, ledger-classified synthetic module, uses the sibling `event-worker`, and records case results. A failed replay prints its result and exits nonzero; it does not initialize a champion. The commands do not accept arbitrary candidate code, create shadow observations or provide a sandbox.

The focused integration test initially failed because these commands did not exist. It now covers initialization and existing-path rejection, replay before registration, successful replay and inspection, a mismatched candidate/fixture, unknown names, and rejection of identical but unclassified artifact bytes without execution. Existing demo tests also passed. The full `cargo test --locked` suite passed; output is in `/private/tmp/sese-operator-replay-suite.log`, and formatting was applied. A separate operator shadow-observation path, positive training exports and verified OS containment remain open. No untrusted candidate was executed.

### Separate synthetic shadow observations

Added `event-builtin-observe`, which processes one synthetic event against a named built-in expected champion and shadow using the sibling worker and a durable stream. It supports legacy and measured state semantics, rejects a mismatched champion or semantics before execution, and retains the host's generation check at commit. It accepts no arbitrary candidate code. Stream capacity is fixed at 32; the legacy path supplies `last_error=0`. A worker failure is recorded and returns a nonzero exit status; stream initialization may precede a later failure.

The cross-process test first failed because the command was absent. It now covers initialization, fixed candidate registration and replay, fixture pinning, bootstrap, two observations across restarts, preview and explicit approval, promotion, another observation, rollback, caller-asserted outcome and history/export. Separate tests cover measured-state persistence, wrong semantics without stream creation, invalid metrics, wrong champions, and rejection of a non-built-in champion before execution. The full `cargo test --locked` suite passed; output is in `/private/tmp/sese-operator-observe-suite.log`, and formatting was applied. Positive training exports and verified OS containment remain open; no untrusted candidate was executed.

### Fail-closed fixture selection and bootstrap

The manual operator workflow exposed that `event-pin` accepted a failed typed-event replay and bootstrap checked only that a pin existed. New selections now require a verified passing replay, and bootstrap rechecks the pinned replay in its transaction. Failed runs remain inspectable. Existing failed pins are preserved for inspection but cannot bootstrap a new champion; no history is rewritten.

The focused integration test first reproduced the accepted failed pin. It now verifies failed-pin and bootstrap rejection with no champion, and reconstructs a valid legacy ledger-bound failed pin to verify that bootstrap also rejects older records. The operator and event-review suites and full `cargo test --locked` suite passed; full output is in `/private/tmp/sese-failed-fixture-pin-suite.log`, and formatting was applied. This strengthens a synthetic workflow gate but does not establish worker containment, fixture quality or positive training eligibility. No untrusted candidate was executed.

### Manual operator walkthrough

Added `runtime-spike/MANUAL_CYCLE.md` with separate commands for initialization, built-in registration, replay, fixture selection, shadow observation, preview, explicit approve/reject, optional promotion and rollback, caller-asserted outcome, history, export and checkpoint verification. The guide obtains IDs from command output and preserves the explicit human choices. README and onboarding link to it.

The documented shell blocks were extracted and run against fresh local databases. A scripted rejection/skip run finished with only bootstrap, a rejected review and no outcome; an independent approval/rollback/validated run finished with bootstrap, promotion and rollback, and a caller-asserted validated outcome. Both reached the independent ledger verifier. Logs are `/private/tmp/sese-manual-reject.log` and `/private/tmp/sese-manual-approve-comparison.log`; this is scripted verification, not a fresh-operator study.

An initial approval run remained live at `event-init` without creating its database. Two separate initialization probes and the later sequential approval run succeeded. A privileged sample of the stalled PID showed only macOS `_dyld_start` before application code; the underlying reason remains unknown. The specific diagnostic process was terminated after sampling and returned exit 143; its database had not been created. No code fix or claim of root cause was made. Positive training exports and verified OS containment remain open; no untrusted candidate was executed.

## Conditional synthetic training exports

Added `event-training-export` for one selected SFT, DPO or RLVR row. In one verified snapshot it rechecks retained signed claims, a covering checkpoint and a recomputed preselected synthetic follow-up, and requires a pinned authority set, operator trust declaration and explicit synthetic-trust acceptance. DPO additionally checks an explicit signed pair, reviewer declaration and review time; RLVR requires a pinned versioned reward rule. Failed checks emit no row and return a structured exclusion report. Updated the contract and runtime README with invocation and trust limits.

`cargo test --locked` passed 179 tests across 42 test binaries; `cargo fmt --check` passed. Focused tests exercised all three formats, changed pins or payloads, missing acceptance and preference, stale checkpoint and future-dated preference. Test authorities and declarations are controlled in the same test process. These checks do not establish independent authority or checkpoint custody, operator identity, observer or fixture independence, pair comparability, model authorship or real-world outcome truth. Conditional rows explicitly preserve those limitations. A real falsified follow-up producing a negative RLVR export and the full positive/negative verification matrix remain untested. Verified OS containment, untrusted execution and the overall goal remain open.

## Failed follow-up selection gate

A controlled synthetic integration case exposed that `event-followup-pin` accepted an already-failed replay, although the demo rejected it, and promotion did not recheck that selection. Reject failed follow-up replay selection and reverify existing selected follow-ups during promotion. A regression test reconstructs a ledger-bound selection accepted by the older behavior, confirms promotion fails, and checks that the champion remains unchanged. The normal demo and selector also reject the failed replay. This gate does not establish fixture independence or authorize untrusted execution.

The new regression first failed against the previous selector, then passed after the fix. Focused follow-up and signed-follow-up tests passed; `cargo test --locked` passed 180 tests across 43 test binaries, with output in `/private/tmp/sese-followup-gate-suite.log`, and `cargo fmt --check` passed. The attempted negative outcome was based on a fixture known to fail before approval; it is not independent follow-up evidence and is now rejected. An end-to-end negative RLVR export through an admissible follow-up remains unverified. Verified OS containment and the overall goal remain open.

## Approval ordering for selected follow-ups

A synthetic cross-process regression exposed that a passing follow-up could be selected after an approval and that approval could still promote the candidate. Promotion now checks the verified ledger ordering: any selected follow-up must precede the approval used for promotion. The test first reproduced the bypass; it now confirms rejection leaves the champion unchanged and that a subsequent new approval can promote. Existing late selections remain inspectable. The rule verifies ordering, not whether a reviewer actually read the selection.

Focused late-selection, failed-selection and signed-follow-up tests passed. `cargo test --locked` passed 181 tests across 44 test binaries; its output is `/private/tmp/sese-followup-order-suite.log`. `cargo fmt --check` passed. No untrusted candidate was run. OS containment, independent review and the overall goal remain open.

## Independent follow-up promotion audit

The Python verifier's `--verify-event-followups` option now also audits the current follow-up promotion gates against recorded history: approval must precede promotion, and any selection already present at promotion must precede approval and reference a passing replay. It reports `event_followup_promotion_gates_checked`; a selection made after promotion does not retroactively change that transition. Historical transitions accepted by older versions can therefore fail this optional current-policy audit. The audit does not prove fixture independence, reviewer attention, checkpoint custody or a security boundary.

Six focused Python tests cover passing, failing, late, missing and post-promotion cases. An integration test constructs a coherent ledger-bound legacy transition with a late selection and confirms the independent verifier rejects it; another confirms a valid real synthetic cycle passes and reports one checked gate. `python3 -m unittest discover -s scripts -p 'test_*.py'` passed 10 tests. `cargo test --locked` passed 181 tests across 44 test binaries, output `/private/tmp/sese-followup-audit-suite.log`; `cargo fmt --check` passed. No untrusted candidate was executed. Verified OS containment, the negative RLVR end-to-end path and the overall goal remain open.

## Synthetic negative RLVR path

Added a cross-process synthetic fault-injection test for a follow-up selected and passing before approval. After promotion, a known test wrapper forwards the fixed synthetic guest to the real worker but deliberately changes its returned score when a test marker is present. The persisted post-promotion observation produces a recomputed `verdict_mismatch`; a signed synthetic falsified outcome tied to that receipt exports an RLVR reward of -1 through the API and CLI. With a separately signed explicit preference receipt, both SFT and DPO CLI exports reject the falsified outcome without emitting a row. The same wrapper bytes are used throughout, demonstrating that a file digest does not attest external dependencies or behavior.

The focused test passed, and `cargo test --locked` passed 182 tests across 45 test binaries, output `/private/tmp/sese-negative-rlvr-suite.log`; `cargo fmt --check` passed. The wrapper, marker and signing keys are all test-controlled; the result is not independent observation, real-world outcome validation, or proof of worker trust. The wrapper uses `/usr/bin/python3` and was exercised on this macOS host only. The full exclusion matrix, verified OS containment, independent operator study and overall goal remain open. No untrusted guest was run.

## Pending exclusion and read-only training export

Extended the signed synthetic follow-up integration test to attempt recording otherwise valid signed claims while the review is pending. Recording fails without creating an attestation receipt, and SFT, DPO and RLVR CLI requests each emit no row and report `not_exported`. Resolution is then recorded and the existing positive-path checks continue. This verifies pending exclusion through the attestation prerequisite; it does not establish authority custody or independent outcomes.

A separate regression reproduced that `event-training-export` initialized an existing empty database before rejecting a request. The CLI now opens schema v37 with SQLite read-only flags without running migrations; older databases must be migrated separately. Tests cover an empty file, an older schema and retained data, and rejection of writes through the read-only storage handle. Positive and falsified export tests pass in the new mode. `cargo test --locked` passed 184 tests across 46 test binaries, output `/private/tmp/sese-readonly-training-suite.log`; after that suite started, an additional old-schema CLI assertion was added and its focused test passed. `cargo fmt --check` passed. The full exclusion matrix and verified OS isolation remain open; no untrusted guest was run.

## Training trace chronology

A regression demonstrated that conditional SFT export accepted a trace first recorded after the outcome when its signed capture time was backdated. Conditional SFT, DPO and RLVR exports now require the trace ledger event to precede the outcome event and the signed capture time to be no later than the signed observation time. Generic signed-evidence inspection still permits late records. The tests cover backdating, a valid signed DPO pair on a late trace, and a trace recorded before outcome resolution but claiming capture after observation. They also preserve the existing positive exports and synthetic negative RLVR path. Ledger ordering does not authenticate the source's timestamp or prove that the text caused the decision.

The focused regression failed before the gate and passed afterward. `cargo test --locked` passed 184 tests across 46 test binaries, output `/private/tmp/sese-trace-order-suite.log`; `cargo fmt --check` passed. Verified OS containment, independently authenticated evidence, the full exclusion matrix and the overall goal remain open. No untrusted guest was run.

## Guided synthetic training demonstration

Added `sese-training-demo` and an onboarding section for a fresh, interactive local cycle. The demo requires a literal synthetic-trust acceptance and the existing explicit approve/reject decision; rejection emits no examples. After approval it records a synthetic fixture comparison and signs a checked-in explicit synthetic trace and preference pair using fixed public test keys. It writes the database, fixtures, authority set, policy, rule, checkpoint manifest and SFT/DPO/RLVR examples in a newly created directory. Since all keys and roles are controlled by the demonstration process, saved examples are explicitly marked `demonstration_only`, `demonstration_only_ineligible` and not eligible for training. The lower-level exporter can reproduce their underlying contents but its conditional label does not establish independence for these test keys.

The focused scripted integration test covers approval, rejection, missing acceptance, existing-directory refusal, all three examples and an independent ledger-verifier run; it compares the saved rows to fresh CLI exports after applying the demonstration-only labels. `cargo test --locked` passed 185 tests across 48 test binaries, output `/private/tmp/sese-training-demo-final-suite.log`; `cargo fmt --check` passed. This is a scripted test, not a fresh human operator study; test keys, fixture quality, observer independence and checkpoint custody are not verified. The demo does not provide OS containment or authorize untrusted guests. The full goal remains open.

## Explicit policy gate for synthetic training export

The guided demo exposed an inconsistency: its saved examples were marked ineligible, but its public test-key policy could be passed to `event-training-export` and produce a row labeled conditionally eligible. A regression first reproduced that bypass. Synthetic trust policies now require an explicit Boolean `training_export_allowed: true` for SFT, DPO or RLVR export; absent or false denies export. Inspection checks still accept non-export policies. Separate Rust preview methods require a non-export policy and return `inspection_only` rows marked ineligible. The guided demo uses a false policy and those previews; all three CLI export attempts with that policy emit no row. Saved rows disclose whether the policy permitted training export.

Existing policies without the flag continue to parse for inspection but fail closed for export; operators must deliberately revise and re-pin their policy to enable conditional export. A policy and pin are caller-controlled, so the flag does not establish identity, independent custody, observer independence or outcome truth. Tests cover the demo bypass, missing flag, explicit permission, false-policy preview, invalid flag type and the existing positive and synthetic negative export paths. `cargo test --locked` passed 185 tests across 48 test binaries, output `/private/tmp/sese-policy-permission-suite.log`; `cargo fmt --check` passed. Verified OS containment and the overall goal remain open; no untrusted guest was run.

## Optional independent signature verification

The Python ledger verifier now offers `--verify-event-signatures AUTHORITIES_JSON --authority-sha256 DIGEST`, using the optional PyCA `cryptography` package. It parses a bounded regular authority file against its exact digest, rejects duplicate authorities and the use of revoked authorities, checks each retained synthetic trace, outcome and preference signature against its role and receipt configuration, and requires the supplied checkpoint to cover all checked receipts. The report includes the number of signatures checked and explicitly reports that strict weak-key validation was not performed. It separately reports whether canonical payload fields were compared to database records; signature checking alone does not do that. Core ledger verification has no new mandatory dependency.

Focused Python tests cover valid signatures, altered payloads, role/configuration mismatch, missing or revoked authority and duplicate identities. A synthetic demo integration verifies three signatures and rejects an incorrect authority pin and a checkpoint predating the receipts; the stale-checkpoint case first reproduced acceptance before the fix. PyCA cryptography 50.0.2 was present on this macOS host; the test skips the optional check when it is absent. `python3 -m unittest discover -s scripts -p 'test_*.py'` passed 14 tests. `cargo test --locked` passed 185 tests across 48 test binaries, output `/private/tmp/sese-python-signature-suite.log`; a final report-field assertion was added while that suite was running and its focused test passed afterward. `cargo fmt --check` passed.

This check has not been shown equivalent to Rust's strict Ed25519 verification and does not reject weak keys with the same rule. Neither implementation establishes authority or checkpoint custody, independent control, authentic timestamps, real-world outcome truth, or OS containment. The full goal remains open; no untrusted guest was run.

## Current snapshot clean-clone verification

Because the workspace root is not a Git repository, created a temporary local Git snapshot of the current source (excluding generated `target`, Python caches and `.DS_Store`) and cloned it into `/private/tmp/sese-snapshot-current.h5DgER/clone`. The temporary snapshot commit is `7d155da4f2032203375da539a21e3dceb458bd46`; it is not an authoritative upstream commit. The clone had a clean Git status. With an empty, separate Cargo cache, the documented `cargo build --locked --manifest-path runtime-spike/Cargo.toml --bins` fetched dependencies from crates.io and passed; `cargo test --locked --manifest-path runtime-spike/Cargo.toml` passed 185 tests across 48 test binaries. Python verifier tests passed 14 tests. Cargo.lock SHA-256 was `b700cef02dd431110cfb7bafc674db7c144f097b898b4d3eb8e8f4a4c1644478`. Logs and outputs are under `/private/tmp/sese-snapshot-current.h5DgER/`.

From that clone, a scripted approval/rollback walkthrough recorded a pending then caller-asserted validated synthetic outcome. The inspection export retained one follow-up receipt and `training_ready: false`; the independent verifier reported a matching checkpoint and valid chain. A separate scripted synthetic training preview produced all three demonstration-only ineligible examples. The optional Python signature check verified three signatures with the local test authorities while explicitly reporting strict weak-key and signed-payload-to-record checks as not performed. Checkpoints and public test keys stayed beside their databases; this is not independent custody or real-world validation. Tool versions were rustc 1.98.0, Cargo 1.98.0, Python 3.15.2 and Git 2.56.0 on macOS arm64. This is a clean clone of a locally constructed snapshot, not an upstream checkout or independent human onboarding study. Verified OS containment and the full goal remain open; no untrusted guest was run.

## Independent signed-payload record checks

The optional Python signature-verification path now also decodes every retained synthetic trace, outcome and preference payload and compares its common review/replay/candidate/fixture references, content, trace link and asserted time ordering to the corresponding database records. Trace/outcome claims do not require a preference receipt; standalone preference claims may be checked before an outcome exists. The report includes `attestation_payload_record_links_count`. A preference review time later than its receipt is rejected by this optional audit. The single inspection-export verification remains available separately.

Focused Python tests cover no-preference records, altered content and references, inconsistent asserted times, missing outcomes and truncated payloads. The synthetic demo integration now checks that all three signed payloads were compared to records; its assertion failed before the new check and passes afterward. `python3 -m unittest discover -s scripts -p 'test_*.py'` passed 20 tests; `cargo test --locked` passed 185 tests across 48 test binaries, output `/private/tmp/sese-independent-payload-suite.log`; `cargo fmt --check` passed.

This verifies consistency with local records, not authenticity of signed timestamps, actual model origin, reviewer independence, underlying observation or the truth of an evidence reference. The optional PyCA signature verifier is not demonstrated equivalent to Rust's strict verifier. Verified OS containment, independent custody and the full goal remain open; no untrusted guest was run.

## Independent signed follow-up reference audit

When both `--verify-event-signatures` and `--verify-event-followups` are requested, the Python verifier now matches signed `synthetic-followup-receipt-v1` outcome claims to the separately recomputed local receipt. It checks the receipt ID and hash, matching review and outcome status, receipt-before-outcome ledger order and claimed observation time. It reports `signed_followup_claims_checked` and separately counts `other_outcome_methods_not_checked`; the latter are not represented as verified follow-up claims. The check runs only after the existing receipt recomputation and signature/payload checks. It does not establish that the fixture, worker, observer, timestamps or checkpoint are independently trustworthy.

Python tests cover matching references, altered hashes, contradictory results, wrong review, premature timestamps, incorrect sequence and other methods. The demo integration and controlled synthetic falsified RLVR integration each pass the combined independent check; the demo assertion failed before wiring the new audit and passed after. `python3 -m unittest discover -s scripts -p 'test_*.py'` passed 22 tests; `cargo test --locked` passed 185 tests across 48 test binaries, output `/private/tmp/sese-signed-followup-audit-suite.log`; `cargo fmt --check` passed. PyCA remains optional and is not shown equivalent to Rust strict verification. Verified OS containment, independent custody and the full goal remain open; no untrusted guest was run.

## macOS worker descriptor isolation improvement

A controlled synthetic worker demonstrated that the previous macOS `Command` launch inherited an unrelated descriptor whose close-on-exec flag was clear. The macOS supervisor now uses `posix_spawn` with `POSIX_SPAWN_CLOEXEC_DEFAULT`, explicitly supplies only standard input/output and null stderr, and preserves an empty environment, a separate process group, bounded nonblocking I/O and best-effort cleanup. Its path is used directly without PATH search. Linux retains the previous launch implementation and may still inherit unrelated non-close-on-exec descriptors.

The descriptor regression failed with the previous launch and passes with the new one. A second test showed that raw socket pairs exposed reverse-direction channels; an initial attempt to close both endpoints' unused halves failed on macOS, so the implementation now half-closes the worker ends and the directionality and existing protocol tests pass. Focused supervisor, typed-event, host and replay tests passed, including nonexistent executable, timeout and descendant cleanup cases. `cargo test --locked` passed 188 tests across 48 test binaries, output `/private/tmp/sese-macos-spawn-final-suite.log`; `cargo fmt --check` passed. Earlier Python verifier tests also passed 22 tests before the final directionality change, which does not touch Python.

This reduces inherited-descriptor exposure on the tested macOS host; it is not an OS sandbox, process-memory limit, filesystem/network restriction, proof against escaped descendants, or authorization to run untrusted code. The macOS launch uses Unix sockets instead of pipes and is a platform-specific implementation; Linux descriptor closure remains unimplemented and untested. Verified containment and the full goal remain open. No untrusted guest was run.

## Wasmi parser and compilation limits

The shared interpreter now enables the pinned Wasmi 0.40.0 `EnforcedLimits::strict()` profile before module parsing. This adds structural parser/compilation limits to the existing module byte cap, fuel and store limits for scalar, window and typed-event paths. It may reject otherwise valid modules; it does not limit all host-process memory or provide an OS sandbox.

A controlled synthetic module with 1,001 globals was accepted before this change; the new boundary test verifies that 1,000 passes and 1,001 is rejected. A second test verifies that 32 function parameters pass and 33 are rejected. `cargo test --locked` passed 190 tests across 48 test binaries, output `/private/tmp/sese-wasmi-parser-limits-suite.log`; `cargo fmt --check` passed. The tests exercise fixed generated synthetic modules only. Linux descriptor inheritance, OS containment and the overall goal remain open; no untrusted guest was run.

## Fail-closed Wasm growth denial

A controlled typed-event regression showed that a synthetic guest could ignore a denied `memory.grow`, return a valid verdict and produce a passing newer replay. The shared Wasmi store now traps on denied memory or table growth, so this replay records a worker error and the earlier approval cannot promote the candidate; the champion remains unchanged. This intentionally differs from normal Wasm `-1` growth-failure behavior and may reject guests that recover from such failure. Growth within the configured memory limit remains supported.

The promotion regression failed before enabling the trap and passed afterward. Focused runtime tests cover permitted memory growth, denied memory growth and denied table growth even when the guest drops the result. `cargo test --locked` passed 191 tests across 48 test binaries, output `/private/tmp/sese-growth-fail-closed-suite.log`; `cargo fmt --check` passed. This constrains Wasmi store growth only, not all host allocations or OS memory use. Verified OS containment and the full goal remain open; no untrusted guest was run.

## Oversized memory-growth correction

The preceding statement that denied growth traps was too broad: published Wasmi 0.40.0 returns `-1` before calling the limiter when the requested page count is unrepresentable. A synthetic guest could discard it and return a valid verdict; the new regression failed before the fix. A locally vendored, documented Wasmi patch now presents this request to the limiter. Tests verify it traps under this host's limits, retains ordinary `-1` behavior without a limiter, and blocks promotion after a newer failed typed-event replay. Permitted growth continues to pass. Scalar and window replay metadata now identify the patch; typed-event replay metadata still identifies only the synthetic environment, leaving runtime-version provenance and historical evidence separation unresolved.

`cargo fmt --check` and `cargo test --locked --offline` passed (193 tests across 48 test binaries; `/private/tmp/sese-oversized-growth-suite.log`). The vendored dependency produces compiler warnings and requires maintenance on upgrades. This is synthetic test evidence, not OS containment; no untrusted guest was run. Independent operator onboarding, a clean authoritative checkout, and verified isolation remain open.

## Replay runtime provenance correction

The preceding claim that scalar and window replay metadata identify the patched runtime was not supported: both paths execute a caller-supplied worker, and a regression using `/usr/bin/false` showed that even its failed replay was labeled as patched Wasmi. New records now state that the runtime is unverified. Scalar and window exports retain historical environment labels without rewriting ledger-bound evidence and add `runtime_provenance_verified: false` for both old and new records. Existing typed-event replay and observation worker digests are ledger-bound and checked for equality during promotion, but they identify recorded files, not their runtime or trustworthiness; mapping historical digests to builds remains open.

The new regression failed before the change and passed after it. Focused replay, review and window tests passed; `cargo fmt --check` and `cargo test --locked --offline` passed 194 tests across 48 test binaries (`/private/tmp/sese-runtime-provenance-suite.log`). The local Wasmi dependency still emits compiler warnings. No untrusted guest was run; verified OS containment, independent onboarding and a clean authoritative checkout remain open.

## Local process-limit feasibility probe

The previous turn changed replay provenance and verified the suite, so it was progress. On Darwin 27.2 arm64, a disposable `/usr/bin/python3` process successfully set `RLIMIT_CPU` to one second and a controlled spin exited with shell status 152. Separate disposable processes used `setrlimit` via `ctypes`: `RLIMIT_AS` and `RLIMIT_DATA` each returned `-1` with `errno 22` at both 64 MiB and 4 GiB; Python reports `RLIMIT_AS` and `RLIMIT_RSS` as the same resource. This is an environment-specific probe, not a full platform assessment. Docker still cannot connect to its configured daemon; `podman machine list --format json` returns `[]`. Apple's archived documentation describes RSS and DATA limits more narrowly than a universal allocation cap; see `contracts/runtime-assessment.md`.

No worker was changed and no untrusted code was run. A CPU limit alone would not meet the goal's process-memory or filesystem/network containment requirements. The next isolation implementation needs a testable supported boundary and must fail closed when unavailable; the existing synthetic worker is not that boundary. Independent operator onboarding and an authoritative clean checkout also remain open.

## Vendored-runtime fresh snapshot verification

The preceding process-limit probe was progress because it established host-specific failures relevant to selecting isolation. Apple documents an XPC service or separate helper app for capabilities distinct from the parent; the existing CLI has neither, and this option is not implemented or validated. See `contracts/runtime-assessment.md`.

Created temporary Git snapshot `9d82eba59b923173fccb2cec0388b8d799245999` from the current source, excluding build outputs and caches, and cloned it to `/private/tmp/sese-current-snapshot-v_hdrnrr/clone`. With a separate initially empty Cargo cache, the documented locked binary build passed and `cargo test --locked` passed 194 tests across 48 test binaries. The clone's 22 Python verifier tests passed. Cargo.lock SHA-256 was `118f0b9fe56c401055f55ce909f2193ef73f1548de13aac1ba7e330b49a03e15`; logs are in `/private/tmp/sese-current-snapshot-v_hdrnrr/`. The clone's Git status was clean after the build and tests.

From the clone, a scripted onboarding run used the documented fixture, card and follow-up case, approved and rolled back, recorded a synthetic follow-up and caller-asserted validated outcome, and produced one synthetic, non-training-ready export. The independent verifier reported a valid chain and matching checkpoint, while reporting overall evidence bindings unchecked. The checkpoint and database were kept together; this is not independent custody. This is a temporary local snapshot rather than an authoritative upstream checkout, and a scripted run rather than a fresh human operator study. The local dependency emitted compiler warnings. No untrusted candidate was run. Verified OS isolation and the full goal remain open.

## Built-in worker CPU ceiling

The previous turn verified a new clean local snapshot and onboarding script, so it was progress. The scalar, window and typed-event built-in synthetic workers now set an `RLIMIT_CPU` hard ceiling of 10 seconds before reading input on macOS and Linux. The soft and hard limits are never raised above inherited values, and failure to read or set them makes the worker exit. This is a per-process CPU-time ceiling, independent of the existing wall-clock deadline; it does not apply to caller-supplied executable paths.

A child-process regression failed against the initial no-op and passed after implementation. It reads the installed limit, verifies an inherited one-second limit is retained, and checks that the kernel terminates a CPU-bound test child under that tighter limit on this macOS host. This tests the shared limit function and kernel behavior, not every possible worker failure. Focused runtime and supervisor tests passed, as did `cargo fmt --check` and `cargo test --locked --offline` (196 tests across 49 test binaries; `/private/tmp/sese-cpu-limit-suite.log`). Linux behavior is untested here. The new limit is not process-memory, filesystem or network containment, and does not authorize untrusted candidates; verified isolation, an authoritative clean checkout of this change and independent operator onboarding remain open.

## Started-worker timeout and promotion

The previous turn added and verified the CPU ceiling, so it was progress. Added a typed-event integration test that obtains a passing replay, matching shadow observation and approval with a stable worker wrapper. The test then arms the same wrapper to create a start marker and wait before invoking the actual worker. The supervisor reaches a one-second deadline; the resulting newer replay records the deadline error, verifies as failed, and prevents the earlier approval from promoting the candidate. The champion remains unchanged and the ledger verifies. The marker confirms that the wrapper started; the timeout is deliberately injected before Wasmi starts, not evidence of a Wasmi hang or OS containment.

The focused regression passed, and `cargo fmt --check` plus `cargo test --locked --offline` passed 197 tests across 49 test binaries (`/private/tmp/sese-started-timeout-suite.log`). No untrusted candidate was run. Verified isolation and the full goal remain open.

## Signal-terminated worker and promotion

The preceding turn added a real started-wrapper deadline regression, so it was progress. Extended that integration setup with a second controlled wrapper: it executes the real synthetic event worker for the original passing replay and observation, then, after approval, marks that it started and sends `SIGKILL` to itself on the newer replay. The newer replay records a non-deadline error and verifies as failed, the prior approval cannot promote, the champion stays unchanged, and the ledger verifies. The wrapper deliberately self-signals before launching Wasmi on the failed run; this does not demonstrate a spontaneous runtime crash or isolation.

Both the new signal test and the refactored existing timeout test passed individually; `cargo fmt --check` passed. The full suite was not rerun because only test code changed and the two affected cases were exercised; the preceding full run passed 197 tests. No untrusted candidate was run. Verified OS containment and the remaining goal requirements are still open.

## Candidate isolation backend preparation

The preceding turn added and verified a signal-failure promotion regression, so it was progress. Rechecked the environment: Docker Desktop is installed, but its selected `desktop-linux` socket is absent and `docker info` cannot connect; Podman reports no machine. Prepared `contracts/container-isolation-candidate.md` with a concrete possible container boundary, failure-closed lifecycle, provenance and adversarial verification requirements. This is not an implemented or selected security boundary and does not authorize untrusted code. Asked the user whether they can start Docker Desktop or supply another available backend. The current workspace is not an authoritative Git checkout; independent human onboarding and verified isolation remain open.

## Isolation backend availability recheck

The preceding turn produced a concrete isolation evaluation plan, so it was progress. The selected Docker Desktop context still cannot connect, and Podman still reports no configured machine. Checked the other configured Docker contexts as well: neither the default socket nor the OrbStack socket exists, and OrbStack is not installed at `/Applications/OrbStack.app`. No additional usable container backend was found. No untrusted guest was run and no sandbox implementation was claimed. The earlier request for a running isolation backend remains outstanding.

## Docker Desktop visibility correction

The prior checks were made inside the filesystem sandbox and did not establish that Docker Desktop was stopped. After launching the installed app with sandbox approval, a process check showed Docker Desktop and its VM running; `docker info --format '{{.ServerVersion}}'` with the required external-sandbox access returned `29.5.2`. The apparent missing socket was a visibility limitation of the sandboxed command. This corrects the earlier conclusion; Docker is an available candidate for evaluation through approved access, not a verified isolation boundary. No container was built and no untrusted code was run in this check.

## Synthetic container worker smoke probe

Built a temporary Linux ARM64 image from the current worker source and lockfile in `/private/tmp/sese-oci-probe-0c8dzf5y`, using pinned Rust 1.98.0 and Debian bookworm-slim base image indexes. The local image identity is `sha256:503eae959084b387f6f1bef166211d5ce4410b458ae0aa7c9b077b6037b1fc08`; the Dockerfile SHA-256 is `2889ed1aa24f953524077a61f054a44ab5cb08eaf29b5ce797d029293de9b51c`. The build used Docker Desktop's local daemon with sandbox approval; the staged context contained the Rust source, manifest, lockfile and vendored Wasmi, not the workspace database or fixtures. Cargo fetched dependencies during the build. This is an exploratory image, not an application integration or independently attested build.

Ran the image once with a previously generated known synthetic typed-event request, stdin/stdout only, no host mounts, non-root UID, no network, read-only root, dropped capabilities, no-new-privileges, 128 MiB memory and swap settings, one CPU and a 16-process limit. The process exited successfully and returned the expected 16-byte verdict `(throttle, 3.0, 4)`. Docker accepted these settings, but this run did not inspect the actual configuration or verify kernel enforcement, access denial, failure cleanup, daemon interruptions, concurrency, or promotion provenance. It is a protocol smoke test, not evidence that the container is secure. No untrusted candidate was run; the application still has no verified isolation backend.

## Container configuration and memory-limit probes

The previous turn produced a temporary image and synthetic protocol round trip, so it was progress. On Docker Desktop 29.5.2, inspected a disposable container created from that image with the proposed settings; it reported non-root user, no mounts, network `none`, read-only root, all capabilities dropped, no-new-privileges, a 16-process limit, 128 MiB memory and combined memory-plus-swap limit, and one CPU. Inside, `/proc/self/status` showed zero effective capabilities, no-new-privileges and seccomp filtering; cgroup files reflected the requested limits, a write to `/tmp` failed, the Docker socket was absent and the route table was empty. A separate 32 MiB constrained synthetic AWK allocation was killed; Docker reported `OOMKilled: true` and exit 137. Cleanup traps attempted to remove both containers, but removal was not independently verified.

Details and limitations are in `contracts/container-probe-results.md`; raw inspect, kernel and OOM outputs are under `/private/tmp/sese-oci-probe-0c8dzf5y/`. These bounded checks do not demonstrate complete filesystem or network denial, clean up after daemon loss, host or kernel escape resistance, or safe app integration. The worker image is temporary, the app still uses its synthetic process path, and untrusted execution remains prohibited. No application code was changed and no untrusted guest was run.

## Repository worker image build

The previous turn inspected container settings and exercised one controlled memory-limit failure, so it was progress. Added `runtime-spike/Dockerfile.worker` and its Dockerfile-specific ignore file. The multi-stage build uses pinned Rust and Debian base-image indexes, `Cargo.lock`, a non-root runtime user, and only the manifest, source tree and vendored Wasmi in its build context. It is explicitly experimental and is not wired into replay or promotion.

On Docker Desktop 29.5.2, the repository build completed and the resulting image returned the expected `(throttle, 3.0, 4)` verdict for the same known synthetic request under the previously described container settings. Its platform manifest `sha256:5dc23be52aa34f9c2b14c6c61d7d0dedcb77cea8334aeacff555196f7dd283ec` matched the temporary build's platform manifest. The enclosing image index differed because the attestation manifest differed; this does not establish byte-for-byte index reproducibility or independent provenance. The vendored dependency emitted compiler warnings. This was a local build and smoke test, not a clean-checkout test, complete isolation validation or authorization to run untrusted code. No application runtime code was changed.

## Controlled container host-access probes

The prior repository image build and synthetic worker round trip were progress. On Docker Desktop, a labelled positive-control container read a synthetic host sentinel only when it was explicitly bind-mounted read-only. With the mount omitted, the probe found neither the container mount path nor the known host path. Docker reported no remaining containers with the probe label.

For a network positive control, a loopback-only host test server received a synthetic message from a restricted container on Docker's bridge network. A second container with the same restrictions and `network=none` attempted the same resolved host IPv4 address and port and failed with `Network is unreachable`; the server received only the positive message. Docker reported no remaining containers with that test label. The temporary script is `/private/tmp/sese-oci-probe-0c8dzf5y/network_probe.py`; see `contracts/container-probe-results.md` for scope. These are selected path and TCP checks, not exhaustive filesystem, network, kernel or daemon security evidence. The app still has no container execution integration, and no untrusted code was run.

## Container client-death cleanup hazard

The previous controlled host-file and network probes added evidence, so that turn was progress. A disposable restricted container ran a fixed 20-second sleep under `docker run --rm`. After its exact ID was written and Docker reported it running, the probe killed the client with `SIGKILL`. Docker still reported the container running. The probe explicitly removed its own captured container ID and verified it was no longer listed. The temporary script is `/private/tmp/sese-oci-probe-0c8dzf5y/client_crash_probe.py`.

This demonstrates that the future bounded execution path cannot rely on `--rm` or termination of the client for cleanup. It must track owned container identity, explicitly stop/remove it, verify cleanup, and report uncertainty on daemon failure or ambiguous creation rather than produce passing evidence. The result covers one client-death case only. No application integration or untrusted execution was performed; the goal remains open.

## Synthetic container lifecycle prototype

The prior client-death probe exposed a concrete cleanup hazard, so it was progress. Added `runtime-spike/scripts/synthetic_container_lifecycle.py` as a separate experiment using a fixed synthetic throttle request and a caller-selected full image digest. It runs with the previously tested Docker restrictions, limits captured output to 16 bytes, applies an execution deadline, explicitly removes the captured container ID, and queries Docker for that ID before returning a verdict. It returns an error on a failed run, missing ID, failed cleanup or unconfirmed removal. Its deadline does not cover process startup or the separately bounded cleanup calls. Ambiguous creation can still leave an unidentified container, and whole-process or daemon failure is not resolved; the error includes the probe name or captured ID for investigation. It is not connected to replay, shadow, promotion or untrusted execution.

On Docker Desktop 29.5.2, four focused tests passed with two explicitly selected local images: the known worker returned `(1, 3.0, 4)` and its container was absent afterward; a sleep-only image exceeded the two-second deadline, returned no verdict, and its captured container was absent afterward; malformed image identity and unavailable backend failed closed. The deadline test first failed because the error omitted the container ID needed for independent cleanup verification, then passed after including it. The ordinary Python discovery suite passed 26 tests with the two Docker-dependent cases skipped. This is not a security audit, daemon-failure test, promotion gate or proof of isolation; no untrusted code was run.

## Container output and cleanup failure checks

The previous turn added the synthetic lifecycle prototype and exercised its real success and timeout paths, so it was progress. Added a controlled output-flood image test: the helper rejected output beyond 16 bytes, returned no verdict and removed the captured container. A separate test wrapper deliberately failed `docker rm` after a successful synthetic worker response; the helper returned cleanup-unconfirmed with the container ID instead of returning a verdict. The test then removed that exact container and verified it was absent.

Six focused tests passed against the local Docker engine using the known worker, sleep-only and output-flood images. The ordinary Python discovery suite passed 28 tests with four Docker-dependent cases skipped. The cleanup test injects a command failure rather than a daemon outage; ambiguous creation, host/client death, full resource enforcement, image trust and integration with replay and promotion remain open. The fixed-request helper is experimental and no untrusted candidate was run.

## Synthetic container intent records

The preceding turn added and ran real-engine output-flood and injected cleanup-failure tests, so it was progress. The experimental `run_synthetic` API now requires an existing caller-selected journal directory. Before launching Docker it writes an owner-only pending JSON record containing the unique probe name and image identity, syncs the file and directory, and retains it on uncertain cleanup. After Docker removal and an empty ID-filtered listing, it atomically updates the record to `removed` with the captured container ID; a failed record update withholds the result. The API now returns the record path or includes it in an error. These records are unauthenticated and depend on the caller's directory and filesystem; they are not a trusted external ledger.

The creation-intent regression failed before implementation and passed afterward. Eight focused tests passed with real Docker images, including a controlled parent `SIGKILL` while a sleep container was running: the pending intent survived, the observed container name and label matched, and the test manually removed that exact container and confirmed its absence. Normal success and timeout record `removed`; an injected cleanup failure leaves `pending`. The full Python discovery suite passed 30 tests with five Docker-dependent cases skipped. This does not test power loss, implement automatic recovery, authenticate ownership, prove the daemon is unchanged or resolve ambiguous creation; the helper remains synthetic-only and is not wired to replay or promotion. No untrusted code was run.

## Read-only pending-container inspection

The preceding turn added intent records and a real parent-death test, so it was progress. Added `inspect_pending` to the experimental synthetic helper. It opens an owner-only regular receipt without following the final symlink, bounds input and Docker output, rejects duplicate fields and malformed pending records, and queries Docker without mutating the receipt or container. A matching name, image and probe label returns `observed` with the reported container ID and running state; unavailable, missing, mismatched or otherwise unverifiable Docker results return `unknown`, not proof of absence.

The malformed-receipt regression failed before implementation. Nine focused tests then passed with real Docker images, including a killed-parent case that observed the matching running container and returned `unknown` after the recorded image was altered; the test explicitly removed its own container afterward. The Python discovery suite passed 31 tests with five Docker-dependent cases skipped. Records and labels are unauthenticated, the Docker daemon may change or lie, and inspection does not delete, recover or establish ownership. Container lifecycle integration, independent validation and untrusted execution remain open.

## Synthetic container daemon consistency

The preceding turn implemented and verified read-only pending-record inspection, so it was progress. A new regression showed that a legacy record could return `observed` from matching name, image and label without any daemon identity. New v2 synthetic intents now record Docker's reported daemon ID before container launch. Inspection checks that ID before and after the container query; v1 records and unbound v2 records return `unknown`. Cleanup checks the ID before removal and after the removal query, withholds the verdict on mismatch and avoids removal when the mismatch is detected beforehand. Initial daemon unavailability leaves an unbound pending record without launching the worker.

The legacy regression failed before the change and passed afterward. Eleven focused tests passed with real Docker images, including a wrapper that reported a changed daemon before cleanup: the helper returned an error without issuing its removal command, and the test removed its own container. The Python discovery suite passed 33 tests with six Docker-dependent cases skipped. The Docker-reported ID is unauthenticated, separate checks can race with context changes, and a daemon or local attacker may fabricate metadata. Recovery, comprehensive containment, and integration with replay and promotion remain open; no untrusted candidate was run.

## Explicit synthetic Docker endpoint

The previous turn added daemon consistency checks, so it was progress. The synthetic lifecycle API now requires an explicit normalized Unix socket URI, rejects TCP and malformed endpoints before writing a receipt or contacting Docker, passes `--host` to launch, inspect and cleanup commands, and removes inherited `DOCKER_*` environment overrides from those subprocesses. New v3 intent records retain the endpoint; v1 and v2 records remain inspectable as `unknown` rather than being silently rebound. The endpoint is a caller assertion: it may be forwarded or replaced and does not authenticate the daemon. The helper remains fixed-request, experimental and disconnected from replay and promotion.

The remote-endpoint regression failed before implementation and passed afterward. Twelve focused tests passed on the local Docker engine, including a successful run with deliberately conflicting Docker context and host environment variables, changed endpoint and daemon cases, timeout, output flood and parent-death inspection. The Python discovery suite passed 34 tests with six Docker-dependent cases skipped. No untrusted candidate was run; daemon authentication, race-free socket identity, recovery and full isolation integration remain open.

## Built-in typed requests in container prototype

The previous turn bound experimental Docker commands to an explicit endpoint and tested that behavior, so it was progress. The separate synthetic container helper now accepts the existing `allow`, `throttle` and `quarantine` built-ins plus typed synthetic event and window values. It validates field types, unsigned ranges and finite floats before writing an intent or contacting Docker. It returns a validated worker verdict and the selected artifact's SHA-256 rather than deciding whether a replay passed. It does not accept arbitrary Wasm, authenticate the selected image, prove that the image executed those bytes, or write replay or promotion evidence.

The new input regression failed before the API existed and passed afterward. Fourteen focused tests passed with the real Docker engine, including all three candidates with a non-default event and window. A separate Rust/Python compatibility test compared the helper's exact artifact bytes with all three Rust-registry artifacts. These built-ins ignore the window fields, so the real-engine test does not demonstrate guest use of window state. The Python discovery suite passed 36 tests with seven Docker-dependent cases skipped; `cargo fmt --check` and `cargo test --locked --offline` passed 199 tests across 50 test binaries (`/private/tmp/sese-synthetic-container-builtins-suite.log`). This is synthetic prototype evidence, not application integration, comprehensive isolation or authorization for untrusted code.

## Synthetic container request identities

The previous turn implemented and tested typed built-in requests, so it was progress. New v4 pending intents record the selected synthetic candidate, artifact SHA-256 and complete binary request SHA-256 before launch. After confirmed container removal, the record includes `valid_verdict` and an output digest for a correctly formatted response, or `failed` with no output digest on failure. A valid verdict is not a fixture comparison or replay pass. The original event and window are not stored, so the record alone cannot reconstruct them; it remains local, unauthenticated and outside the app ledger. Earlier v3 pending records retain read-only cleanup inspection without backfilling. Also fixed a variable-name collision that caused the returned candidate field to contain a container ID.

The missing request-identity regression failed before implementation. Fourteen focused tests passed against Docker, including all three candidate names, success and timeout record states, changed artifact rejection and v3 pending inspection. The Python discovery suite passed 36 tests with seven Docker cases skipped; the Rust/Python artifact compatibility test passed. No full Rust rerun was needed for this Python-only change; the preceding full suite passed 199 tests. The records do not prove image contents or execution and are not integrated with replay, shadow, promotion or a verified isolation boundary. No untrusted code was run.

## Experimental container fixture comparison

The Rust fixture inspector can now emit the validated typed cases and their existing pack digest. A separate Python experiment runs each case through the fixed synthetic container lifecycle, compares actual and expected verdicts including float bits, and prints a report marked ineligible for promotion. It does not add the comparison to the application ledger; individual lifecycle receipts persist separately. The inspector, Docker image, daemon and host remain caller-selected and unauthenticated. No untrusted artifact was run.

The new inspector and replay tests failed before implementation. Four Rust fixture tests passed, and two Python replay tests passed with the local Docker engine: the throttle built-in matched the repository fixture, the allow built-in failed it, and an empty pack failed before launch. `cargo fmt --all -- --check` passed; Python discovery passed 38 tests with eight Docker-dependent tests skipped. This demonstrates a real synthetic fixture comparison, not integrated replay, isolation verification, or promotion readiness. Durable case comparisons, worker identity and containment remain open.

## Durable experimental container replay reports

The prior turn executed a real synthetic fixture through the container, so it was progress. The separate experiment now writes an owner-only report before execution and syncs updates for each case, including typed inputs, expected and actual verdicts, errors, timestamps and lifecycle receipt paths. Completed mismatches are recorded as failed comparisons; a backend error leaves a failed report and an interrupted run remains pending or running. The report is separate from the app ledger and marked ineligible for promotion; it does not authenticate its data or establish that the image executed the selected artifact.

The backend-failure persistence test failed before implementation. Three focused Python tests passed against the local Docker engine, covering failed backend reporting, invalid empty fixture rejection, and persisted passing and mismatching real synthetic comparisons. Python discovery ran 39 tests successfully with eight Docker-dependent tests skipped. A durable experimental report does not complete app integration or the containment requirement. No untrusted code was run.

## Interrupted replay to container-intent link

The prior turn persisted experimental replay comparisons, so it was progress. A lifecycle intent is now created and its path synced into the running case report before contacting Docker. If that update fails, the worker is not launched. This makes a pending case's separately stored intent discoverable after an interruption; it does not authenticate ownership or perform recovery.

The missing receipt-path regression failed before implementation. Four replay tests passed against the local Docker engine, including killing the replay process while a synthetic sleep container was running, finding the container through the report and intent using read-only inspection, and removing and checking the test container. Python discovery ran 40 tests successfully with nine Docker-dependent tests skipped. This demonstrates one interruption path only. Container trust, application-ledger integration, isolated shadow execution and automatic recovery remain open; no untrusted candidate was run.

## Synthetic container shadow comparison

The prior turn linked interrupted replay cases to lifecycle intents, so it was progress. Added a separate synthetic shadow command that runs fixed champion and shadow built-ins sequentially in separate containers against identical event and window inputs. It writes an owner-only report with both verdicts, artifact identities, timestamps, errors and lifecycle receipt links. A failed champion run leaves the shadow pending; the command does not change application host state or write promotion evidence.

The unavailable-backend test failed before implementation. Two focused tests passed against the local Docker engine, including actual allow/throttle verdicts, distinct removed-container receipts and request digests for the same input. Python discovery ran 42 tests successfully with ten Docker-dependent tests skipped. The report and Docker metadata are unauthenticated; this does not verify comprehensive isolation, application integration, persistent stream state or the promotion gate. No untrusted code was run.

## Shared local event execution component

The prior turn added synthetic container shadow execution, so it was progress. Typed replay and both legacy and measured event hosts now use a common local event executor to capture the worker, run its binary protocol, and recheck the file identity before persistence. This is a preparatory refactor: the container experiment is not connected to it, and the existing local-worker provenance and promotion semantics have not been strengthened. A future container backend needs a distinct ledger representation for the image, daemon, execution settings and lifecycle evidence; a worker-file digest must not stand in for that provenance.

The new executor test failed to compile before implementation. It then passed using the actual synthetic worker and detected an altered copied worker; the focused executor, replay and event-host suites passed 21 tests. `cargo fmt --all` and `cargo test --locked --offline --quiet` passed; the full suite reported 201 tests across 51 groups (`/tmp/sese-executor-suite.log`). This verifies the refactor's behavior, not containment, executable authenticity or untrusted-code safety.

## Worker identity before each event execution

The prior turn consolidated the local event executor and passed the full Rust suite, so it was progress. The executor now rechecks the captured worker file immediately before each execution, in addition to the existing check before replay or observation persistence. This detects a changed file between cases or between champion and shadow and prevents that changed file from being launched under the earlier captured identity.

The new regression failed before the change: a modified copy of the real worker still executed successfully under the captured identity. Afterward, 24 executor, replay, event-host and operator-workflow tests passed (`/tmp/sese-executor-recheck.log`). The check does not authenticate the worker or eliminate replacement between the check and process launch; it does not establish an OS isolation boundary. Container-backed application execution and promotion evidence remain open.

## Fresh source-copy build and synthetic cycle

The previous turn tightened worker checks and passed focused tests, so it was progress. Copied the current workspace to a new temporary directory excluding `target`, Python caches, `.DS_Store` and `.git`. From that copy, `cargo build --locked --offline --manifest-path runtime-spike/Cargo.toml --bins` succeeded with rustc 1.98.0, cargo 1.98.0 and Python 3.15.2. The documented synthetic event-cycle demo ran with scripted `approve` and `rollback`, reported a passing fixture, promotion, rollback and valid ledger. The documented follow-up record, synthetic outcome resolution and export also ran; the export marked the outcome caller-asserted and not training-ready. The independent verifier reported a matching checkpoint and valid chain, with overall evidence bindings still incomplete.

The fresh copy also passed `cargo test --locked --offline --manifest-path runtime-spike/Cargo.toml --quiet`: 201 tests across 51 groups. This was a fresh source copy using the machine's existing Cargo cache, not a clean Git checkout: the workspace has no Git repository. Scripted approval is a test input, not an independent human review or usability study. The worker was local and synthetic, not a verified container boundary. Logs are `/tmp/sese-fresh-build.log`, `/tmp/sese-fresh-demo.log` and `/tmp/sese-fresh-tests.log`; the temporary test database is `/private/tmp/sese-fresh-source-a5749pfo-demo.db`.

## Reject image-declared volumes

The previous turn supplied fresh-copy build and cycle evidence, so it was progress. The synthetic container lifecycle now inspects the selected image's declared volumes before launch, rejects nonempty or unavailable volume configuration, and rechecks the daemon's reported ID. This addresses an observed gap in which Docker could add a writable image-declared volume despite `--read-only`. This is not image authentication, daemon authentication or a complete mount or isolation assessment.

The regression failed against a local synthetic worker image with `VOLUME /sese-volume-test`: it executed instead of rejecting the image. A test-only wrapper removed the container's anonymous volume after that reproduction. After the change, a real-engine test confirmed rejection before `docker run`; 21 container lifecycle, replay and shadow tests passed with local images, and Python discovery ran 43 tests with 11 Docker-dependent cases skipped. The first local probe-image build failed because BuildKit interpreted the bare image ID as a registry name; rebuilding from the already present local tag succeeded. The container experiment remains separate from the application ledger and promotion.

## Compare container reports with an application review

The prior turn rejected image-declared volumes and tested the container lifecycle, so it was progress. Added an informational command that compares completed synthetic container replay and shadow reports with the application's verified review preview: fixture identity and semantics, candidate hashes, each replay input and verdict, observation input, and champion/shadow verdicts. It prints the preview checkpoint and marks its output ineligible for promotion. The command does not import or authenticate container reports, verify their lifecycle receipts, or establish isolation; the preview uses the application's normal writable database-opening path.

The fixture-mismatch regression failed before implementation. A real local Docker replay and two-container shadow run matched a previously created synthetic application review; altering the shadow event caused rejection. The first integration attempt revealed that application observations use named decisions and container outputs use numeric codes; the comparison now reconciles them and still checks exact float bits. Python discovery passed 46 tests with 11 Docker-dependent cases skipped. Container execution remains disconnected from authoritative app replay, shadow state and promotion.

## Container review receipt consistency

The prior turn connected experimental reports to a review preview, so it was progress. The comparison command now reads owner-only, regular v4 lifecycle receipts without following symlinks, rejects duplicate fields or reused container identities, and recomputes each built-in artifact, binary request and verdict digest. It checks matching image and endpoint and requires a recorded valid verdict after confirmed removal. The earlier real container replay and shadow reports passed this check against the existing application preview. The local records and daemon remain unauthenticated, and the result cannot authorize promotion or establish containment.

The altered-request regression failed before implementation and passed afterward; duplicate receipt reuse is rejected. Python discovery ran 47 tests successfully with 11 Docker-dependent tests skipped. The command checks receipt consistency but does not observe the historical process or prevent coordinated rewriting of local records. Application-ledger integration and the containment boundary remain open.

## Ledger link for synthetic comparison reports

The prior turn added receipt consistency checks, so it was progress. An operator can now use `--record-actor` after a successful container/app comparison to append a caller-asserted ledger event linking the existing replay and observation to SHA-256 digests of both report files and the exact preview checkpoint. A changed ledger head rejects the append. The new event is separately inspectable and is not an application replay, review or promotion input; the direct API accepts caller-supplied hashes and does not prove that the comparison ran. Report hashes cover report bytes, not the linked lifecycle receipt contents.

The new API test failed to compile before implementation. The event-review, operator-workflow and checkpoint suites passed 37 tests; two focused tests passed for stale-checkpoint rejection, restart and tamper detection. The full offline Rust suite passed 203 tests across 51 groups (`/tmp/sese-container-assert-full.log`). A real earlier container comparison was appended to a copied synthetic application database, re-read, checked by the independent ledger verifier, and rejected after its event payload was altered in another copy. Python discovery passed 47 tests with 11 Docker-dependent tests skipped. The ledger link does not authenticate the operator, reports, daemon or execution, or establish containment; direct container-backed application replay and promotion remain open.

## One-command synthetic container review comparison

The prior turn added a checkpoint-guarded ledger link, so it was progress. `container_review_run.py` now selects fixed built-ins from an application review preview, executes its fixture and champion/shadow input in separate containers, checks the results and lifecycle receipts, and appends the caller-asserted report hashes only if the preview checkpoint remains current. It returns report paths for inspection. This is an integrated operator workflow, not a replacement for the app's stored replay, observation or promotion gate. Receipt consistency now also rejects a changed reported daemon ID across executions.

The built-in selection regression failed before implementation; the cross-daemon receipt regression exposed a second gap and passed after correction. A real-engine test created a fresh synthetic app cycle, ran the complete container comparison, inspected the assertion, and verified an empty fixture left the ledger unchanged. A concurrent test appended another assertion during execution and confirmed the operation failed specifically on its stale checkpoint without appending its own. Python discovery ran 49 tests with 12 Docker-dependent tests skipped; the focused real-Docker suite passed two tests. The image, daemon and local records are not authenticated, and the workflow does not establish isolation for untrusted code or authorize promotion.

## Retained synthetic container comparison snapshots

The prior turn added a one-command synthetic comparison with a guarded assertion, so it was progress. The workflow now captures the exact replay report, shadow report and verified lifecycle receipt JSON bytes in a bounded caller-asserted ledger payload. The app checks bundle structure and retains the report hashes; it does not independently attest execution or admit the capture as promotion evidence. Existing digest-only assertions remain supported. The bundle is capped at 4 MiB, with at most 1,002 receipts of 4 KiB each.

The new capture API test failed to compile before implementation and passed afterward. A real Docker integration ran the synthetic cycle, captured three receipts, removed its own temporary report and receipt files, re-read the same captured bytes through a new app process and checked the ledger with the independent verifier; it also exercised stale-checkpoint and malformed-fixture guards. The first persistence-test attempt failed because `/var` and `/private/var` refer to the same macOS path; comparing canonical paths resolved that test issue. The full offline Rust suite passed 204 tests across 51 groups (`/tmp/sese-capture-full.log`); Python discovery ran 49 tests with 12 Docker-dependent cases skipped, and the focused real-Docker suite passed two tests. The captured records, daemon and operator remain unauthenticated; actual app replay and promotion still use the local worker.

## Recheck captured synthetic container evidence

The previous turn retained a bounded comparison snapshot, so it was progress. Added `verify_container_capture.py` to open the application database read-only, require an external checkpoint covering the capture, verify the ledger chain and replay, fixture and observation bindings, and compare the retained reports and receipt snapshots with those records. It uses the existing Python comparison logic and does not read the original journal files. Digest-only assertions without a bundle are rejected. The result remains caller-asserted and ineligible for promotion; the command cannot authenticate the daemon, image, execution, checkpoint custody or historical receipt permissions.

The uncovered-checkpoint regression failed before implementation. A real Docker test verified the capture after its source files were removed, then used the lower-level capture API to append an intentionally altered replay report with a valid chain; the new recheck rejected the inconsistent case. Python discovery ran 50 tests with 12 Docker-dependent tests skipped, and the real-Docker workflow suite passed two tests. This verifies a bounded consistency check, not an OS isolation boundary or authoritative container replay.

## Application-initiated synthetic container replay

The previous turn added a retained-capture consistency check, so it was progress. The Rust supervisor now passes literal arguments without a shell, and a fixed-artifact binary bridge connects the application to the experimental container lifecycle. `event-container-replay` executes registered built-ins against the validated fixture and persists a distinct ledger event with ordered cases, expected and actual verdicts, failures, timestamps and completed lifecycle receipts. The write checks the original ledger head, and inspection rechecks built-in candidate identity, fixture digest and case results. The separate event is explicitly unauthenticated and not promotion evidence; its ledger sequence is not an ordinary replay ID and may numerically collide with one.

The literal-argument, bridge, replay and consistency regressions failed before implementation. Nine focused tests passed against real Docker for the Rust executor and operator command, a correct throttle candidate, a wrong allow candidate, a timeout with explicit removal of the test-owned container, and a concurrent ledger change that prevented the result from being recorded. A deliberately rehashed inconsistent result was rejected on inspection, and the independent ledger verifier matched a checkpoint without claiming all evidence was verified. The full offline Rust suite passed 214 tests across 52 groups (`/tmp/sese-rust-container-final-full.log`); Docker-dependent Rust tests return early without their environment settings. Python discovery ran 53 tests with 13 Docker-dependent tests skipped; the separate real-Docker bridge tests passed two tests. A later negative test exposed a missing receipt path in invalid-verdict errors; that was corrected and the focused suite passed. Local-worker shadow, fixture selection and promotion remain separate; daemon/image trust, cleanup after all failures, full container containment and untrusted candidates are unverified.

## Current verification

After the invalid-verdict error-path correction, `cargo test --locked --offline --quiet` passed; Docker-dependent Rust tests return early without their environment settings. `python3 -m unittest discover -s scripts -p 'test_*.py' -q` ran 53 tests successfully with 13 skipped. An initial discovery attempt against `tests/` found no Python tests; the corrected command targets `scripts/`. No container-backed application shadow or promotion integration was added in this verification pass. The workspace has no Git metadata, so the previous fresh source-copy check does not establish a clean checkout.

## Application-initiated synthetic container shadow stream

The preceding turn rechecked existing tests and recorded that evidence; it did not add the planned shadow path. The Rust application now runs the current built-in champion and a built-in shadow in separate containers using the same host-owned measured-v2 prior state. Successful comparisons and both measured durations are persisted in distinct ledger events. Stream reopening recomputes state transitions and receipt/request/output digest consistency; writes reject stale stream state, changed ledger head, or a changed champion. The new command and inspection interface are documented in `runtime-spike/README.md`. The event sequence is not an ordinary observation ID and does not authorize review or promotion. Failed execution leaves the experimental stream unchanged and reports available receipt paths; failure attempts are not themselves recorded in this stream.

The host API and command regressions first failed before implementation. The failure-path regression exposed a missing champion receipt path and the duration regression exposed missing shadow timing; both were corrected. The focused suite passed all five tests with the local synthetic Docker image and endpoint configured (`/tmp/sese-shadow-verified.log`): fixed-artifact preflight, successful execution and restart, stale writer, concurrent ledger change, failed shadow attempt, command inspection, and rejection of a deliberately inconsistent rehashed state. Formatting passed. The full offline Rust suite passed 219 tests across 53 groups (`/tmp/sese-shadow-final-full.log`); Docker-dependent tests in that run returned early without environment configuration. These checks do not authenticate the daemon, image or execution, establish containment for untrusted code, or replace the local-worker promotion path. A clean Git checkout remains unverified because this workspace has no Git metadata.

## Durable container shadow failure attempts

The preceding turn added the container shadow stream and verified it, so it was progress. Failed champion and shadow attempts now produce distinct, versioned, non-promotional events when the ledger and champion remain current. A champion failure records measured duration, updates state with error 500 and skips shadow execution; a shadow failure retains the successful champion receipt and updates state from that champion. Both return a failure event sequence and available receipt paths. Invalid deadlines are rejected before an attempt; stale-ledger or changed-helper failures do not commit or update state. Earlier successful event format remains supported.

Champion-failure persistence, shadow-failure persistence and invalid-deadline regressions each failed before their respective fixes. The focused suite passed eight tests with the local synthetic Docker endpoint and image configured (`/tmp/sese-failure-final-live.log`), covering restart, rehashed inconsistent failure state, changed ledger during failure, valid champion plus failed shadow, and previous successful and stale-stream cases. Formatting passed. The full offline Rust suite passed 222 tests across 53 groups (`/tmp/sese-failure-final-full.log`); Docker-dependent tests returned early without their environment settings. The error event describes the host's attempt and does not establish that a container started, that cleanup succeeded, or that the daemon or image is authentic; inspect reported receipt paths for unresolved cleanup. Container results remain ineligible for review and promotion.

## Cooperative synthetic journal guard

The previous turn made progress by adding the guard and its focused tests, but its application-level Docker verification was interrupted before a process handle or log was created. The synthetic lifecycle now serializes cooperating calls using a bounded directory lock and checks existing synthetic receipts before launching. Pending, malformed, symlinked, unsafe or unrecognized records block another launch; structurally valid v4 removed receipts permit it. The lock is limited to cooperating callers sharing the same unchanged journal directory. Different journals, non-cooperating processes and altered local records are outside the guard; it does not authenticate the daemon or provide cleanup or automatic reconciliation. A Rust error can name an intent path that was never created if the helper blocked earlier, so inspect the journal itself.

Tests for a pending record, two concurrent processes, and a bounded lock deadline failed before implementation and passed afterward. Python discovery ran 57 tests with 13 skipped (`/tmp/sese-journal-python-current.log`). The synthetic lifecycle suite ran 20 tests with four skipped against the local Docker image (`/tmp/sese-journal-lifecycle-live.log`), including a deliberately failed cleanup that blocked the next call before test-owned cleanup. The focused Rust replay and shadow suites passed 18 tests with the image configured (`/tmp/sese-journal-rust-live.log`); timeout-image cases returned early without their separate image setting. The full offline Rust suite passed 223 tests across 53 groups (`/tmp/sese-journal-full-rust.log`); Docker-dependent cases returned early without their environment settings. Formatting passed. Authoritative container-backed promotion, cleanup recovery, untrusted-code containment and clean-checkout verification remain open.

## Explicit synthetic container recovery

The preceding turn added and verified the cooperative journal guard, so it was progress. Added `scripts/synthetic_container_recover.py`: read-only inspection returns the receipt digest and observed identities; explicit removal requires the exact receipt digest, container ID, image, endpoint, reported daemon ID, operator label and removal flag. Under the journal lock it rechecks the observation, writes a recovery intent, removes only the selected ID, checks reported absence and daemon identity, and marks the original v4 receipt removed with a failed result. Unknown observations and mismatches do not remove anything. A failed or interrupted attempt retains evidence for investigation; an atomic replacement followed by a filesystem sync failure can leave the visible receipt updated with durability unconfirmed. Recovery is caller-directed, local and unauthenticated, not an automatic action or promotion evidence.

The missing recovery API and command regressions failed before implementation; a further real-container test exposed an unreported intent-write failure and passed after correction. The final test (`/tmp/sese-recover-audit-green.log`) used a test-owned synthetic container to check wrong ID and receipt digest, failed audit creation, refused removal, successful explicit removal, retained audit and a following journal run. Python discovery ran 59 tests with 14 skipped (`/tmp/sese-recover-python-verified.log`); the live lifecycle suite ran 22 tests with four skipped before the final audit-error reporting change (`/tmp/sese-recover-live-full.log`). Before that last change, the full offline Rust suite passed 223 tests across 53 groups (`/tmp/sese-recover-rust-full.log`) and the focused Rust replay/shadow suites passed 18 with the local image configured (`/tmp/sese-recover-rust-live.log`); timeout-image cases returned early. The final recovery test and Python suite were rerun after the error-reporting change. Docker and receipt identity are not authenticated, untrusted-code containment and authoritative container-backed promotion remain unverified, and a clean Git checkout is still unavailable in this workspace.

## Recovery completion binding

The preceding turn added operator-directed recovery and was progress. New recovery attempts now write v2 started records and, after the removed receipt is written, completion records bound to its exact digest. The journal guard retries a directory sync and refuses launch while an unmatched v2 attempt exists, even if the receipt visibly says removed. A later completed attempt for the same source and container identities resolves earlier started attempts. Historic v1 audits are not reinterpreted as v2 completion; they are left unchanged. The guard remains cooperative, journal-scoped and unauthenticated, and does not establish power-loss durability.

A regression showed that a removed-looking receipt with an incomplete recovery previously allowed another launch. The repaired test also checks an incorrect completion digest, a matching completion and legacy v1 behavior. A real-Docker test used a test-owned container and injected an error after the receipt replacement: the visible removed receipt and started recovery correctly blocked a further attempt (`/tmp/sese-recovery-marker-live.log`). Python discovery ran 63 tests with 15 skipped (`/tmp/sese-v2-recovery-python-final.log`); the live lifecycle suite ran 25 tests with four skipped before the final offline-only legacy test was added (`/tmp/sese-v2-recovery-live-full.log`). The focused Rust replay and shadow suites passed 18 tests with the image configured (`/tmp/sese-v2-recovery-rust-live.log`); timeout-image cases returned early. The full offline Rust suite passed 223 tests across 53 groups (`/tmp/sese-v2-recovery-rust-full.log`); Docker-dependent cases returned early. Manual investigation remains necessary when removal occurred but the v2 completion was not recorded.

## Captured container receipt binding

The preceding turn added recovery completion binding and was progress. Experimental replay inspection previously validated fixture and result data but only checked that successful receipt fields existed. Replay and shadow inspection now share a strict captured-receipt parser and reconstruct the fixed artifact, request and verdict digests; they also reject unexpected fields, reused names or container IDs, and changed reported daemon IDs. Existing inconsistent experimental records are preserved but fail inspection. These records remain distinct from ordinary review and promotion evidence; checks cannot establish historical file permissions or authenticate the image, daemon, or execution.

The live replay regression first showed that an altered request digest was accepted after rehashing the ledger. A second regression showed that shadow inspection accepted an unexpected receipt field. Both now fail closed. The final Docker-configured replay and shadow suites passed 19 tests (`/tmp/sese-binding-final-live.log`), including two-case changes to request and output digests, reused receipt, daemon ID and extra fields; timeout-image cases returned early. Formatting passed. The full offline Rust suite passed 225 tests across 53 groups (`/tmp/sese-binding-final-rust.log`); Docker-dependent cases returned early. These consistency checks do not establish containment or complete the container-backed promotion path.

## Advisory container review preview

The preceding turn added captured-receipt binding and was progress. The new read-only `event-container-review-preview` command compares explicitly selected stored container replay and shadow events with the ordinary local review preview in one verified database snapshot. It requires the domain's pinned fixture, matching candidate and current champion generation, exact prior state and event, matching verdicts, and distinct container identities on the same reported daemon. It returns the ordinary preview, experimental records and checkpoint; the result is explicitly unauthenticated and ineligible for promotion. Independently measured p99 values often differ, so independently advanced streams can fail the exact-input check.

The initial API and CLI regression failed before implementation. A real-container regression then found that matching replays could use a fixture other than the domain pin; that now fails closed. The final Docker-backed workflow (`/tmp/sese-container-preview-final-live.log`) passed for a matching synthetic replay and shadow, rejected different events, prior state, failed replay, unpinned fixture, wrong ID namespace, and a replay receipt reused in a separately valid shadow event, and verified read-only operation with an unchanged checkpoint. The full offline Rust suite passed 226 tests across 54 groups (`/tmp/sese-container-preview-final-rust.log`); Docker-dependent tests returned early. A separate offline comparison-logic test added afterward passed (`/tmp/sese-container-preview-unit.log`); it uses fabricated values and is not evidence of container execution. Formatting passed. This preview does not authenticate execution, replace the normal review decision, or complete container-backed promotion.

## Container comparison of a stored observation

The preceding turn added the advisory review comparison and was progress. `event-container-observation` now retrieves a verified measured-v2 application observation and runs fixed champion and shadow built-ins against its exact recorded prior window and event. A separate ledger experiment retains their successful outputs and receipts without changing the app stream; its write rejects a changed ledger or champion. The source must fall within the current champion tenure, and inspection rechecks the experiment against its source and the champion-transition history at the experiment's ledger position. The advisory review preview accepts this experiment as well as the independently advanced container stream and identifies its comparison kind. Failed execution returns receipt paths without appending this experiment. All container evidence remains unauthenticated and ineligible for promotion.

The API regression initially failed to compile because the operation was absent. An offline rollback regression then demonstrated that champion ID alone admitted an observation from an earlier tenure; it now rejects before launching. A Docker-backed regression showed that a rehashed generation change was accepted by inspection; inspection now rejects it. The three-test Docker-configured suite passed (`/tmp/sese-observation-complete-live.log`), including the later measured window, a concurrent ledger change, stale tenure, source-linked comparison, and rehashed historical generation. The final interface and read-only CLI rerun passed (`/tmp/sese-observation-interface-final.log`). Formatting passed. The final offline Rust suite passed 229 tests across 54 groups (`/tmp/sese-observation-interface-rust.log`); Docker-dependent cases returned early. Receipt consistency and transition linkage do not authenticate Docker, establish containment, validate user value, or complete the promotion path.

## Stored-observation comparison failures

The preceding turn added comparisons using a stored observation and was progress. Champion and shadow execution failures now append a distinct versioned failure experiment when the original ledger checkpoint and champion remain current. The record links the unchanged source observation, identifies the failed phase, retains available error and receipt paths, and preserves the completed champion receipt when the shadow fails. It is not accepted by the successful advisory preview and cannot authorize promotion. Invalid input, changed helper identity or a changed ledger still prevents the append; the source stream and champion are unchanged. An error or receipt path does not prove that a container started or was removed.

Champion-failure and shadow-failure regressions each demonstrated that the previous code returned an error without an experiment. The offline focused tests passed for restart, a rehashed inconsistent failure stage, invalid deadline, unchanged source observations and champion, and a ledger change while the attempt failed (`/tmp/sese-observation-failure-focused.log`). The Docker-configured review suite passed five tests (`/tmp/sese-observation-failure-final-live.log`), including a successful test-owned champion followed by a deliberately failed shadow, retained receipt and rejection by the advisory preview. Formatting passed. The full offline Rust suite passed 231 tests across 54 groups (`/tmp/sese-observation-failure-full.log`); Docker-dependent cases returned early. Docker, image and operator identity remain unauthenticated; this does not establish containment or complete the promotion path.

## Window-sensitive synthetic guest

Added the fixed `window-probe` guest across the Rust registry and Python container bridge, including replay, observation and review artifact recognition. Its allow score uses prior count, mean and p99, and its reason uses prior last error. Unlike the earlier fixed guests, it detects omission or substitution of selected prior-window fields. Cross-language byte parity is checked; arbitrary artifacts remain rejected.

The guest and cross-language regressions failed before implementation and pass afterward. A local Docker test passed with two distinct prior windows and separate receipts (`/tmp/sese-window-docker.log`). A separate Docker-backed test passed through measured replay, a later stored observation, source-linked container comparison and a non-promotional review preview (`/tmp/sese-window-review.log`). Python discovery passed 63 tests with 15 skipped (`/tmp/sese-window-python.log`); Rust formatting and the offline Rust suite passed (234 tests across 54 groups, `/tmp/sese-window-rust-full.log`), with Docker-dependent cases returning early in the offline run. These controlled tests do not authenticate the daemon, image or operator, establish containment, authorize promotion, or complete the application.

## Linux worker descriptor inheritance

The preceding window-sensitive probe was progress. The Linux supervisor now marks all descriptors above stderr close-on-exec in the forked child with `close_range(CLOSE_RANGE_CLOEXEC)`, preserving the spawn error pipe until exec and refusing startup when the syscall fails. This closes a concrete host-resource leak in the real worker launch path but does not provide filesystem, network, kernel or full process isolation; untrusted execution remains prohibited. Linux kernels and security policies must support and permit the operation.

In a local Linux Docker build, the existing descriptor test, enabled for Linux, failed against the prior launcher because the synthetic worker inherited descriptor 200 (`/tmp/sese-linux-fd-red.log`), then passed after the change (`/tmp/sese-linux-fd-green.log`). A second regression used a seccomp filter to deny `close_range`: the old launcher started the worker and failed the test (`/tmp/sese-linux-denied-red.log`); the new launcher reported the error without starting it (`/tmp/sese-linux-denied-green.log`). The final Linux build passed 15 supervisor tests and one literal-argument unit test (`/tmp/sese-linux-supervisor-verified.log`), while the macOS supervisor suite passed 16 (`/tmp/sese-macos-supervisor.log`); formatting passed. An intermediate Linux suite reused an older test binary and was repeated with a forced rebuild. These are synthetic platform checks, not full containment or a clean-checkout proof.

## Measured operator walkthrough

The preceding Linux launcher hardening was progress. Added a separately versioned measured-v2 synthetic fixture and updated the manual CLI walkthrough to replay it, compare two pre-review events using persistent measured state, and independently verify stream receipts. The earlier legacy fixture is unchanged. The existing measured-stream operator test now uses the documented fixture; it failed before the fixture existed and passed after its addition.

Executed the walkthrough's extracted shell blocks on fresh local databases with scripted inputs, not an independent human operator. The approval path ran replay, two observations, explicit approval, promotion, a third observation, rollback, caller-asserted validated outcome, export and verification; the verifier checked four measured receipts (`/tmp/sese-measured-manual-final.log`). The rejection path skipped promotion, retained the bootstrap champion, left the outcome pending and exported no rows; the verifier checked three measured receipts (`/tmp/sese-measured-rejection.log`). The focused operator suite and two stream-verifier tests passed (`/tmp/sese-measured-guide-tests.log`, `/tmp/sese-measured-guide-verifier-tests.log`). Local versions: rustc 1.98.0, cargo 1.98.0, Python 3.15.2. This is neither an independent operator walkthrough nor a clean Git checkout, and process timings are not performance evidence. Untrusted-code isolation remains unverified.

## Nonblocking candidate intake

The measured walkthrough was progress. Both event and window artifact registration now use the existing bounded regular-file reader. On macOS and Linux it opens nonblocking before checking the file type, so a named pipe without a writer is rejected rather than hanging intake; the 65,536-byte bound and non-executing behavior remain. Other platforms do not receive a nonblocking-open guarantee from this change.

The new regression reproduced an `event-register` hang with a named pipe before the change (`/tmp/sese-intake-fifo-red.log`); afterward both registration commands returned an error without candidate or ledger records (`/tmp/sese-intake-fifo-green.log`). The full affected macOS CLI and operator workflow suites passed 14 tests (`/tmp/sese-intake-cli-tests.log`), and the focused FIFO regression passed in a Linux build (`/tmp/sese-linux-intake.log`). Formatting passed. This addresses an intake availability issue, not safe execution of untrusted candidates or the remaining completion gates.

## Baseline and scope review

Initialized the previously unversioned directory and pushed baseline `05a25a0` to the user-supplied GitHub remote. A fresh clone of that commit built with the README's locked build command, and the scripted synthetic event demo ran through approval and rollback (`/tmp/sese-clean-remote-build.log`, `/tmp/sese-clean-remote-demo.log`). Its locked Rust suite passed 235 tests across 54 groups, with Docker-dependent cases returning early (`/tmp/sese-clean-remote-test.log`); Python discovery ran 63 tests with 15 skipped (`/tmp/sese-clean-remote-python.log`). The clone used the current machine and available dependency cache; this is not an independent-machine or independent-operator verification.

The source sketch describes candidate generation and earned autonomy, including automatic promotion when validated and escalation for borderline cases. The current goal narrowed its first milestone to fixed synthetic work and explicit human promotion; that was an implementation choice, not evidence that all the resulting operator steps were requested. The prototype has working synthetic replay, observation, decision history and rollback, but lacks the candidate-generation loop, a verified untrusted execution boundary and automatic promotion. Experimental container evidence remains separate and non-promotional. Repeated work on auxiliary receipts and governance has not closed those core gaps. Next work should focus on a low-friction automated synthetic cycle and the actual generation/execution path, preserving evidence and an explicit boundary for real or untrusted changes rather than multiplying manual steps.

## Shorter synthetic evaluation workflow

The previous turn pushed the baseline and verified a fresh clone, which was progress. Added `--prepare-only` to run fixed-candidate replay and shadow comparison without stdin and emit a complete review snapshot for a persistent database. The single `event-review-prepared` command rechecks that snapshot and checkpoint, records an explicit approval or rejection, and attempts promotion after approval. Rejection leaves the champion unchanged; a promotion failure reports the review already recorded because the two writes are not one transaction. The README documents the commands. This is still a human-gated fixed synthetic cycle, not automatic generation or automatic promotion.

The preparation and decision regressions failed before their respective interfaces existed. Tests now cover successful measured preparation and approval, rejection, altered and stale evidence, missing persistent storage, and failed replay without promotion; a separate run checked JSON output with a follow-up fixture. The affected demo and CLI suites passed 30 tests (`/tmp/sese-prepared-suites.log`); the full offline Rust suite passed 238 tests across 54 groups (`/tmp/sese-prepared-full.log`), with Docker-dependent cases returning early. Formatting and changed-file whitespace checks passed. No untrusted execution or production readiness follows from these tests.

## Atomic prepared decision

The shorter synthetic evaluation workflow was progress. Its combined approval command initially committed the review before attempting promotion, so a promotion failure could leave an approval behind and force manual recovery. Prepared approval and promotion now use one immediate SQLite transaction; a failure rolls back the review, its ledger event and the champion update. Prepared rejection still records a rejection without promotion, and the existing separate APIs continue to work.

An injected SQLite trigger reproduced the orphaned approval before the fix (`/tmp/sese-atomic-review-red.log`). Afterward, the failed operation left the checkpoint unchanged and the same prepared evidence succeeded on retry (`/tmp/sese-atomic-review-retry.log`). The affected demo, review and operator workflow suites passed 50 tests (`/tmp/sese-atomic-focused.log`); the full offline Rust suite passed 239 tests across 54 groups (`/tmp/sese-atomic-full.log`), with Docker-dependent cases returning early. Formatting and changed-file whitespace checks passed. This removes a recovery step for the current synthetic workflow; candidate generation, autonomous promotion and untrusted-code isolation remain open.

## Local threshold candidate proposal

The preceding atomic decision change was progress. Added `event-propose-threshold`, which compiles a finite, bounded numeric threshold into a fixed event-v1 Wasm rule template and stores its bytes by digest through existing non-executing intake. Newly stored results remain unclassified; existing identities retain their prior classification. The output reports the supplied threshold and local compiler version, but these proposal parameters are not yet retained as ledger-bound provenance. This is deterministic user-parameterized generation, not autonomous or model-assisted generation.

The interface regression failed before implementation (`/tmp/sese-propose-red.log`). The final focused suites passed 22 tests (`/tmp/sese-propose-suites-final.log`), checking deterministic identity, different thresholds, invalid inputs without additional records, parseable import-free Wasm with the expected export, and rejection by the synthetic replay path before worker launch. The generated artifact was not executed; its intended rule behavior is not yet runtime-verified. This does not complete the generation/evaluation loop or establish a containment boundary.

## Pre-start synthetic container inspection

The local threshold proposal was progress but remained unexecutable. The fixed-synthetic lifecycle now uses `docker create`, inspects the unstarted container's reported identity and restrictions, and only then attaches and starts it. It checks the requested image, label, user, network, mounts, capabilities, no-new-privileges and resource limits; mismatches fail closed and an identified container is removed and recorded as failed. Unknown creation or cleanup remains pending. This is an incremental boundary check, not authorization to execute generated or arbitrary candidates: the daemon, image and inspection report are not authenticated, and the check does not independently establish kernel enforcement.

A real Docker wrapper changed the requested network to bridge; the old helper ran the fixed worker without detecting it (`/tmp/sese-preflight-red.log`), while the new helper rejected it before start and recorded cleanup (`/tmp/sese-preflight-green.log`, with final lifecycle coverage in `/tmp/sese-preflight-live-final.log`). On Docker Desktop 29.5.2, the complete lifecycle suite passed 28 tests using worker, timeout, flood and volume images; three Rust container integration suites passed 26 tests (`/tmp/sese-preflight-rust-final.log`). The initial Rust run used fault-injection wrappers expecting `docker run` and failed; the wrappers were updated to intercept `docker start`, then the suites passed. Offline Python discovery ran 65 tests with 16 skipped (`/tmp/sese-preflight-python-offline.log`). Generated candidates remain blocked from this path and container evidence remains ineligible for promotion.

## Shared evaluation deadline and cleanup grace

The preceding pre-start inspection was progress. The synthetic lifecycle now shares one evaluation deadline across journal-lock wait, Docker preflight and worker output instead of starting a new clock after inspection. The Rust supervisor allows 30 additional seconds for cleanup after that evaluation deadline. Cleanup and process creation remain separately bounded or subject to filesystem/OS delays; uncertain cleanup still leaves a pending receipt. A bridge error may be reported generically as `worker failed`, without a specific cause in the receipt.

A delayed-inspection regression exceeded the old two-second evaluation budget (`/tmp/sese-preflight-deadline-red.log`) and passed after the change (`/tmp/sese-preflight-deadline-green.log`), without starting the test worker and with the known container removed. The old Rust timeout test demonstrated that its supervisor killed the bridge during cleanup, leaving a test container (`/tmp/sese-rust-cleanup-red.log`); the revised test confirmed removal and a failed receipt (`/tmp/sese-rust-cleanup-final.log`). The real-Docker lifecycle suite passed 29 tests (`/tmp/sese-preflight-deadline-live-final.log`), and the Rust container executor suite passed 11 (`/tmp/sese-rust-cleanup-suite.log`). Offline Python discovery ran 66 tests with 17 skipped (`/tmp/sese-deadline-python-offline.log`). This reduces a known manual recovery case but does not prove cleanup after daemon or host failure, nor authorize untrusted candidate execution.

## Retry after read-only preflight failure

The deadline and cleanup change was progress. A failed Docker preflight previously left an indistinguishable pending creation intent and blocked all subsequent attempts even though creation had never been called. The lifecycle now writes a versioned `not_created` preflight record; after successful read-only checks it durably replaces that record with the existing pending intent before Docker create. Valid preflight records allow retries; pending, malformed and legacy unresolved records still block. An inspection result describes the record read, not the state of an active run later. The journal remains unauthenticated and power-loss durability is unproven.

The retry regression failed before the change (`/tmp/sese-preflight-retry-red.log`) and passed afterward. A real-Docker test confirmed the pending intent was present before create, deliberately made a completed creation ambiguous, verified retry was blocked, and removed its identified test-owned container (`/tmp/sese-preflight-ambiguous-live.log`). Another real-Docker test ran a worker successfully after an unavailable preflight without manual recovery (`/tmp/sese-preflight-retry-recovery.log`). The live lifecycle suite passed 32 tests before the latter test was added (`/tmp/sese-preflight-retry-live.log`); the focused new test also passed. Offline Python discovery ran 70 tests with 19 skipped (`/tmp/sese-preflight-retry-offline-final.log`). Three offline Rust container suites passed 26 tests with Docker cases returning early (`/tmp/sese-preflight-retry-rust-offline.log`), and a real Rust executor success test passed (`/tmp/sese-preflight-retry-rust-live.log`). No untrusted execution is authorized.

## Worker-side isolation self-check

The preceding retry change was progress. The experimental Linux worker image now starts with `--require-isolation-v1`, which checks selected process, cgroup, root-mount and network conditions before reading a guest request. The ordinary local worker mode remains available for known synthetic inputs. The helper accepts other caller-selected images and does not authenticate the worker or require this entrypoint; the self-check is not an attestation or a complete isolation boundary.

The unrestricted-host regression failed before implementation and passed afterward (`/tmp/sese-worker-profile-red.log`, `/tmp/sese-worker-profile-green.log`). A first image rejected the intended Docker Desktop profile because that host exposed inactive tunnel interfaces; inspection showed that those devices were down, so the network check was corrected to reject active non-loopback interfaces and IPv4 routes. On the rebuilt image, a fixed synthetic request succeeded with the expected profile and failed with bridge networking, changed memory, writable root, or no-new-privileges disabled (`/tmp/sese-worker-canary-profile-final.log`). Fixed candidates also succeeded through the lifecycle and Rust executor (`/tmp/sese-worker-canary-lifecycle.log`, `/tmp/sese-worker-canary-rust-live.log`). The local event ABI and CPU-limit suites passed 10 tests, and offline Python discovery ran 71 tests with 20 skipped (`/tmp/sese-worker-canary-rust-tests.log`, `/tmp/sese-worker-canary-python.log`). The image, daemon, source custody, unchecked access paths and kernel boundary remain unverified; generated candidates remain blocked.

## Fixture-driven threshold proposal

The worker-side self-check was progress but did not unblock generated candidate execution. Added `event-propose-from-fixture`, which derives a threshold from labeled allow and throttle examples when the classes are strictly separable and the template can reproduce their expected scores and reasons. It compiles and stores the resulting artifact through existing non-executing intake. The output includes the training fixture identity and labels the result as a static in-sample fit; it is not a replay or validation. Training provenance is returned but is not yet ledger-bound, and the generated artifact remains unclassified and blocked from the synthetic executor.

The CLI regression failed before the command existed (`/tmp/sese-infer-red.log`). Focused CLI, candidate guard and fixture suites passed 20 tests (`/tmp/sese-infer-suites.log`), including rejection of overlapping labels, a missing class, unsupported decisions, and unrepresentable scores or reasons without creating extra candidates. The documented synthetic example produced threshold 2.0 without executing or validating the artifact. No independent holdout, generated-code runtime behavior, autonomous model, or untrusted containment was established.

## Fixture proposal provenance

The fixture-driven proposal was progress, but its source identity disappeared unless the caller saved stdout. Fixture-derived generation now stores the exact caller-supplied fixture, its byte and semantic digests, the threshold bits, generator version and candidate digest in a ledger event atomically with candidate insertion. `event-proposal-inspect` opens the database read-only and recomputes the source-to-artifact link; ordinary chain verification does not itself recompute proposal semantics. This is local caller-supplied provenance, not authenticated authorship, replay or validation. The explicit-threshold command remains separate and does not yet store a proposal event.

The inspection regression failed before implementation (`/tmp/sese-proposal-provenance-red.log`). Focused suites passed 21 tests (`/tmp/sese-proposal-provenance-suites.log`), including an injected ledger-write failure that left no candidate, exact fixture retention across reopen, and rejection of altered candidate bytes. A separate process verified the generated event's ledger chain and checkpoint. The full offline Rust suite passed 243 tests across 54 groups (`/tmp/sese-proposal-provenance-rust-full.log`), with Docker-dependent cases returning early; Python discovery ran 71 tests with 20 skipped (`/tmp/sese-proposal-provenance-python.log`). Generated candidates remain unexecuted and unclassified; the trusted execution boundary and actual replay are still open.

## Require the isolation-checking worker profile

The previous goal turn verified and pushed the work through commit `866d563`; the generation-to-replay gap remained. The application synthetic-container bridge now requires the Docker-reported entrypoint `/event-worker --require-isolation-v1` and no additional command arguments. A mismatch fails before start and uses the existing cleanup path. Direct lifecycle tests can still select other images for fault injection; the Rust timeout regression uses a test-only bridge for its sleep image. This does not authenticate the image or daemon, prove kernel containment, permit untrusted execution, or make container evidence eligible for promotion.

The new profile regression failed before implementation because the check was absent, then passed. A live local Docker test ran the canary image and rejected an injected entrypoint override before start; a real Rust synthetic executor test passed using the same canary image. The real sleep-image timeout and cleanup regression also passed with its test-only fault-injection bridge. Offline Python discovery ran 73 tests with 21 skipped. Generated candidates still cannot be replayed; the bridge check is a prerequisite, not completion of that workflow.

## Prevent loader-environment override

The entrypoint check was progress but did not inspect the worker's environment; a dynamic-loader override could run before the worker self-check. The application bridge now requires the expected PATH-only environment from the pinned worker image. Direct synthetic fault-injection lifecycle use remains unchanged. The new regression failed with an injected `LD_PRELOAD` before the change; afterward, live Docker tests accepted the expected profile and rejected both loader and entrypoint overrides before start. Offline Python discovery ran 73 tests with 21 skipped. This reported environment is not proof of image, daemon, or kernel trust; generated candidates remain blocked from replay.

## Restricted-worker failure cases

The previous turn's environment check was progress. Added `container_worker_limits.rs` to exercise fixed test-owned Wasm modules directly in the restricted worker image: a correct control, a forbidden host import, an infinite loop and excessive initial Wasm memory. On the local Docker Desktop engine, the control returned the expected binary verdict and each negative case exited without a verdict; each identified container was removed and its absence checked. Run with `SESE_TEST_CANARY_IMAGE=sha256:... SESE_TEST_DOCKER_HOST=unix:///... cargo test --offline --test container_worker_limits` from `runtime-spike/`. Without the image variable the test returns early. This tests worker rejection under the selected profile, not the application's replay evidence, a kernel OOM, arbitrary candidates, or promotion behavior. No generated candidate was executed.

## One-command synthetic container comparison

The previous turn's restricted-worker tests were progress but did not connect the experimental results to a user workflow. Added `event-container-evaluate` to check a measured review's fixture and source observation, run the fixed candidate's container replay and the observation's champion/shadow comparison, and return their existing consistency preview in one operation. A later failure reports already-recorded experiment sequences where available; the records remain separate, synthetic, unauthenticated, and ineligible for promotion. The command rejects mismatched fixture versions before starting Docker or changing the ledger. It does not run generated or arbitrary candidates.

The live window-sensitive integration test initially failed because the command did not exist; after implementation it passed against the local canary worker. It also verifies the mismatch rejection leaves the ledger and journal unchanged. The same test still exercises the separate underlying APIs. This removes manual assembly of experiment IDs; it does not close the execution-boundary, untrusted-replay, or independent-review gaps.

## Carry a prepared review through container comparison

The previous turn's combined comparison was progress, but its two ledger events made an existing prepared review stale. The comparison now optionally accepts that prepared JSON, validates it before launch, and returns a refreshed copy only when exactly its two expected experiment events were appended and the original review otherwise stayed unchanged. `event-review-prepared` accepts the resulting comparison JSON directly. The explicit decision still uses ordinary local synthetic review evidence; the container experiment is advisory, not an additional promotion gate.

A live Docker integration test first failed because the combined command did not accept the prepared review. After the change it rejected an altered checkpoint before starting a container, ran the valid comparison, rejected the old stale review, and recorded an explicit rejection from the returned comparison without promotion. The offline demo and container-review suites passed 27 tests; their Docker-dependent cases returned early. This removes a recovery step but does not enable generated candidates or establish a trusted containment boundary.

## Event intake structural feedback

The preceding prepared-review refresh was progress but did not address candidate intake. `event-register` now automatically reports a non-executing structural check for parseability under the runtime's strict parser settings, forbidden imports, and the typed-event export signature. Incompatible bytes remain stored, and a compatible result is explicitly not replay, behavioral validation, or promotion evidence. The parser runs on the host; this is not proof of parser security or permission to execute the candidate.

The intake regression failed before the feedback existed, then passed for malformed bytes, an import, a wrong signature, a structurally compatible infinite-loop function and a module with a trapping start function. The latter two were not instantiated or run by intake. The focused CLI, candidate-guard and event-ABI suites passed 25 tests. Unclassified candidates still cannot enter the real replay path; this feedback is not a substitute for that missing capability.

The subsequent full offline Rust suite passed 245 tests across 55 groups (`/private/tmp/sese-current-full-test.log`); Docker-dependent cases returned early without their environment variables. Python discovery ran 73 tests with 21 skipped. Live Docker checks are recorded in the relevant entries above and do not establish an untrusted execution boundary.

## Fresh checkout of the current checkpoint

Pushed `b50da6582198914556c1d8adb7500c0a780ff932` and cloned that exact GitHub revision into `/private/tmp/sese-clean-current.URagYL/repo`. The documented locked build succeeded (`/private/tmp/sese-current-clean-build.log`); the documented locked Rust suite passed 245 tests across 55 groups (`/private/tmp/sese-current-clean-test.log`), with Docker-dependent cases returning early. Python discovery ran 73 tests with 21 skipped. Versions were rustc 1.98.0, cargo 1.98.0 and Python 3.15.2; the machine's existing dependency cache was available.

A scripted run of the onboarding commands approved, promoted, rolled back, recorded a caller-asserted synthetic outcome, exported, and verified the checkpoint and selected ledger links. The verifier reported `evidence_bindings_checked: false`, so it did not establish all evidence bindings. A separate scripted run from the clone used the existing local canary image to complete the combined container comparison and reject the prepared review; it reported `matched: true`, `promotion_evidence: false`, and `promoted: false`. These were not an independent operator walkthrough, a fresh-machine install, untrusted candidate replay, or proof of containment. The source checkout and clone were clean at the time of the checks.

## Preserve request bytes through the container bridge

The preceding turn pushed and verified the fresh checkout, but unclassified replay remained open. The application bridge now forwards its original immutable request to the synthetic lifecycle rather than rebuilding it after validation; the lifecycle checks exact equality with the fixed candidate and typed inputs before launch and derives the artifact digest from that request. Mismatched or mutable request bytes are rejected before the lifecycle runs. Direct synthetic callers without a request retain the existing constructed-request behavior. This removes a redundant translation in the candidate-to-worker path, but the fixed built-in allowlist remains and this is not untrusted replay or sandbox verification.

The new request-forwarding regression failed before the interface existed and passed afterward. The internal lifecycle also rechecks the fixed candidate and bytes before any container work, including when called directly. Python discovery ran 75 tests with 21 skipped; the live Rust container executor, timeout-cleanup and prepared-comparison tests passed against the local synthetic images. No generated or unclassified candidate was executed.

## Worker build reproducibility experiment

The preceding request transport change was progress but image provenance remained open. On Docker Desktop 29.5.2, Linux arm64, an uncached `--network=none` build failed because the build image lacked a Cargo registry cache. Two sequential locked builds with normal networking and the original Dockerfile produced the same 2,845,008-byte worker SHA-256 `b3c70cbca4038836d5ca2baa2922a35bdbbb1009549201c77256265fdefb6f52`, but different image IDs and worker-layer hashes. Two further sequential uncached builds with `--build-arg SOURCE_DATE_EPOCH=0` still produced different worker layers.

The Dockerfile now normalizes the worker executable timestamp after compilation. Two sequential uncached builds with that change and `SOURCE_DATE_EPOCH=0` had identical root filesystem layers and matching inspected configuration, while the image IDs still differed (`sha256:320966f7d40129133171c9fdbbe0ca4155019a2083adced1dcef3f8391de76c2` and `sha256:16a15f01587fd53f3b70b2f4b52dbace0a985012135ee2782a5f3cdf9f76c91c`). A normalized image passed two live worker-profile tests and the live Rust synthetic container executor test. This is repeatability on one builder, not independent provenance, image or daemon authentication, or untrusted-execution authorization. Builds with an empty dependency cache still require registry access.

## Container user-namespace observation

The preceding build experiment was progress but did not establish an execution boundary. A fixed synthetic shell under the same restricted Docker profile reported identity UID and GID mappings in the Docker Desktop Linux VM; `unshare -Ur id` returned `Operation not permitted`. This demonstrates that this profile did not itself create a remapped user namespace on the tested engine. It does not identify the reason for the `unshare` denial or establish the host's macOS UID mapping. Docker's documented enhanced isolation uses user namespaces as an additional protection; its availability was not determined. The exact result and source are recorded in `contracts/container-probe-results.md`. This informs the platform threat model and does not authorize untrusted candidate execution.

## macOS sandbox alternative probe

The preceding user-namespace observation was progress but left candidate isolation unresolved. Podman still reports no configured machine; Docker reports runc as its available runtime. Outside the tool sandbox, `sandbox-exec` could apply a permissive profile and a targeted read-denial profile, but a deny-by-default profile aborted the fixed worker at startup. An `allow default` profile denying network, writes, and reads from selected user and workspace directories ran the fixed synthetic worker and denied a controlled private-file sentinel. See `contracts/runtime-assessment.md` for the exact scope and untested paths. This is an alternative feasibility observation, not a deny-by-default profile, memory boundary, app integration, or permission to execute untrusted candidates. No arbitrary candidate was run.

## Per-case event replay timing

The macOS probe was progress but did not enable untrusted replay. The existing local synthetic event replay now records each case's wall-clock start and finish plus monotonic elapsed nanoseconds in a versioned ledger event, atomically with the replay. It binds the replay hash and ordered case IDs; inspection reports `case_timings: null` for older runs without the event. The independent verifier checks present timing events whenever it checks event replays. Timing is a host assertion, includes worker supervision, and is not an execution attestation or benchmark.

The new regression failed before timing was implemented. Tests cover successful and trapped cases, restart, older records without timing, a rehashed case-ID mismatch rejected by both verifiers, transaction rollback on timing-write failure, and the maximum valid case-ID payload. The first full suite exposed a historical migration test that reconstructed an old ledger while accidentally retaining the new timing event; removing that impossible event from the test reconstruction fixed it without changing old data. The final offline Rust suite passed 248 tests across 55 groups (`/private/tmp/sese-case-timing-full-current.log`), with Docker-dependent cases returning early; Python discovery ran 75 tests with 21 skipped. A live container prepared-comparison regression also passed (`/private/tmp/sese-timing-container-live-final.log`). Unclassified candidates remain ineligible for execution.

## Deny-by-default macOS probe and status

A controlled deny-by-default profile importing Apple's `system.sb` ran the fixed synthetic worker. A system-file positive control succeeded; the synthetic private sentinel was denied; a test write did not create its target; a shell reported fork denied; and a sandboxed loopback connection did not reach a listener that received an unsandboxed positive control. The write test also encountered a denied working-directory lookup, so it is not isolated proof of a write-rule denial. The inherited Apple policy, other access paths, memory enforcement and integration remain unverified. No untrusted artifact was run. The grouped probe is `/private/tmp/sese-native-profile-probe.py` while that temporary file exists.

Status: the fixed synthetic CLI supports replay, shadow observations, explicit review and promotion, rollback, synthetic outcomes, history and conditional exports. Candidate intake records arbitrary bounded bytes, but unclassified candidates cannot be replayed; isolation and independent operator/security review are not established. The current uncommitted timing and container changes postdate the last pushed checkpoint `227e040`. The user's concern about excess intervention is valid: explicit promotion is a stated first-milestone requirement, while repeated sandbox escalations are platform permission checks and prior work also accumulated more auxiliary review and evidence machinery than the original self-evolving direction requires. Continue toward a real candidate-to-replay path and avoid adding extra operator decisions.

## Stop repeated worker failures during replay

The preceding controlled sandbox probe was progress but did not authorize untrusted replay. The local synthetic event replay previously launched the worker again for every remaining case after an execution error. It now stops launching on the first worker error, persists that failure and all remaining cases as skipped, and leaves skipped timestamps null. A valid verdict that merely differs from the expected value does not stop later cases. Timing records use v2; existing v1 records retain their original interpretation, including multiple executed failures. Rust and Python verifiers reject invalid v2 skip ordering.

The regression first reproduced three launches for a three-case fixture, then confirmed one launch, two persisted skipped cases and no fabricated timing. Tests also checked that mismatched verdicts do not cause early stopping, v1 compatibility, invalid skip ordering, and that timeout, crash, fuel and memory failures with skipped cases cannot use an earlier approval to promote. The live synthetic container prepared-comparison regression passed (`/private/tmp/sese-failfast-container.log`). The full offline Rust suite passed 250 tests across 55 groups (`/private/tmp/sese-failfast-full.log`), with Docker-dependent tests returning early; Python discovery ran 76 tests with 21 skipped. This improves the existing replay path but does not enable unclassified candidate execution.

## Prepare reviews in an existing synthetic domain

Added `event-review-prepare` to assemble a prepared evaluation from an existing replay, the latest matching shadow observation, and the current champion without writing a review or promotion. An optional review-card file supplies context; a neutral default avoids another required input. The existing atomic decision command retains freshness and promotion checks. The preview still has a 100-observation limit, and unclassified candidates remain blocked. A focused integration test exercises rejection, a later observation, preparation without ledger writes, and approval on the existing database.

The full offline Rust suite passed 251 tests across 55 groups (`/private/tmp/sese-existing-domain-full.log`); Docker-dependent tests returned early without their environment. `git diff --check` passed.

## Continue review after 100 observations

The preceding existing-domain review preparation was progress, but it inherited a hard stop after 100 observations. A real measured synthetic stream reproduced the failure at observation 101. The review preview now returns the latest 100 observations, marks omitted older observations, and its guarded decision checks membership in the same recent window. Older observations remain accessible through paginated inspection. The 101-observation reproduction took 162 seconds on this machine; repeated whole-ledger verification is a suspected contributor and sustained-stream performance remains unvalidated. This does not enable unclassified replay or establish isolation.

The 101-observation regression passed after the change (170.79 seconds); a separate direct check against the original 101-observation database rejected its omitted oldest observation and recorded a rejection using the newest prepared review. The existing event-review suite passed 27 tests, the original prepared-review compatibility test passed, and `git diff --check` passed. The newly added assertion for omitted observations was compiled with the compatibility run and exercised separately against that database; it was added after the long regression process started.

## Index ledger evidence lookups

The preceding review-window fix was progress, but a 101-observation synthetic cycle took minutes. A controlled read-only preparation comparison on two copies of the same real 101-observation database found SQLite scanning `ledger_events` for a typical binding lookup; a non-unique `(kind, reference)` index changed this to an indexed search. Two local preparation runs measured 0.219/0.237 seconds without the index and 0.183/0.179 seconds with it, with identical JSON output (`/private/tmp/sese-index-compare-f_gwfl2l`). These are small, unreplicated local timings, not a sustained-stream benchmark; full-chain verification remains and unclassified candidate execution remains unavailable.

Writable open now creates the non-unique index transactionally and idempotently, without changing the v37 logical schema or evidence. Read-only open never creates it, so existing v37 databases remain readable. A migration regression first failed on a full table scan and checks that opening an existing database restores the index while preserving candidate bytes and the schema version.

The focused storage and event-review suites passed 36 tests. The full offline Rust suite passed 253 tests across 55 groups (`/private/tmp/sese-index-full.log`), including the complete 101-observation review regression with its omitted-observation check. Docker-dependent cases returned early without their environment. `git diff --check` passed.

## Reject added container groups before launch

The prior ledger-index change improved the synthetic workflow but did not address untrusted replay. The experimental container pre-start checker validated the primary user but did not inspect Docker's `GroupAdd`: a report adding group 0 passed its configuration check. The checker now rejects missing or nonempty supplementary-group configuration before starting the container. The original unit regression failed on that injected value before the fix and passed afterward. A live Docker Desktop test ran the fixed synthetic canary normally, then separately injected an altered entrypoint, loader environment, and `--group-add=0`; each altered configuration was rejected before the wrapper observed a worker start. No containers with the synthetic-probe label remained after the test.

Python discovery passed 76 tests with 21 skipped (`/private/tmp/sese-groupadd-python.log`); the specific live canary test passed separately. These checks rely on Docker's report and do not validate actual supplementary group membership, authenticate the daemon or image, establish general containment, or authorize unclassified candidates. No arbitrary candidate was run.

## Check worker identity in the running container

The prior pre-start `GroupAdd` check left the worker itself checking only effective UID/GID and effective capabilities. On the current Docker Desktop Linux VM, a fixed shell under the restricted profile reported four UID/GID slots of 65534, supplementary group 65534, and zero inheritable, permitted, effective, bounding and ambient capability sets. A live regression showed the earlier worker still returned a verdict with `--group-add=0` when launched directly.

The experimental worker now checks all those fields from `/proc/self/status` before reading input. A new image `sha256:f607d2bfaab75de7f8d802dae9822e285bd05da4ffbe61955002dc4967d87069` built successfully; the live normal-profile and changed-group tests passed, as did the launcher integration test with injected entrypoint, loader environment and supplementary group. Python discovery passed 77 tests with 22 skipped (`/private/tmp/sese-identity-python.log`); live tests were run separately. No labelled synthetic probe containers remained. The worker self-check and Docker reports are not independent attestation, and strict group expectations may reject other configurations. The image build used registry access and local builder cache. No unclassified candidate was executed, and containment and candidate-to-replay integration remain open.

## Integrated check of the identity-checked worker

Pushed checkpoint `c4d8639` and ran the existing real Rust-to-container prepared-review comparison with the newly built identity-checked image `sha256:f607d2bfaab75de7f8d802dae9822e285bd05da4ffbe61955002dc4967d87069` on the local Docker Desktop endpoint. The targeted test passed (`/private/tmp/sese-current-integrated-container.log`): it checked stale preparation rejection, executed the fixed synthetic comparison, then recorded a rejection from the refreshed preview. No labelled synthetic probe containers remained. This is integration evidence for fixed synthetic candidates only; the container comparison remains advisory, unauthenticated, and ineligible for promotion. The local synthetic replay is still the review evidence and unclassified candidates cannot be executed.

## Share replay case semantics across backends

The previous turn verified the fixed synthetic container path but it remained separate from ordinary replay. Both paths now use the same case loop for exact verdict comparison, per-case timing, and stop-after-execution-error behavior. The local replay retains its existing timing record and the container experiment retains its own receipt representation; no container result becomes promotion evidence and no unclassified artifact is executed. This prepares a common replay seam without asserting that the experimental executor is a trusted boundary.

Focused offline local replay and container-executor suites passed 22 tests (`/private/tmp/sese-shared-replay-focused.log`); Docker-dependent tests returned early there. A separate live Docker test using the current identity-checked image passed (`/private/tmp/sese-shared-replay-live.log`), including a two-case fixture where a valid wrong verdict was followed by a successful second case, and verified the experiment cannot pin a fixture. This is a fixed-synthetic integration check, not untrusted replay or proof of isolation.

The integrated prepared-review comparison also passed live against the same image (`/private/tmp/sese-shared-replay-demo-live.log`). The full offline Rust suite passed 253 tests across 55 groups (`/private/tmp/sese-shared-replay-full.log`); Docker-dependent cases returned early there. `git diff --check` passed.
