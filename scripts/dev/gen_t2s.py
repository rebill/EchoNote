"""Regenerate crates/echonote-core/data/t2s.txt from OpenCC's TSCharacters.txt.

Fetches the file at a fixed OpenCC tag through the GitHub contents API, checks its git blob
SHA-1, and writes "TradSimp" pairs (the first Simplified candidate of each line), skipping
characters that map to themselves. Only the Python standard library is used.

Usage: python3 scripts/dev/gen_t2s.py
"""

from __future__ import annotations

import hashlib
import urllib.request
from pathlib import Path

TAG = "ver.1.1.9"
BLOB = "a236514576eed080868c64b4899c46315063ce02"
URL = f"https://api.github.com/repos/BYVoid/OpenCC/contents/data/dictionary/TSCharacters.txt?ref={TAG}"
OUT = Path(__file__).resolve().parents[2] / "crates" / "echonote-core" / "data" / "t2s.txt"


def main() -> None:
    request = urllib.request.Request(
        URL, headers={"Accept": "application/vnd.github.raw", "User-Agent": "echonote-dev-t2s/1"}
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        data = response.read()
    blob = hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()
    if blob != BLOB:
        raise SystemExit(f"TSCharacters.txt blob {blob} is not the expected {BLOB}")
    pairs = []
    for line in data.decode("utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        traditional, candidates = line.split("\t")
        simplified = candidates.split(" ")[0]
        assert len(traditional) == 1 and len(simplified) == 1, line
        if traditional != simplified:
            pairs.append(traditional + simplified)
    with OUT.open("w", encoding="utf-8") as out:
        out.write("# Traditional to Simplified, one character each: the first candidate of OpenCC 1.1.9\n")
        out.write(f"# data/dictionary/TSCharacters.txt (git blob {BLOB}).\n")
        out.write("# Copyright 2010-2020 Carbo Kuo and OpenCC contributors, Apache License 2.0.\n")
        out.write("# Regenerate with scripts/dev/gen_t2s.py.\n")
        for start in range(0, len(pairs), 32):
            out.write(" ".join(pairs[start : start + 32]) + "\n")
    print(f"{len(pairs)} pairs written to {OUT}")


if __name__ == "__main__":
    main()
