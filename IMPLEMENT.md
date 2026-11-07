# Implementation plan

1. Validate runtime selection with a minimal isolated spike: compare Wasm fuel/memory/capabilities with Lua instruction and memory limits, including process-level containment, deterministic inputs, supported platform, dependency availability, and failure behavior. Record evidence and limitations before executing untrusted artifacts.
2. Define concrete ABI, canonical event encoding, migration schema, promotion evidence freshness and concurrency rules. Reconcile examples against `contracts/v1.md`; use full artifact digests, not the truncated example in `bld3.md`.
3. Build Rust host and tests for isolated execution, state ownership, champion/shadow separation and persistent telemetry; use synthetic data.
4. Implement real replay execution, persisted case evidence and fail-closed promotion, then local review and append-only history with hash verification and rollback.
5. Implement provenance-aware exports, demo, documentation and clean-checkout verification. Test negative paths and restart behavior throughout.

Each stage must supply reproducible commands and record actual results in PROGRESS.md. Do not import the sample master loop as a functioning implementation: it simulates replay and bytecode and does not enforce all gates.

## Typed-event telemetry reconciliation

The current typed-event window semantics differ from the source sketch (see `contracts/v1.md`). Before claiming the sketched telemetry, introduce a versioned representation that separates event metrics from measured champion duration, defines total versus bounded count and last-error behavior, and explicitly fixes percentile and failed-execution rules. Preserve existing v25 synthetic receipts without relabeling them as measured durations; add transition, restart, tamper and replay coverage for the new representation. The existing experiment remains synthetic until then.
