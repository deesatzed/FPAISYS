# Container isolation probe results

On 2026-11-07, a newly built synthetic worker image included a fail-closed
in-process Linux check before reading the guest request. It checked UID/GID,
effective capabilities, no-new-privileges, seccomp filter mode, selected cgroup
limits, the root mount, active non-loopback interfaces, IPv4 routes and the
usual Docker socket path. Fixed synthetic input succeeded under the requested
profile, and the worker refused it with bridge networking, a different memory
limit, a writable root, or no-new-privileges disabled. The same image succeeded
through the Rust container executor. The first
check incorrectly rejected inactive tunnel interfaces on this host and was
corrected to check active interfaces and routes. These checks are observations
by the worker itself, do not authenticate the image, cover all access paths or
prove kernel isolation, and do not authorize untrusted candidates.

On 2026-11-07, the synthetic lifecycle was changed to create a container,
inspect Docker's reported configuration while it is unstarted, and start it
only if the requested identity, user, mounts, network, capability and resource
settings match. A wrapper changed the network to `bridge`: before the change
the synthetic worker ran successfully; afterward the helper rejected the
configuration before starting it and recorded verified cleanup. Normal fixed
synthetic workers and the lifecycle, replay and shadow tests also passed on
Docker Desktop 29.5.2. This is a check of Docker's own unauthenticated report,
not independent verification of kernel enforcement or authorization for
arbitrary candidate execution.

On 2026-11-06 a fixed synthetic `window-probe` ran through the local Docker
worker with two distinct prior windows. Its verdict uses the supplied count,
mean, p99 and last error; both results matched and had separate receipts. A
separate test matched a measured replay and a stored application observation
through the advisory container review preview. These checks cover the named
synthetic inputs and do not authenticate execution or permit promotion.

This is evidence from controlled synthetic probes, not a verified sandbox or
authorization to execute untrusted candidates. The image is a temporary local
build of the typed-event worker; it is not integrated into the application.

On 2026-11-02, Docker Desktop 29.5.2 on Darwin arm64 ran image
`sha256:503eae959084b387f6f1bef166211d5ce4410b458ae0aa7c9b077b6037b1fc08`.
The created test container's inspected configuration showed non-root UID/GID
65534, network `none`, read-only root, `CapDrop: ALL`, no-new-privileges,
16-process limit, 128 MiB memory and combined memory-plus-swap limit, one CPU,
`Privileged: false`, and no mounts. Full inspect output is retained at
`/private/tmp/sese-oci-probe-0c8dzf5y/inspect.json` while that temporary
directory exists.

Inside a disposable container with those settings, `/proc/self/status` reported
zero effective capabilities, `NoNewPrivs: 1`, and `Seccomp: 2`. The cgroup
files reported `memory.max=134217728`, `memory.swap.max=0`, `pids.max=16`,
and `cpu.max=100000 100000`. A shell's attempt to create a file in `/tmp`
failed, `/var/run/docker.sock` was absent, and `/proc/net/route` showed no
routes. These are observations of specific checks, not a comprehensive proof
of filesystem or network isolation. The image includes `/dev/shm`; writable
temporary mounts and all other accessible paths were not exhaustively tested.

A separate disposable container used the same restrictions with a 32 MiB
memory and memory-plus-swap limit. A controlled AWK allocation was terminated;
Docker reported `OOMKilled: true` and exit code 137. This demonstrates one
kernel memory-limit failure, not how the application records an OOM or handles
container cleanup after daemon failure. Cleanup traps attempted to remove the
two test containers, but removal was not independently verified. The raw outputs
are in the temporary directory above.

The repository now includes `runtime-spike/Dockerfile.worker` and a matching
Dockerfile-specific ignore file. A local build completed and returned the same
synthetic verdict. Its platform manifest matched the earlier temporary image,
while the image index differed because the build attestation manifest differed.
This is not a clean-checkout or independently attested reproducibility test.

On the same Docker Desktop host, two labelled disposable containers tested a
synthetic host sentinel. A positive control read the sentinel when it was
explicitly bind-mounted read-only; the unmounted container found neither that
container path nor the sentinel's host path. No containers with the probe
label remained afterward. This checks the named file and paths, not every
possible host access path.

A separate loopback-only host test server received a synthetic message from a
restricted container on Docker's bridge network. A second container with the
same restrictions and `network=none` tried the identical resolved host address
and port and failed with `Network is unreachable`; the server received only the
positive-control message. No containers with that probe label remained. The
temporary probe script is `/private/tmp/sese-oci-probe-0c8dzf5y/network_probe.py`.
This checks one TCP path on this Docker Desktop installation, not all network
protocols, loopback behavior, host services or kernel escape paths.

