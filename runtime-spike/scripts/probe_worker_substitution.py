"""Reproduce worker-image substitution with a fixed synthetic verdict.

This creates and removes a disposable image and container on the selected Docker
endpoint. It does not execute registered candidates or validate containment.
"""
import argparse
import pathlib
import shutil
import struct
import subprocess
import tempfile

from synthetic_container_lifecycle import run_synthetic


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", required=True, help="existing synthetic worker image sha256")
    parser.add_argument("--docker-host", required=True, help="Docker Unix socket URI")
    parser.add_argument("--worker-sha256", help="independently supplied expected worker digest")
    args = parser.parse_args()
    docker = ["docker", "--host", args.docker_host]
    container = image = None
    with tempfile.TemporaryDirectory(prefix="sese-image-substitution-") as directory:
        root = pathlib.Path(directory)
        worker = root / "worker"
        verdict = struct.pack("<IdI", 1, 3.0, 4)
        octal = "".join("\\%03o" % byte for byte in verdict)
        worker.write_text('#!/bin/sh\ncat >/dev/null\nprintf "' + octal + '"\n')
        worker.chmod(0o755)
        journal = root / "journal"
        journal.mkdir()
        try:
            container = subprocess.check_output(docker + ["create", args.image], text=True, timeout=15).strip()
            subprocess.run(docker + ["cp", str(worker), container + ":/event-worker"], check=True, timeout=15, stdout=subprocess.DEVNULL)
            image = subprocess.check_output(docker + ["commit", container], text=True, timeout=30).strip()
            if args.worker_sha256:
                result = run_synthetic(args.image, docker_host=args.docker_host, journal_dir=journal,
                                       timeout=30, require_worker_profile=True,
                                       expected_worker_sha256=args.worker_sha256)
                if result["verdict"] != (1, 3.0, 4):
                    raise RuntimeError("unexpected reference verdict")
                marker = root / "impostor-started"
                wrapper = root / "docker-wrapper"
                docker_path = shutil.which("docker")
                if not docker_path:
                    raise RuntimeError("Docker CLI unavailable")
                wrapper.write_text("#!/usr/bin/env python3\nimport os,sys\na=sys.argv[1:]\n"
                                   + "if len(a)>2 and a[2]=='start': open(" + repr(str(marker)) + ",'w').close()\n"
                                   + "os.execv(" + repr(docker_path) + ",[" + repr(docker_path) + "]+a)\n")
                wrapper.chmod(0o700)
                try:
                    run_synthetic(image, docker_host=args.docker_host, journal_dir=journal,
                                  timeout=30, require_worker_profile=True, docker=str(wrapper),
                                  expected_worker_sha256=args.worker_sha256)
                except RuntimeError as error:
                    if "worker pin unconfirmed or mismatched" not in str(error):
                        raise
                    if marker.exists():
                        raise RuntimeError("substituted worker was started")
                    print("Reference worker accepted and substituted worker rejected before launch.")
                else:
                    raise RuntimeError("substituted worker unexpectedly accepted")
            else:
                result = run_synthetic(image, docker_host=args.docker_host, journal_dir=journal,
                                       timeout=30, require_worker_profile=True)
                if result["verdict"] != (1, 3.0, 4):
                    raise RuntimeError("unexpected impostor verdict")
                print("Substituted executable accepted; fixed synthetic verdict:", result["verdict"])
            print("The Docker daemon and image are unauthenticated; this does not authorize candidate execution.")
        finally:
            if container:
                subprocess.run(docker + ["rm", "-f", container], check=True, timeout=15, stdout=subprocess.DEVNULL)
            if image:
                subprocess.run(docker + ["image", "rm", image], check=True, timeout=20, stdout=subprocess.DEVNULL)


if __name__ == "__main__":
    main()
