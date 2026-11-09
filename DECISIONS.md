# Decisions

- Initial interface and workload: local CLI and synthetic inputs, as assumed in GOAL.md. External generation and training are deferred.
- Contract v1 uses full artifact SHA-256 identities, explicit evidence references, append-only outcomes and explicit approval. This replaces abbreviated hashes and in-place outcome updates in example code; it does not establish authenticity.
- Runtime selection remains open pending an actual feasibility and containment assessment. No untrusted execution is authorized until a containment boundary and limits are implemented and tested.
- Provisional runtime spike: Wasm via wasmi, chosen for explicit fuel and store resource limits and a capability-free interface. See `contracts/runtime-assessment.md`; actual containment, dependency availability and platform behavior remain unverified. This does not authorize untrusted execution before required controls are tested.
- Synthetic follow-up selection uses a separate, pre-decision replay case whose input is absent from the reviewed fixture. Both fixtures and results are visible before review; the selection is not blinded or independently authoritative and does not establish a real-world outcome or training eligibility. The demo reruns its reviewed fixture after selection to preserve the existing newest-replay promotion gate.
- For the proposed evidence-attestation path, use strict Ed25519 verification via pinned `ed25519-dalek` 2.2.0 and a versioned domain-separated message binding the authority ID, role, payload length and SHA-256 digest. This selects a signature primitive only; trust configuration, evidence import and eligibility remain unimplemented.
- Experimental authority configuration is an externally supplied, bounded `sese-authority-set-v1` JSON byte sequence pinned by a caller-provided SHA-256 digest. IDs and public keys must be unique; weak keys, unsupported roles and revoked signers fail closed. Digest matching does not establish pin custody or independence, and does not authorize training export.
- For synthetic evidence only, represent the operator's claimed authority relationships in a separately digest-pinned `sese-synthetic-trust-policy-v1` document bound to the authority configuration. Matching that declaration records an input to future eligibility work; it does not establish the operator's identity, policy custody, independence, comparability, outcome truth or training eligibility.


## Conditional synthetic training rows

Permit a single selected SFT, DPO or RLVR row only after verified ledger and signatures, a recomputed preselected synthetic follow-up, pinned authority and policy inputs, a covering checkpoint, and explicit caller acceptance of synthetic trust. DPO also requires the retained signed explicit pair and reviewer declaration; RLVR requires a pinned reward rule. This is eligibility conditional on the operator's declared assumptions, not independent verification of custody, identity, independence, comparability, model origin or real-world outcomes. A policy declaration or generic signed receipt alone remains insufficient. Preserve the unverified flags with the row and do not generalize this path to non-synthetic data.

## Failed selected follow-up replays

Reject selection of a failed synthetic follow-up replay. At promotion, recheck any existing selection so databases created before this gate cannot promote a candidate with a known failed follow-up. This does not make a passing follow-up independent or authorize untrusted execution.

## Follow-up selection and approval ordering

When a candidate has a selected synthetic follow-up, promotion requires that selection to precede the approval in the verified ledger. A later selection remains inspectable but cannot retroactively become evidence for an earlier approval; a new explicit review is required. Ordering alone does not prove the reviewer examined the selection.

## Read-only conditional training export

The training-export CLI opens only an existing schema v37 database with SQLite read-only flags. It does not initialize or migrate a database, including on rejected requests; migrate older databases as a separate operation. Other commands retain their existing migration behavior.

## Training trace and outcome chronology

Conditional training exports require the trace ledger event before the outcome event and the signed capture time no later than the signed observation time. Post-outcome trace records remain available for inspection but are excluded from these exports. Ledger ordering and signed timestamps do not authenticate when the underlying text was created.

## Synthetic training demonstration

Provide a separate interactive local demonstration using the explicit synthetic trace fixture and fixed public test keys. It requires an explicit review decision and a literal acceptance of synthetic trust, writes to a new directory, and labels the common operator and lack of independent custody. These keys and examples are for demonstration only and do not authorize non-synthetic evidence.

## Explicit synthetic export permission

A digest-pinned synthetic trust policy must explicitly set `training_export_allowed: true` before any conditional training export. Missing or false defaults to denial; inspection-only previews require a non-export policy and are explicitly ineligible. The demonstration policy denies export. This flag is an operator declaration, not proof of key custody, observer independence or outcome truth, and replacing a policy and its pin changes the operator assumption rather than authenticating it.

## macOS worker descriptor inheritance

