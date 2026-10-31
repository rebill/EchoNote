"""Check a model's files in assets.json against an upstream release tarball.

Streams the tarball (nothing is written to disk), checks its SHA-256 against the digest the
release publishes, then hashes every member and compares it with the manifest entry of the
same path. Exit status 0 only when the tarball digest matches and every manifest file is
present in the tarball with the same size and hash.

Usage: verify_release_digest.py <asset-id> <tarball-url> <sha256>
Example: verify_release_digest.py qwen3-asr-int8 \\
  https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2 \\
  393f8a14e2f5fb96746aaab342997a40641001fbd5bf9592a080a8329178ee96
"""

from __future__ import annotations

import hashlib
import json
import sys
import tarfile
import urllib.request
from pathlib import Path

MANIFEST = Path(__file__).resolve().parents[2] / "crates" / "echonote-bench" / "assets.json"


class Hashing:
    """A file-like reader that hashes what passes through it."""

    def __init__(self, raw):
        self.raw = raw
        self.digest = hashlib.sha256()
        self.size = 0

    def read(self, n=-1):
        chunk = self.raw.read(n)
        self.digest.update(chunk)
        self.size += len(chunk)
        return chunk


def main() -> int:
    asset_id, url, expected = sys.argv[1], sys.argv[2], sys.argv[3].lower().removeprefix("sha256:")
    asset = json.loads(MANIFEST.read_text(encoding="utf-8"))["assets"][asset_id]
    wanted = {entry["path"]: entry for entry in asset["files"]}
    request = urllib.request.Request(url, headers={"User-Agent": "echonote-dev-verify/1"})
    found: dict[str, tuple[int, str]] = {}
    with urllib.request.urlopen(request, timeout=120) as response:
        stream = Hashing(response)
        with tarfile.open(fileobj=stream, mode="r|*") as archive:
            for member in archive:
                if not member.isfile():
                    continue
                # Members are "<dir>/<path>"; the manifest paths are relative to the dir.
                path = member.name.split("/", 1)[1] if "/" in member.name else member.name
                handle = archive.extractfile(member)
                digest = hashlib.sha256()
                while chunk := handle.read(1 << 20):
                    digest.update(chunk)
                found[path] = (member.size, digest.hexdigest())
        # Drain what tarfile did not read (padding at the end), so the digest covers it all.
        while stream.read(1 << 20):
            pass
    ok = True
    tarball = stream.digest.hexdigest()
    print(f"tarball {stream.size} bytes sha256 {tarball} {'OK' if tarball == expected else 'MISMATCH'}")
    ok &= tarball == expected
    for path, entry in wanted.items():
        size, sha = found.get(path, (None, None))
        match = size == entry["size"] and sha == entry["sha256"]
        ok &= match
        print(f"{'OK      ' if match else 'MISMATCH'} {path} manifest={entry['size']}/{entry['sha256'][:12]} upstream={size}/{(sha or '')[:12]}")
    extra = sorted(set(found) - set(wanted))
    if extra:
        print("in the tarball, not in the manifest:", ", ".join(extra))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
