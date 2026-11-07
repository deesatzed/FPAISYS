# Candidate container boundary: evaluation plan

Status: **unimplemented and unverified**. This is an option to evaluate for the
existing typed-event worker, not permission to run untrusted candidates. The
Docker Desktop engine is reachable only with the required sandbox approval in
this environment; Podman has no configured machine. The existing synthetic
process path remains in use.

## Proposed boundary

Run one pinned, locally built worker image per evaluation. Send only the bounded
request over stdin and receive the bounded binary verdict over stdout. Do not
mount the workspace, database, host directories, credentials, Docker socket or
other host resources. Clear the environment and use a non-root UID, a read-only
root filesystem, no network, no added capabilities, `no-new-privileges`, and
explicit memory, swap, CPU and process-count limits. Retain Docker's default
seccomp and masked paths; do not use privileged mode or host namespaces.
Configure both memory and memory-plus-swap limits explicitly: Docker documents
that a memory limit alone may still permit swap. See [Docker run resource
constraints](https://docs.docker.com/engine/containers/run/) and [container
create options](https://docs.docker.com/reference/cli/docker/container/create/).

Use a verified immutable image identity and record the actual image and worker
identities alongside replay and shadow evidence. Do not equate a local digest
with trusted source or build provenance. Worker startup, execution, output and
cleanup must fit a bounded lifecycle. A timeout, daemon failure, unexpected
container configuration, unavailable resource control, malformed response or
unconfirmed cleanup must fail closed; do not fall back to the unsandboxed worker.
Avoid deleting or stopping any container that this evaluation did not create.
Do not rely on killing the `docker run --rm` client: a controlled local probe
showed the container continuing after client death. Track the created container
identity and verify its removal; handle uncertain creation and daemon loss
without returning successful evidence.

## Evidence required before use

1. Inspect the actual container configuration and kernel enforcement on the
   selected engine, including memory and swap, PID, user, filesystem, network,
   capabilities and seccomp settings. Confirm the chosen limits are supported,
   not merely accepted by the CLI.
2. Run known synthetic workers to exercise correct and incorrect fixtures,
   forbidden imports, fuel exhaustion, memory exhaustion, process termination,
   output flooding, blocked input and deadline cleanup. Test daemon interruption
   and concurrent evaluations, and confirm no container remains after failure.
3. From a controlled synthetic probe, attempt host-file access, network access,
   writes outside an explicit temporary area, process creation and access to the
   Docker socket. Record the host, engine, image digest, commands and results.
4. Test replay, review and promotion end to end: missing, changed or failed
   isolation evidence must not authorize promotion; prior approvals must not
   survive a newer failed replay. Keep old evidence inspectable without silently
   relabeling its execution environment.
5. Verify a fresh local build and an independent operator walkthrough. Review
   platform and kernel risks and document the remaining threat model before
   separately authorizing untrusted execution.

Docker Desktop uses a Linux VM and has its own host-sharing and daemon security
boundaries; these are not proof of resistance to container or kernel escape.
See [Docker Desktop platform security](https://docs.docker.com/faqs/platform/)
and [Docker Engine security](https://docs.docker.com/engine/security/).

The first restricted-container and memory-exhaustion observations are recorded
in [container-probe-results.md](container-probe-results.md). They do not satisfy
the full evidence requirements above or enable untrusted execution.