Use macOS `posix_spawn` with `POSIX_SPAWN_CLOEXEC_DEFAULT` and explicitly configured standard streams to prevent unrelated descriptors from reaching the synthetic worker. Preserve bounded I/O and best-effort process-group cleanup. This is a macOS-specific capability reduction, not a sandbox; Linux retains its existing launch behavior and untrusted execution remains prohibited.

## Wasmi parsing limits

Enable the pinned Wasmi 0.40.0 strict parsing/compilation limits for the shared interpreter, in addition to the byte cap, fuel and store limits. This intentionally rejects some valid but structurally large modules and reduces parser exposure without claiming process or OS containment.

## Fail-closed Wasm growth

Trap on denied Wasm memory or table growth instead of returning the normal `-1`. This deliberate compatibility restriction prevents a synthetic candidate from ignoring a resource-limit denial and returning a successful verdict. It does not establish an operating-system memory limit or isolation.

Published Wasmi 0.40.0 bypasses the limiter for unrepresentable page counts. Use a local, documented patch to present these requests to the limiter, preserving the normal result when no limiter traps. This adds a dependency maintenance burden and must be reassessed on runtime upgrades. Event replay records do not yet identify the patch.

Scalar and window workers are caller-supplied, so new replay records must not assert a specific runtime version. Preserve historical labels as recorded and flag their runtime provenance as unverified in exports. Typed-event worker digests distinguish recorded files but are not runtime attestation.

## Synthetic worker CPU limit

On macOS and Linux, the three bundled workers set a ten-second hard CPU limit before reading their synthetic request, respecting tighter inherited soft and hard limits. Exit on setup failure. This defensive ceiling is separate from the wall-clock deadline, does not apply to arbitrary caller-supplied workers, and is not an isolation or memory boundary. Linux behavior has not been verified on this host.

## Synthetic container intent records

Require the experimental fixed-request container helper's caller to select an existing journal directory. Write and sync an owner-only pending record before Docker launch; mark it removed only after confirmed removal. Retain pending records for investigation rather than silently deleting them. Records are unauthenticated and do not implement recovery or authorize untrusted execution.

Expose read-only inspection of a pending synthetic intent: report a matching Docker observation only if the name, image and probe label agree, and treat other results as unknown. Do not infer absence or delete a container from this inspection. Neither local metadata nor labels authenticate ownership or the daemon.

New synthetic container intents also record Docker's reported daemon ID before launch. Inspection checks it before and after querying a container, and cleanup withholds a result on a detected daemon mismatch. Older intents without the ID remain unknown rather than being backfilled. The reported ID is not authenticated and checks can race with a context change.

Require an explicit Unix socket URI for the synthetic container prototype, pass it to every Docker operation and remove inherited `DOCKER_*` overrides. New v3 intents retain that endpoint; v1 and v2 records lack endpoint binding and remain unknown. A Unix socket and reported daemon ID do not authenticate the daemon or prevent socket replacement or forwarding.

Extend the container experiment to the existing `allow`, `throttle` and `quarantine` synthetic artifacts and typed synthetic event/window values. Reject arbitrary artifacts and invalid fields before launch. Return the validated worker verdict and selected artifact digest without treating the verdict as a passed replay. The image and execution are not attested, and this does not enable untrusted candidates or promotion.

Record the selected candidate, artifact digest and complete request digest in v4 synthetic container intents before launch. After confirmed removal, record whether a valid verdict was received and its output digest, or record failure. This is lifecycle provenance, not replay success or an authenticated receipt; preserve v3 pending records for read-only cleanup inspection without backfilling them.

Persist experimental container comparisons as separate owner-only v2 reports, with pending, running, completed and error states, timestamps and references to individual lifecycle receipts. An interrupted or failed report cannot count as a passed replay. These records are unauthenticated and are not imported into the application ledger or promotion gates; integrating them requires an execution boundary and provenance model suitable for those gates.

Reject a synthetic container image when Docker reports declared volumes or the volume configuration cannot be confirmed. Check the daemon's reported ID again after inspection and before launch. A read-only root alone does not prevent writable image-declared volumes. The inspection and daemon ID are unauthenticated and cannot establish containment against a dishonest daemon.

Allow an informational comparison of completed synthetic container reports with an application review preview. It compares their candidate, fixture, inputs and verdicts and returns the preview checkpoint, but does not import results or authorize promotion. Report contents and lifecycle receipts are not authenticated by this comparison.

The comparison command also requires each linked v4 lifecycle receipt to agree with the reconstructed built-in artifact, request and verdict, and to record confirmed removal. Reject missing, repeated, malformed or inconsistent receipts. These checks establish consistency among local records, not authenticity or a trusted execution boundary.

