"""Fetch EchoNote development assets (ASR models, VAD, evaluation audio) with SHA-256 verification.

Assets live outside the repository, in ECHONOTE_DEV_DATA (default: ~/.cache/echonote-dev).
The manifest is crates/echonote-bench/assets.json. Only the Python standard library is used.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
MANIFEST_PATH = REPO_ROOT / "crates" / "echonote-bench" / "assets.json"
CHUNK_BYTES = 1 << 20
USER_AGENT = "echonote-dev-fetch/1"


def default_data_dir() -> Path:
    configured = os.environ.get("ECHONOTE_DEV_DATA")
    return Path(configured).expanduser() if configured else Path.home() / ".cache" / "echonote-dev"


def build_opener(url: str) -> urllib.request.OpenerDirector:
    host = urllib.parse.urlparse(url).hostname or ""
    direct_hosts = [item.strip() for item in os.environ.get("ECHONOTE_DIRECT_HOSTS", "modelscope.cn,aliyuncs.com").split(",") if item.strip()]
    if any(host == suffix or host.endswith("." + suffix) for suffix in direct_hosts):
        return urllib.request.build_opener(urllib.request.ProxyHandler({}))
    return urllib.request.build_opener()


def resolve_url(source: dict[str, str]) -> str:
    kind = source["kind"]
    if kind == "url":
        return source["url"]
    if kind == "modelscope-model":
        return f"https://www.modelscope.cn/models/{source['repo']}/resolve/master/{source['path']}"
    if kind == "modelscope-dataset-file":
        query = urllib.parse.urlencode({"Revision": "master", "FilePath": source["path"]})
        return f"https://www.modelscope.cn/api/v1/datasets/{source['repo']}/repo?{query}"
    if kind == "modelscope-dataset-oss":
        listing_url = (
            f"https://www.modelscope.cn/api/v1/datasets/{source['repo']}/oss/tree/"
            "?MaxLimit=-1&Revision=master&Recursive=True&FilterDir=True"
        )
        request = urllib.request.Request(listing_url, headers={"User-Agent": USER_AGENT})
        with build_opener(listing_url).open(request, timeout=60) as response:
            listing = json.loads(response.read())
        for entry in listing.get("Data") or []:
            if entry.get("Key") == source["key"]:
                return entry["Url"]
        raise RuntimeError(f"OSS object {source['key']} not found in dataset {source['repo']}")
    raise ValueError(f"unsupported source kind: {kind}")


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(CHUNK_BYTES):
            digest.update(chunk)
    return digest.hexdigest()


def verified_marker(path: Path) -> Path:
    return path.with_name(path.name + ".verified")


def is_verified(path: Path, size: int, sha256: str | None) -> bool:
    marker = verified_marker(path)
    if not path.is_file() or path.stat().st_size != size or not marker.is_file():
        return False
    return sha256 is None or marker.read_text(encoding="utf-8").strip() == sha256


def download(url: str, destination: Path, expected_size: int, attempts: int = 6) -> None:
    for attempt in range(1, attempts + 1):
        try:
            download_once(url, destination, expected_size)
            return
        except (OSError, RuntimeError) as exc:
            if attempt == attempts:
                raise
            print(f"    {destination.name}: attempt {attempt} failed ({exc}); resuming", flush=True)
            time.sleep(min(30, 2**attempt))


def download_once(url: str, destination: Path, expected_size: int) -> None:
    partial = destination.with_name(destination.name + ".part")
    offset = partial.stat().st_size if partial.exists() else 0
    if offset > expected_size:
        partial.unlink()
        offset = 0
    if offset == expected_size:
        partial.replace(destination)
        return
    headers = {"User-Agent": USER_AGENT}
    if offset:
        headers["Range"] = f"bytes={offset}-"
    request = urllib.request.Request(url, headers=headers)
    with build_opener(url).open(request, timeout=120) as response:
        if offset and response.status != 206:
            offset = 0
        with partial.open("ab" if offset else "wb") as handle:
            received = offset
            next_report = received + 64 * CHUNK_BYTES
            while chunk := response.read(CHUNK_BYTES):
                handle.write(chunk)
                received += len(chunk)
                if received >= next_report:
                    print(f"    {destination.name}: {received / 1e6:.0f}/{expected_size / 1e6:.0f} MB", flush=True)
                    next_report = received + 64 * CHUNK_BYTES
    actual_size = partial.stat().st_size
    if actual_size != expected_size:
        raise RuntimeError(f"{destination.name}: expected {expected_size} bytes, received {actual_size}")
    partial.replace(destination)


def fetch_file(asset_dir: Path, entry: dict[str, object]) -> None:
    destination = asset_dir / str(entry["path"])
    size = int(entry["size"])
    sha256 = entry.get("sha256")
    if is_verified(destination, size, sha256 if isinstance(sha256, str) else None):
        return
    destination.parent.mkdir(parents=True, exist_ok=True)
    if not (destination.is_file() and destination.stat().st_size == size):
        url = resolve_url(entry["source"])  # type: ignore[arg-type]
        print(f"  downloading {entry['path']} ({size / 1e6:.1f} MB)", flush=True)
        download(url, destination, size)
    actual = sha256_of(destination)
    if isinstance(sha256, str) and actual != sha256:
        destination.unlink()
        raise RuntimeError(f"{entry['path']}: SHA-256 mismatch, expected {sha256}, got {actual}")
    if not isinstance(sha256, str):
        print(f"  WARNING {entry['path']} has no pinned SHA-256; computed {actual}")
    verified_marker(destination).write_text(actual + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("assets", nargs="*", help="asset ids to fetch; default is every asset not marked optional")
    parser.add_argument("--data-dir", type=Path, default=default_data_dir())
    parser.add_argument("--list", action="store_true", help="list asset ids and exit")
    args = parser.parse_args()

    manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    assets = manifest["assets"]
    if args.list:
        for asset_id, asset in assets.items():
            total = sum(int(item["size"]) for item in asset["files"])
            flag = " (optional)" if asset.get("optional") else ""
            print(f"{asset_id:28} {total / 1e6:8.1f} MB  {asset['description']}{flag}")
        return 0

    selected = args.assets or [asset_id for asset_id, asset in assets.items() if not asset.get("optional")]
    unknown = [asset_id for asset_id in selected if asset_id not in assets]
    if unknown:
        parser.error(f"unknown asset ids: {', '.join(unknown)}")

    failures: list[str] = []
    for asset_id in selected:
        asset = assets[asset_id]
        asset_dir = args.data_dir / asset["dir"]
        print(f"[{asset_id}] -> {asset_dir}", flush=True)
        for entry in asset["files"]:
            try:
                fetch_file(asset_dir, entry)
            except (OSError, RuntimeError) as exc:
                print(f"  FAILED {entry['path']}: {exc}", flush=True)
                failures.append(f"{asset_id}/{entry['path']}")
    if failures:
        print("failed: " + ", ".join(failures))
        return 1
    print("done")
    return 0


if __name__ == "__main__":
    sys.exit(main())