In a controlled lifecycle probe, a `docker run --rm` client launched a
restricted container running a fixed 20-second sleep. After the container ID
was recorded and the container was observed running, the probe killed the
client with `SIGKILL`. Docker still reported the container running. The probe
then removed that exact container ID and confirmed it was no longer listed.
Thus `--rm` and client termination alone are insufficient for deadline cleanup.
The probe script is `/private/tmp/sese-oci-probe-0c8dzf5y/client_crash_probe.py`;
it tested one client-kill case, not daemon loss or all creation races.

The separate `runtime-spike/scripts/synthetic_container_lifecycle.py` prototype
now runs a fixed synthetic request with a caller-selected full image identity,
bounded verdict output, a deadline and explicit removal of the captured
container ID before success. A real-engine success test and a sleep-image
deadline test verified the returned error or verdict and confirmed each known
container was no longer listed. Ambiguous creation and daemon loss still report
unconfirmed cleanup; the helper is not integrated with application evidence and
does not authorize untrusted code or images.

Additional real-engine tests used a controlled output-flood image to confirm
that more than 16 bytes are rejected and its container is removed. A test
wrapper deliberately failed the Docker removal command after the known worker
returned successfully; the helper withheld the verdict and reported the
container ID as cleanup-unconfirmed. The test then removed that exact container
and verified it was absent. This is injected command failure, not a Docker daemon
outage or proof of cleanup after host or client termination.

The synthetic lifecycle prototype now records a pending intent in a
caller-selected directory before launching Docker and updates it only after
confirmed removal. A controlled test killed the lifecycle parent while its
sleep container was running: the pending record remained, and its name matched
the observed container label. The test then removed the identified container.
This demonstrates recoverable metadata for that case, not automatic recovery,
receipt authenticity, power-loss durability, or safe handling of a changed
Docker daemon or a malicious local user.

The prototype's read-only `inspect_pending` API now checks a bounded,
owner-only pending record and compares its name, image and label with Docker's
reported container metadata. A parent-death test observed its matching running
container, while an altered image in the record returned `unknown`; the test
performed its own explicit cleanup afterward. An unavailable Docker client also
returns `unknown`. This does not authenticate the daemon, receipt or container,
prove absence, or stop or remove anything.

New synthetic intent records also contain Docker's reported daemon ID. The
helper checks it before and after inspection and around cleanup; legacy records
without it remain `unknown`. A real-engine test used a wrapper to report a
different daemon ID before cleanup and confirmed the helper withheld its result
without issuing removal; the test then removed its own container. This is a
reported-ID mismatch test, not daemon authentication or proof that a context
cannot change between checks.

The prototype now requires an explicit Unix socket URI and passes it via
Docker's `--host` flag for launch, inspection and cleanup, while removing
inherited `DOCKER_*` environment overrides. New v3 intent records retain the
endpoint; v1 and v2 records are `unknown` for inspection rather than acquiring
an inferred endpoint. A real-engine test succeeded with conflicting
`DOCKER_HOST` and `DOCKER_CONTEXT` environment values, and a modified recorded
socket produced `unknown`. This reduces default-context drift but does not
authenticate the socket, prevent replacement or prove that a local socket is
not a proxy. Docker documents the relevant [CLI configuration precedence](https://docs.docker.com/reference/cli/docker).

The prototype now supports the three Rust-registry built-in synthetic artifacts
and typed synthetic event and window values. A cross-language test compares
their exact Wasm bytes with the Rust registry. Real Docker tests exercised all
three candidates with a non-default event and window, but these built-ins ignore
the window fields; that test does not show guest use of window state. The helper
returns valid worker output rather than assigning replay success, and does not
prove the image actually executed the selected bytes.

New v4 lifecycle intents record the selected built-in name, artifact digest and
request digest before launch. After confirmed container removal, they record a
valid-verdict or failed status and, for a valid verdict, its output digest. The
valid-verdict status checks format, not expected fixture results. Earlier v3
pending records remain inspectable for cleanup without being relabeled. These
local records are not ledger-bound or authenticated; a digest does not prove
execution, protect against rewriting, or reconstruct omitted request values.

Still required: complete and bind a bounded fail-closed lifecycle to replay,
shadow and promotion;
expand the controlled host-access and network tests; test deadline, OOM,
daemon failure, output and process limits, cleanup and concurrent
runs; establish image and runtime provenance; and perform an independent
security assessment. Docker's VM, daemon and host remain in the threat model.