Allow an explicit caller-asserted ledger event linking a replay, observation, preview checkpoint and two container-report digests. The event is a provenance pointer, not an application replay, review, attestation or promotion input. Guard append against a changed ledger head. The report digests do not bind the referenced receipt bytes; retain and assess those files separately.

Provide a single synthetic operator command to run container fixture and shadow comparisons from an existing app preview, verify their local receipts and append the guarded caller assertion. Require all receipts to report the same daemon ID. The command accepts only fixed built-ins and does not promote or replace the app's replay and observation; the reported daemon ID is not authentication.

For this single-command workflow, retain a bounded snapshot of the report and lifecycle receipt JSON bytes inside the caller-asserted ledger event. Preserve the earlier digest-only assertion API. The capture is not proof of execution or promotion evidence, and the ledger event does not authenticate its caller or the captured files.

Provide a read-only recheck of a captured bundle against an externally supplied checkpoint covering the assertion. Verify ledger, replay, fixture and observation bindings plus captured report and receipt consistency. This reuses the Python comparison implementation and cannot authenticate execution or reconstruct historical file permissions; it does not change promotion eligibility.

Permit the Rust application to run only fixed synthetic built-ins through the experimental container lifecycle and store the fixture, case results, errors, timestamps and successful receipts as a distinct `container_replay_experiment` ledger event. Guard the write against ledger changes during execution and recheck fixture/result consistency on inspection. This event is not an ordinary replay or promotion input. Its ledger sequence and ordinary replay IDs occupy different namespaces and can coincide; operators must not use the sequence with `event-pin`. The helper, image and daemon are not authenticated, and untrusted artifacts remain prohibited.

Provide a separate measured-v2 synthetic container shadow stream using one host-owned prior state for both fixed built-ins. Store successful comparisons, state transitions and lifecycle receipts in distinct checkpoint-guarded ledger events; validate transitions and receipt digests on reopening. A failed attempt does not advance the stream or create a shadow event, and its reported receipt paths must be retained for inspection. These ledger sequences are not ordinary observation IDs or promotion evidence. The daemon and execution remain unauthenticated; the authoritative review and promotion paths are unchanged.

Supersede the preceding failure-state rule: record failed container attempts as versioned, non-promotional shadow events when the ledger and champion guards remain current. A failed champion attempt advances measured-v2 state with error 500 and does not launch the shadow; a failed shadow attempt advances state using the successful champion's duration and retains its completed receipt. Return the event sequence and available failure receipt paths. Invalid requests, changed helper identities and stale ledger or champion guards do not commit an event or advance state. A recorded error and local receipt are not proof of cleanup, execution authenticity or containment.

Serialize cooperating synthetic lifecycle calls with a bounded lock on their journal directory. Before creating another intent, reject pending, malformed, unsafe or unrecognized synthetic receipt records; permit only structurally valid v4 records marked removed. This guard is limited to an unchanged shared directory and cannot authenticate records or constrain different journals and non-cooperating processes. Do not infer absence from a missing receipt or automatically remove containers; operator recovery remains unresolved. A Rust bridge failure before its intent is written may name a receipt path that does not exist.

Permit an explicit operator-requested recovery of a currently observed pending v4 synthetic container. Require the exact receipt digest, container ID, image, endpoint and daemon ID plus an operator label; recheck observation before removal and preserve a durable owner-only recovery intent. Mark the original receipt removed with a failed result only after the exact removal and reported absence on the same reported daemon. Unknown or changed identity and unsuccessful cleanup leave the receipt pending. This is a caller-directed destructive action and local assertion, not authenticated ownership or execution; no container is automatically recovered and no promotion evidence is created.

For new recovery attempts, write a v2 `started` intent before removal and a `completed` record bound to the final receipt digest only after that receipt update succeeds. Cooperating journal launches retry directory sync and reject an incomplete v2 recovery, even when the receipt appears removed. A matching completed recovery resolves prior started attempts for the same source receipt and container identities. Historical v1 audits are not reinterpreted or backfilled. These checks do not prove power-loss durability or authenticate any local record.

Revalidate captured successful container replay and shadow receipts using the same strict parser and exact fixed-artifact request and output digests. Require distinct container identities and a consistent reported daemon across each comparison. Existing inconsistent experimental records remain preserved but fail inspection. Receipt consistency does not establish historical file permissions, daemon or image authenticity, containment, or promotion eligibility.

