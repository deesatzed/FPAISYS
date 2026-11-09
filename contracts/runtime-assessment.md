# Runtime feasibility assessment (preliminary)

## macOS sandbox probe (2026-11-08–09)

On the current Darwin host, `/usr/bin/sandbox-exec` failed to apply a profile
inside the tool sandbox (`Operation not permitted`) but a permissive profile
ran when launched outside it. A deny-by-default profile allowing process
execution, system-library reads and the worker executable aborted at startup
(exit 134); its missing entitlement or service was not identified. It is not a
working worker profile.

A separate `allow default` profile with network and file writes denied, plus
file-read denials for `/Users`, `/Volumes`, `/private`, `/Library` and
`/Applications` and a literal exception for the built event worker, ran the
fixed synthetic throttle request and returned `(1, 3.0, 4)`. Under the same
profile `/bin/cat` was denied access to a synthetic `/private/tmp` sentinel;
the unsandboxed positive control read that sentinel. Input and output were
opened by the parent before applying the profile. Network, other host paths,
inherited descriptors, process creation, and memory enforcement were not tested
in this experiment. The profile uses `allow default`, so unlisted operations
remain allowed. It is not a general filesystem policy, a selected containment
boundary, or authorization to run untrusted modules. The application does not
use this profile.

In a later fixed-synthetic probe, a deny-by-default profile importing Apple's
`system.sb` and allowing worker execution, metadata reads, selected system
paths, `/dev`, and the worker executable returned the expected throttle
verdict. With the same profile, a system plist was readable while the synthetic
private-file sentinel was denied; a test write did not create its target; and a
shell reported `fork: Operation not permitted`. A loopback listener received a
positive-control message outside the profile, while the sandboxed `nc` attempt
exited unsuccessfully and made no connection. The write probe also reported a
denied working-directory lookup, so its failure is not isolated to a single
write rule. Apple's imported system policy is not independently inspected or
pinned, and paths, inherited handles, memory limits, other network mechanisms,
process behavior, and runtime integration remain unverified. This is not
authorization to execute untrusted candidates.

Date: 2026-10-29. This is a source review and local availability check, not a security assessment or a working sandbox.

## Alternatives

