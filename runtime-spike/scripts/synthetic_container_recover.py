"""Explicit operator inspection and recovery for one synthetic intent."""
import argparse
import json
import sys

from synthetic_container_lifecycle import inspect_pending, recover_pending


def main():
    parser = argparse.ArgumentParser(description="Inspect or explicitly remove one observed synthetic container.")
    parser.add_argument("receipt")
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--inspect-only", action="store_true")
    mode.add_argument("--remove-observed-container", action="store_true")
    parser.add_argument("--container-id")
    parser.add_argument("--daemon-id")
    parser.add_argument("--docker-host")
    parser.add_argument("--image")
    parser.add_argument("--receipt-sha256")
    parser.add_argument("--actor")
    parser.add_argument("--docker", default="docker")
    args = parser.parse_args()
    if args.inspect_only:
        if any((args.container_id, args.daemon_id, args.docker_host, args.image, args.receipt_sha256, args.actor)):
            parser.error("identity arguments apply only to explicit removal")
        result = inspect_pending(args.receipt, docker=args.docker)
    else:
        if not all((args.container_id, args.daemon_id, args.docker_host, args.image, args.receipt_sha256, args.actor)):
            parser.error("explicit removal requires all identity arguments and actor")
        result = recover_pending(args.receipt, container_id=args.container_id, daemon_id=args.daemon_id,
                                 docker_host=args.docker_host, image=args.image, receipt_sha256=args.receipt_sha256,
                                 actor=args.actor, docker=args.docker)
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
