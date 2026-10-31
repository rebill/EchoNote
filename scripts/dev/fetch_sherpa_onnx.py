#!/usr/bin/env python3
"""Download the pinned sherpa-onnx static library archive for a target and check its SHA-256.

The sherpa-onnx-sys build script downloads the archive without checking it; CI and release
builds fetch it here first and point SHERPA_ONNX_ARCHIVE_DIR at the folder instead.

    python3 scripts/dev/fetch_sherpa_onnx.py <archive-dir> [rust-target]
"""

from __future__ import annotations

import hashlib
import platform
import sys
import urllib.request
from pathlib import Path

VERSION = "2.0.1"
BASE_URL = f"https://github.com/k2-fsa/sherpa-onnx/releases/download/v{VERSION}"
# Digests from the GitHub release asset metadata.
ARCHIVES = {
    "aarch64-apple-darwin": (
        f"sherpa-onnx-v{VERSION}-osx-arm64-static-lib.tar.bz2",
        "d7a9b14f337958e7373c8c650f046df33d210bbaff2f33cb8322dda663ee5e1c",
    ),
    "x86_64-apple-darwin": (
        f"sherpa-onnx-v{VERSION}-osx-x64-static-lib.tar.bz2",
        "6a3cb3c14104d3a02e7b2a6cd5621f590b14892cce7d0bc0935f7655ef518938",
    ),
    "x86_64-pc-windows-msvc": (
        f"sherpa-onnx-v{VERSION}-win-x64-static-MT-Release-lib.tar.bz2",
        "23e7b986b197eda0bbc47ff3d7d5c87afd6ef2b9077214ba26b0f1ec52ef0e66",
    ),
}


def host_target() -> str:
    machine = platform.machine().lower()
    if sys.platform == "darwin":
        return "aarch64-apple-darwin" if machine == "arm64" else "x86_64-apple-darwin"
    if sys.platform == "win32":
        return "x86_64-pc-windows-msvc"
    raise SystemExit(f"no pinned archive for {sys.platform}/{machine}")


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> None:
    if len(sys.argv) not in (2, 3):
        raise SystemExit(__doc__)
    folder = Path(sys.argv[1])
    target = sys.argv[2] if len(sys.argv) == 3 else host_target()
    if target not in ARCHIVES:
        raise SystemExit(f"no pinned archive for {target}")
    name, expected = ARCHIVES[target]
    destination = folder / name
    if destination.is_file() and sha256_of(destination) == expected:
        print(f"{name}: cached")
        return
    folder.mkdir(parents=True, exist_ok=True)
    partial = destination.with_suffix(".part")
    print(f"downloading {BASE_URL}/{name}")
    with urllib.request.urlopen(f"{BASE_URL}/{name}", timeout=60) as response, partial.open("wb") as out:
        while chunk := response.read(1 << 20):
            out.write(chunk)
    actual = sha256_of(partial)
    if actual != expected:
        partial.unlink()
        raise SystemExit(f"{name}: SHA-256 mismatch, expected {expected}, got {actual}")
    partial.replace(destination)
    print(f"{name}: verified")


if __name__ == "__main__":
    main()
