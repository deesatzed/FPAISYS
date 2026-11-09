"""Binary-protocol bridge for fixed synthetic artifacts; not an untrusted sandbox."""
import struct
import sys

from synthetic_container_lifecycle import SYNTHETIC_CANDIDATES, _synthetic_request, run_synthetic


def run():
    if len(sys.argv) != 7:
        raise ValueError("invalid synthetic container worker arguments")
    image, host, journal, docker, name, timeout = sys.argv[1:]
    data = sys.stdin.buffer.read(65_593)
    if len(data) < 56 or len(data) > 65_592:
        raise ValueError("invalid request length")
    count, mean, p99, last_error, timestamp, entity, metric, flags = struct.unpack("<QdQIQQdI", data[:56])
    event, window = (timestamp, entity, metric, flags), (count, mean, p99, last_error)
    candidate = None
    for name_candidate in SYNTHETIC_CANDIDATES:
        expected, _ = _synthetic_request(name_candidate, event, window)
        if data == expected:
            candidate = name_candidate
            break
    if candidate is None:
        raise ValueError("unknown synthetic artifact")
    result = run_synthetic(image, docker_host=host, journal_dir=journal, docker=docker,
                           candidate=candidate, event=event, window=window, timeout=float(timeout), probe_name=name,
                           require_worker_profile=True, request_bytes=data)
    sys.stdout.buffer.write(struct.pack("<IdI", *result["verdict"]))


if __name__ == "__main__":
    try:
        run()
    except (OSError, RuntimeError, ValueError) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
