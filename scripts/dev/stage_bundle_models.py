#!/usr/bin/env python3
"""Stage the bundled models (live ASR and VAD) for `tauri build` and check every file's SHA-256.

The app looks for them under `$RESOURCE/models/<model dir>/` (app/src-tauri/src/models.rs);
`app/src-tauri/tauri.bundle.conf.json` maps `resources/models/` there. The file list, sizes and
digests come from crates/echonote-models/manifest.json, the same manifest the model manager uses.
Files are copied from a local models directory when they match (default: the dev data
directory, see fetch_assets.py) and downloaded from the manifest mirrors otherwise. Anything
else under the output directory is removed, so the bundle holds exactly the manifest files.

    python3 scripts/dev/stage_bundle_models.py [--source DIR] [--out DIR] [--offline]
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import sys
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Callable

REPO_ROOT = Path(__file__).resolve().parents[2]
MANIFEST = REPO_ROOT / "crates" / "echonote-models" / "manifest.json"
OUT_DIR = REPO_ROOT / "app" / "src-tauri" / "resources" / "models"
BUNDLED_ROLES = ("live", "vad")
CHUNK = 1 << 20
USER_AGENT = "echonote-stage-models/1"

Fetch = Callable[[str, Path], None]


class StageError(Exception):
    pass


def default_source() -> Path:
    configured = os.environ.get("ECHONOTE_DEV_DATA")
    data = Path(configured).expanduser() if configured else Path.home() / ".cache" / "echonote-dev"
    return data / "models"


def bundled(manifest: dict) -> list[dict]:
    models = [model for model in manifest["models"] if model["role"] in BUNDLED_ROLES]
    roles = sorted(model["role"] for model in models)
    if roles != sorted(BUNDLED_ROLES):
        raise StageError(f"manifest needs exactly one model per role {BUNDLED_ROLES}, found {roles}")
    return models


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(CHUNK):
            digest.update(chunk)
    return digest.hexdigest()


def matches(path: Path, entry: dict) -> bool:
    return path.is_file() and path.stat().st_size == entry["size"] and sha256(path) == entry["sha256"]


def download(url: str, path: Path) -> None:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=60) as response, path.open("wb") as handle:
        shutil.copyfileobj(response, handle, CHUNK)


def place(part: Path, target: Path, entry: dict) -> bool:
    if part.stat().st_size == entry["size"] and sha256(part) == entry["sha256"]:
        part.replace(target)
        return True
    part.unlink()
    return False


def stage_file(model: dict, entry: dict, source: Path | None, out: Path, fetch: Fetch | None) -> str:
    relative = Path(model["dir"]) / entry["path"]
    target = out / relative
    if matches(target, entry):
        return "kept"
    target.parent.mkdir(parents=True, exist_ok=True)
    part = target.with_name(target.name + ".part")
    if source is not None and (source / relative).is_file():
        shutil.copyfile(source / relative, part)
        if place(part, target, entry):
            return "copied"
        print(f"warning: {source / relative} does not match the manifest", file=sys.stderr)
    if fetch is None:
        raise StageError(f"{relative}: no matching local copy and downloads are off")
    for mirror in model["mirrors"]:
        url = f"{mirror.rstrip('/')}/{urllib.parse.quote(entry['path'])}"
        if urllib.parse.urlparse(url).scheme != "https":
            print(f"warning: skipping non-https mirror {mirror}", file=sys.stderr)
            continue
        try:
            fetch(url, part)
        except OSError as error:
            part.unlink(missing_ok=True)
            print(f"warning: {url}: {error}", file=sys.stderr)
            continue
        if place(part, target, entry):
            return "downloaded"
        print(f"warning: {url} does not match the manifest", file=sys.stderr)
    raise StageError(f"{relative}: every mirror failed")


def prune(out: Path, keep: set[Path]) -> list[Path]:
    """Remove files under `out` that are not in `keep`, then empty directories."""
    removed = []
    for path in sorted(out.rglob("*"), reverse=True):
        if path.is_dir():
            if not any(path.iterdir()):
                path.rmdir()
        elif path not in keep:
            path.unlink()
            removed.append(path)
    return removed


def stage(manifest: dict, source: Path | None, out: Path, fetch: Fetch | None) -> dict[str, int]:
    out.mkdir(parents=True, exist_ok=True)
    counts: dict[str, int] = {}
    keep = set()
    for model in bundled(manifest):
        for entry in model["files"]:
            outcome = stage_file(model, entry, source, out, fetch)
            counts[outcome] = counts.get(outcome, 0) + 1
            keep.add(out / model["dir"] / entry["path"])
            print(f"{outcome:>10}  {model['dir']}/{entry['path']}")
    for path in prune(out, keep):
        print(f"{'removed':>10}  {path.relative_to(out)}")
    return counts


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--source", type=Path, default=default_source(), help="local models directory to copy from")
    parser.add_argument("--out", type=Path, default=OUT_DIR, help="staging directory (default: %(default)s)")
    parser.add_argument("--offline", action="store_true", help="fail instead of downloading")
    args = parser.parse_args()
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    source = args.source if args.source.is_dir() else None
    try:
        counts = stage(manifest, source, args.out, None if args.offline else download)
    except StageError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    size = sum(path.stat().st_size for path in args.out.rglob("*") if path.is_file())
    summary = ", ".join(f"{count} {outcome}" for outcome, count in sorted(counts.items()))
    print(f"staged {size / 1e6:.1f} MB in {args.out} ({summary})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