Expose a read-only advisory comparison between an ordinary review preview and explicitly selected stored container replay and shadow events. Check fixture and candidate identity, current champion generation, exact prior state, event and verdicts, as well as receipt and cross-experiment container identities, in one verified database snapshot. Return its checkpoint and mark the result unauthenticated and ineligible for promotion. It does not change the ordinary review or promotion path; independently measured stream latencies may prevent an exact comparison.

Permit an application-initiated fixed-artifact container comparison using the exact prior window and event from a verified measured-v2 observation in the current champion tenure. Guard the append against a changed ledger or champion and retain successful outputs in a distinct observation-linked experiment; failed attempts return receipt paths without appending. This does not mutate the original observation or stream and cannot authorize promotion. Allow the advisory review preview to compare this event as well as a container shadow-stream event, while rejecting observations predating the current champion transition. Container, daemon and execution authenticity remain unverified.

Supersede the preceding failure-retention rule: record a versioned, non-promotional observation-linked failure event when the checkpoint and champion remain current. Champion failure records the error and no verdicts; shadow failure also retains the successful champion receipt. Return the event sequence and available receipt paths, leave the source stream unchanged, and reject failure events in the successful comparison preview. A failed or missing receipt does not prove container creation, removal or execution.

Add a fixed, allowlisted `window-probe` synthetic guest whose score uses prior count, mean and p99 and whose reason uses prior last error. Keep Rust and Python artifacts byte-identical and exercise it in replay and observation comparison. This checks selected host-state plumbing; it does not authorize arbitrary artifacts or change the unauthenticated, non-promotional status of container experiments.

On Linux, mark all descriptors above standard input, output and error close-on-exec in the forked worker using `close_range(CLOSE_RANGE_CLOEXEC)`. Preserve the spawn error pipe until exec; refuse worker startup if the syscall is unavailable or denied, rather than silently inheriting descriptors. This is one launch hardening measure, not an isolation boundary or permission to run untrusted candidates.

Use a distinct versioned measured-v2 fixture in the manual operator walkthrough, preserving the existing legacy fixture and its interpretation. Demonstrate two pre-review events, verify measured stream receipts with the independent verifier, and keep the observed timings labeled as synthetic process measurements rather than performance results.

Offer noninteractive preparation for the existing fixed synthetic cycle and a single subsequent explicit decision command. Save the complete review preview and its checkpoint in the preparation output; reject altered or stale evidence before recording a decision. Approval attempts promotion after recording the review and reports the retained review if promotion fails; rejection leaves the champion unchanged. This reduces bookkeeping while preserving the current human-approval gate and does not implement automatic candidate generation or untrusted execution.

Supersede the preceding partial-write rule: prepared approval and promotion commit in a single immediate SQLite transaction. A failed promotion rolls back the approval and its ledger event; prepared rejection records only the rejection. Existing separate review and promotion APIs remain available.

Add a local deterministic threshold-rule compiler as an initial candidate-generation input. Compile only a fixed Wasm template with a finite bounded numeric parameter, then store the artifact through unclassified event intake. Do not execute, synthetically classify or promote the result. The returned proposal parameters are not yet ledger-bound provenance, and this local compiler is not an autonomous model or a verified execution boundary.

For fixed synthetic container probes, create the container first and require Docker's reported identity, unstarted state, user, network, mounts, capabilities and resource limits to match the requested profile before starting it. Mismatch or unavailable inspection fails closed and attempts removal of the identified container. Docker and its report remain unauthenticated; this does not yet permit arbitrary candidates or make container evidence promotional.

Use one evaluation deadline across the synthetic journal lock, Docker preflight and worker output. Give the Rust supervisor a separate 30-second cleanup grace after that deadline; it is not a guarantee of cleanup after daemon, host, process-creation or filesystem failure. Retain pending receipts when identity or cleanup cannot be confirmed.

Represent the synthetic lifecycle's read-only preflight as a versioned `not_created` record. A valid preflight record permits retry; after preflight succeeds, durably replace it with the existing pending intent before calling Docker create. Pending or malformed records remain blocking, including existing historical pending records. The journal is an unauthenticated local assertion and does not establish power-loss durability or protect against a non-cooperating writer.

Build the experimental Linux worker image with a fail-closed self-check of selected process, cgroup, root-mount and network conditions before reading the guest request. Keep the ordinary local worker mode for existing synthetic tests. The helper does not authenticate the image or require this entrypoint, so a passing self-check is not a sandbox attestation or authorization for untrusted candidates.

