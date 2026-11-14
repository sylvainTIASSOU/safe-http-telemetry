#!/usr/bin/env python3
"""Install a pinned DB-IP City Lite archive outside the Cargo build process."""
import argparse
import gzip
import hashlib
import os
from pathlib import Path
import re
import tempfile
import urllib.request

MAX_BYTES = 128 * 1024 * 1024


def install(release, expected_sha256, output, max_bytes=MAX_BYTES):
    if not re.fullmatch(r"\d{4}-(0[1-9]|1[0-2])", release):
        raise ValueError("release must have the form YYYY-MM")
    if not re.fullmatch(r"[0-9a-fA-F]{64}", expected_sha256):
        raise ValueError("a trusted archive SHA-256 is required")
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    url = f"https://download.db-ip.com/free/dbip-city-lite-{release}.mmdb.gz"
    temporary = None
    try:
        with tempfile.TemporaryFile() as archive:
            digest = hashlib.sha256()
            size = 0
            with urllib.request.urlopen(url, timeout=30) as response:
                while chunk := response.read(64 * 1024):
                    size += len(chunk)
                    if size > max_bytes:
                        raise ValueError("compressed database exceeds the size limit")
                    digest.update(chunk)
                    archive.write(chunk)
            if digest.hexdigest() != expected_sha256.lower():
                raise ValueError("archive SHA-256 mismatch")
            archive.seek(0)
            with gzip.GzipFile(fileobj=archive) as source:
                with tempfile.NamedTemporaryFile(dir=output.parent, delete=False) as dest:
                    temporary = Path(dest.name)
                    size = 0
                    tail = b""
                    while chunk := source.read(64 * 1024):
                        size += len(chunk)
                        if size > max_bytes:
                            raise ValueError("decompressed database exceeds the size limit")
                        tail = (tail + chunk)[-131072:]
                        dest.write(chunk)
                    if b"\xab\xcd\xefMaxMind.com" not in tail:
                        raise ValueError("missing MMDB metadata marker")
                    dest.flush()
                    os.fsync(dest.fileno())
                    os.fchmod(dest.fileno(), 0o644)
            # The application parses the MMDB metadata when its engine opens.
            os.replace(temporary, output)
            temporary = None
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release", required=True, help="DB-IP release, YYYY-MM")
    parser.add_argument("--sha256", required=True, help="trusted SHA-256 of the .mmdb.gz archive")
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    try:
        install(args.release, args.sha256, args.output)
    except (OSError, ValueError, EOFError) as error:
        parser.exit(1, f"Database installation failed: {error}\n")
    print(f"Installed DB-IP City Lite {args.release} at {args.output}")
    print("Retain the DB-IP attribution and CC BY 4.0 license with your deployment.")


if __name__ == "__main__":
    main()
