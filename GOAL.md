# GOAL: Build the Symbiotic Self-Evolving Software Engine

## Purpose and intended user

Build S-SESE, a local-first system for a software operator who wants to evaluate a proposed change to an event-processing rule while retaining control over what runs. The intended workflow observes a local event stream, evaluates a candidate against fixed replay cases, compares it with the current rule in shadow mode, and presents evidence for a human decision. A durable record connects the candidate, its tests, the decision, and later observed outcomes. The claimed value is safer, more auditable iteration; it is a hypothesis until measured with real users and workloads.

## Source assessment and assumptions

- `bld1.md` proposes the contracts, SQLite bus, Rust host, and champion/shadow runtime. `bld2a.md` proposes a Python airlock and oracle; `bld2.md` describes governance and outcome-based exports; `bld3.md` sketches the complete loop.
- These are design sketches containing code examples, not an existing application. The replay example explicitly simulates success, and the master loop uses simulated bytecode and outcomes. The 95% rejection, zero-leakage, latency, isolation, and training-benefit claims are unverified. The sketches also require reconciliation of imports, schemas, state transitions, and units before integration.
- Assume the first user is a technical operator, the first workload is synthetic and local, and a CLI is an acceptable initial interface. Record consequential choices and open questions as the build proceeds; do not represent assumptions as validated demand.

## Outcome

Deliver a reproducible local application that runs a complete, inspectable candidate-evaluation cycle on a synthetic event-processing workload. The operator can start the host, register a candidate, run real deterministic replay cases, compare champion and shadow results, review a three-column decision, explicitly approve or reject promotion, record an outcome, and inspect the resulting history. Rejection, failed replay, runtime failure, or missing approval must leave the champion unchanged.

## Scope and sequence

1. Reconcile the four documents into explicit, versioned contracts and an implementation plan. Define candidate identity, telemetry units, replay results, promotion states, decision/outcome provenance, and a SQLite migration strategy. Explain any divergence from the examples.
2. Implement a Rust host with host-owned event state, one constrained plugin runtime, champion/shadow execution, bounded resource use, and persistent telemetry. Select Lua or Wasm based on a documented feasibility and isolation assessment; do not claim the other runtime is supported.
3. Implement local candidate intake and a replay harness that actually executes candidates against versioned fixtures with expected results. Persist candidate identity, fixture version, per-case results, errors, and timestamps. A static prefilter may reject candidates but cannot establish correctness or replace execution.
4. Implement governance and a usable local operator interface: a three-column review, explicit decision and promotion, append-only decision/outcome events, and independently verifiable hash-chain integrity. Preserve pending, validated, and falsified outcomes as distinct states. If blinded review is implemented, test that the proposal is inaccessible until the independent decision is recorded.
5. Implement outcome-linked local exports where records and labels have sufficient provenance; exclude pending or ineligible records and identify synthetic fixtures. Define SFT/DPO/RLVR eligibility separately, and do not fabricate preference pairs or claim model improvement. Provide a documented end-to-end demonstration and onboarding commands.

The intended architecture also includes optional model-assisted generation, external services, automatic promotion, production traffic, and MLX training. Treat these as subsequent milestones requiring separate decisions and evidence; the first completion target does not depend on accounts, cloud models, real proprietary data, or model training.

## Safety and provenance

- Use synthetic fixtures by default. Do not send source code, sensitive data, credentials, or private mappings to external services. Regex redaction and the proposed airlock are not proof of privacy.
- Treat candidate code as untrusted. Disable unnecessary capabilities, impose execution and memory limits, test failure paths, and document remaining isolation limits. Do not run untrusted candidates outside the selected containment boundary.
- Require explicit human approval for promotion in this milestone. A hash chain detects some modifications but is not a digital signature or proof of authenticity; document its threat model and anchoring limitations.
- Preserve provenance and distinguish test observations, simulations, human decisions, and real-world outcomes. No clinical or production-use claim follows from synthetic tests.

## Proof of done

- A clean checkout can install dependencies, build, run the local demo, and execute the documented test suite using commands supplied in the project README; record actual command output and tool versions.
- Automated tests show that a correct candidate passes real fixtures; a wrong candidate fails; an empty or malformed replay pack fails closed; and crashes, timeouts, resource exhaustion, and unavailable evidence cannot promote a candidate.
- Integration tests demonstrate isolated shadow comparison, explicit approval and rejection, promotion and rollback, persistence across restart, and a verifiable decision/outcome history. Tampering with recorded events is detected within the stated threat model.
- Tests verify pending and ineligible outcomes are excluded from each export; exported records retain candidate, decision, outcome, and synthetic-data provenance. Test both positive and negative cases.
- A fresh operator can follow the documented demonstration from input through review and inspect the artifacts and limits. Report what was actually measured separately from untested performance, security, user value, and production readiness.

## Working rules and stop conditions

Read the project control files and source documents before implementation. Work in small increments, preserve the original specifications, document decisions and progress, and run focused checks before the full suite. Do not weaken tests or substitute mocks for the execution and persistence required above. Discover actual commands from the created project rather than assuming they already exist.

Stop for missing credentials required by an approved task, sensitive-data exposure, destructive action, production deployment, unresolved legal or compliance uncertainty, repeated failures after mitigation, or a product decision that materially changes scope. Report the blocker and evidence. Mark this goal complete only after every applicable proof item passes; list deferred milestones and remaining risks explicitly.