Allow the local threshold compiler to infer its parameter from a labeled synthetic fixture when allow and throttle examples are strictly separable and the template can represent every expected score and reason. Record the source fixture identity in the returned proposal, label the fit as static and in-sample, and keep the generated artifact unclassified and unexecuted. This deterministic fit is not model-assisted generation or independent replay evidence; proposal provenance is not yet ledger-bound.

Supersede the preceding provenance limitation for fixture-derived proposals: atomically retain the exact supplied fixture, its byte and semantic digests, generator identity, threshold bits and candidate digest in a ledger event alongside candidate insertion. Provide a read-only inspection that recomputes the link. This records caller-supplied source, not authenticated authorship, execution or validation; generic ledger verification does not recompute proposal semantics.

Require the application synthetic-container bridge to check the reported worker entrypoint and absence of extra command arguments before starting the container. Direct lifecycle experiments may still use other synthetic fault-injection images. This is a reported-configuration check, not authentication of the image, daemon, self-check or kernel; generated and arbitrary candidates remain ineligible for execution and promotion.

Also require the pinned worker image's exact PATH-only reported environment in that bridge profile. This excludes injected dynamic-loader settings before the self-check, but a dishonest daemon or substituted image can still falsify the report or bypass the worker. A future base-image environment change needs an explicit profile update and verification.

Offer a single `event-container-evaluate` operation for an existing measured synthetic review: check the fixture and source observation, execute the fixed candidate's container replay and the observation's champion/shadow comparison, and return the existing consistency preview. Preserve the separate non-promotional records, report their sequences on later failure, and do not infer image or execution authenticity or approve promotion.

When that operation receives an existing prepared review, validate it before launch and return a refreshed prepared review only if the two experimental events are the only ledger additions and the underlying review is unchanged. Permit `event-review-prepared` to consume the returned comparison directly. The ordinary explicit decision and promotion checks remain authoritative; experimental records remain advisory.

Report a non-executing structural check automatically with event intake, using the existing bounded Wasmi parser configuration to inspect imports and the event export signature. Store incompatible bytes as before. A compatible result is not behavioral validation, execution authorization, or promotion evidence; parsing occurs on the host and does not establish parser security.

Forward the application bridge's original immutable request through the synthetic container lifecycle after checking exact equality with the allowed built-in and typed inputs. Hash the artifact slice from that request. This preserves the built-in-only restriction; it does not authorize unclassified candidate execution or authenticate the worker.

Normalize the built worker executable's filesystem timestamp in the experimental Dockerfile. For repeatability checks, pass `SOURCE_DATE_EPOCH=0` explicitly to the build. Matching worker bytes, layers or configuration across local builds is not authenticated source or builder provenance, and the observed image IDs still differ.

Record per-case local synthetic event replay timing in a versioned ledger event appended atomically with its replay. Bind it to the replay event hash and ordered case IDs, report wall-clock start and finish separately from monotonic elapsed nanoseconds, and verify the link independently. Preserve older replay hashes and report their missing timing as unknown rather than backfilling it.

Stop local synthetic event replay after the first worker execution error, retaining the failed case and marking every remaining case as skipped. Continue after a valid but unexpected verdict. Version timing records as v2 so skipped cases have null times; retain v1 interpretation for older runs that could execute additional cases after an error. Both verifiers check v2 skip ordering.

Keep review previews bounded to the latest 100 observations in a domain, ordered chronologically, and mark a preview when older observations are omitted. The guarded review transaction accepts only an observation in that same recent window at the checkpoint; older observations remain available through paginated inspection. This removes a hard stop for long-lived domains without expanding the evidence represented in a single decision.

Create an idempotent non-unique `(kind, reference)` ledger index in the existing writable-open transaction. The index is a rebuildable query optimization, does not change the v37 evidence format, and must not hide duplicate ledger bindings. Read-only opens do not create it; old databases remain readable without migration.

Require the experimental synthetic container pre-start inspection to report no supplementary `GroupAdd` entries. Missing or nonempty values fail closed and trigger the existing cleanup path. This is a reported Docker configuration check, not proof of group membership, daemon authenticity, or a containment boundary.

For the experimental Linux worker profile, fail before reading input unless `/proc/self/status` reports UID/GID 65534 in all four slots, exactly supplementary group 65534, and empty inheritable, permitted, effective, bounding, and ambient capabilities. This deliberately restricts compatible environments and supplements, but does not authenticate, the launcher inspection; the synthetic-only execution restriction remains.