- **Wasm with wasmi:** the Rust interpreter documents opt-in fuel metering and a store resource limiter. An empty linker can reject imports, avoiding WASI and host I/O capabilities. A fresh instance per evaluation avoids persistent guest state. Module parsing, validation, engine vulnerabilities, host process memory, and wall-clock time remain separate concerns. Fuel and parsing limits are disabled by default and must be configured explicitly. [Config](https://docs.rs/wasmi/latest/wasmi/struct.Config.html), [Store](https://docs.rs/wasmi/latest/wasmi/struct.Store.html).
- **Lua with mlua:** VM memory caps and periodic instruction hooks exist, but the hook is thread-specific and capability removal and coroutine handling require careful configuration. Native host functions and runtime defects remain outside a Lua instruction budget. [Lua API](https://docs.rs/mlua/latest/mlua/struct.Lua.html).
- **Wasm with Wasmtime:** supports fuel and resource limiting, but has a larger runtime/JIT configuration surface. The Wasmtime API also makes fuel opt-in. [Config](https://docs.rs/wasmtime/latest/wasmtime/struct.Config.html).

## Recommendation and gates

Prototype wasmi with no host imports, no WASI, explicit fuel, parser and memory/table limits, bounded module bytes and input/output, a fresh store and instance per evaluation, and a parent-supervised worker process with a wall-clock deadline. Pin the tested dependency and record its actual version. This choice is provisional until the spike demonstrates the actual API and limits on this machine. A child process alone is not an OS security sandbox: inherited credentials, filesystem and network access, memory, process limits and native runtime bugs need separate treatment. Do not claim strong containment without platform-specific enforcement and adversarial verification.

Required local tests: valid deterministic function; forbidden imports; infinite loop; memory growth; oversized or malformed module; trap; worker crash; deadline; fresh instance state; output validation; missing worker response. No promotion or production use based on this spike alone. Local checks found Rust/Cargo 1.98.0, no standalone wasmtime, wasm-tools or lua, and no cached wasmi/mlua crate. Dependency installation and a clean-environment build remain unverified.

## Local availability update (2026-10-30)

The current host is Darwin arm64. Docker CLI 29.5.2 is installed but its configured daemon socket was absent. Podman 6.1.4 is installed, but `podman machine list` returned an empty list and `podman info` could not connect to a Linux VM. These checks do not evaluate container isolation; neither service was available as a containment boundary. Earlier `sandbox-exec` attempts failed in the current environment as recorded in `PROGRESS.md`. No untrusted guest was run. A fresh source copy has since fetched pinned dependencies and passed the test suite; this does not verify isolation or a clean Git checkout.

## macOS inherited-descriptor reduction

The supervisor now launches the macOS worker with `posix_spawn` and `POSIX_SPAWN_CLOEXEC_DEFAULT`, explicitly connecting only standard input, output and null stderr. A local regression reproduced inheritance of a non-close-on-exec descriptor with the prior launch and passed after the change. macOS uses half-closed socket pairs rather than pipes; process-group cleanup remains best effort. Worker paths are used directly without PATH lookup. This is not a filesystem, network, process, memory or kernel isolation boundary. The Linux path has not received this change and may still inherit unrelated descriptors. Neither path is authorized for untrusted code.

The Linux supervisor now marks file descriptors 3 and above close-on-exec in the forked child with `close_range(CLOSE_RANGE_CLOEXEC)` and fails worker startup if the operation is unavailable or denied. This supersedes the Linux descriptor-inheritance limitation in the preceding paragraph. A Linux container regression observed an inheritable descriptor in the old worker and not in the new one; a seccomp test denying the syscall confirmed no worker started. This covers the tested launch path, not filesystem, network, kernel or full process isolation. The Linux host kernel and security policy must permit the operation.

## Interpreter parsing limits

The shared interpreter now enables Wasmi 0.40.0 `EnforcedLimits::strict()` before parsing, alongside the existing module byte cap, fuel and store limits. In that pinned version the strict profile includes limits such as 1,000 globals and 32 function parameters, plus limits on function, table, memory and segment counts. Synthetic boundary tests cover global and parameter counts. The profile may reject otherwise valid Wasm and does not establish a host-process memory limit or a security boundary; no untrusted execution is authorized.

## Fail-closed growth denial

The interpreter configures Wasmi to trap when a configured memory or table growth request is denied. This intentionally differs from the usual Wasm `-1` return and may reject modules that recover from allocation failure. A synthetic typed-event integration test shows that a guest which ignores a denied growth cannot produce a passing newer replay or promote a previously approved candidate. This does not establish an OS process-memory limit or protect against every kind of host allocation failure.

Published Wasmi 0.40.0 bypasses the limiter for an unrepresentable page count. The locally vendored patch described in `runtime-spike/vendor/wasmi/LOCAL-PATCH.md` presents that request to the limiter. Tests cover the oversized request, normal behavior without a limiter, and a promotion attempt after a failed synthetic replay. Event replay records currently say only `synthetic` and do not identify this runtime patch; runtime provenance for that path remains incomplete.

Typed-event evidence records a ledger-bound worker-file digest, and promotion checks matching digests, but this does not attest the worker's contents or map a historical digest to a Wasmi version. Scalar and window callers also select the worker. New scalar and window records therefore state that their runtime is unverified. Earlier version labels are historical assertions, not verified worker provenance; their exports explicitly flag this limitation.

## Local process-limit probe

On the current Darwin arm64 host, a disposable Python subprocess accepted a one-second `RLIMIT_CPU` and was terminated while spinning (shell exit 152). Separate subprocess probes attempting `setrlimit` for `RLIMIT_AS` and `RLIMIT_DATA` at 64 MiB and 4 GiB returned `EINVAL`; Python's `RLIMIT_AS` and `RLIMIT_RSS` constants are the same on this host. These observations do not establish a process-memory cap. Apple's archived [`setrlimit(2)` documentation](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/setrlimit.2.html) describes RSS as a preference under memory pressure and DATA in terms of the data segment, so neither should be assumed to bound all allocations. The probe did not modify the application or authorize untrusted code. Docker's configured daemon remains unavailable and Podman reports no machine. A verified filesystem, network and memory isolation boundary remains a prerequisite.

The three bundled synthetic workers now apply an `RLIMIT_CPU` hard ceiling of 10 seconds on macOS and Linux, without raising inherited limits, before reading input; setup errors make the worker exit. A subprocess test observes the limit and verifies termination under an inherited one-second limit on the current macOS host. Caller-supplied workers need not implement this policy. This is not a wall-clock, memory, filesystem or network boundary, and Linux enforcement remains untested here.

## Possible macOS application boundary

Apple's [sandbox documentation](https://developer.apple.com/documentation/security/discovering-and-diagnosing-app-sandbox-violations) says a directly launched helper inherits the app's sandbox and identifies an XPC service or separate helper app as designs for different capabilities. The current CLI is not packaged as either, and no such boundary has been implemented or tested here. A sandbox entitlement alone would not demonstrate a process-memory cap, correct launch enforcement, or isolation from the host's data. This is a candidate for further feasibility work, not an authorization to execute untrusted artifacts.
