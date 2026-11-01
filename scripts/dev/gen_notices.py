#!/usr/bin/env python3
"""Write THIRD_PARTY_NOTICES.txt for the installers.

Inputs: third_party/notices.json (models, native libraries, data files, frontend runtime) and the
Rust crates echonote-app links on macOS and Windows, read with `cargo metadata` (normal and build
dependencies; dev-dependencies are excluded). Each distinct license text is printed once,
followed by the components it applies to. `tauri.bundle.conf.json` puts the file next to the
bundled models.

    python3 scripts/dev/gen_notices.py [--out PATH]
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
THIRD_PARTY = REPO_ROOT / "third_party"
NODE_MODULES = REPO_ROOT / "app" / "node_modules"
OUT = REPO_ROOT / "app" / "src-tauri" / "resources" / "THIRD_PARTY_NOTICES.txt"
TARGETS = ("aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc")
ROOT_PACKAGE = "echonote-app"
LICENSE_NAME = re.compile(r"^(licen[cs]e|copying|notice|unlicense|copyright)", re.IGNORECASE)


class NoticeError(Exception):
    pass


@dataclass
class Entry:
    group: str
    name: str
    version: str
    license: str
    source: str
    attribution: str = ""
    texts: list[str] = field(default_factory=list)

    @property
    def title(self) -> str:
        return f"{self.name} {self.version}".strip()


def read(path: Path) -> str:
    if not path.is_file():
        raise NoticeError(f"missing license file {path}")
    return path.read_text(encoding="utf-8", errors="replace").strip()


def component(spec: dict, third_party: Path, node_modules: Path) -> Entry:
    """One notices.json entry. Frontend files are read from node_modules at the built version."""
    version = spec.get("version", "")
    base = third_party
    if "npm" in spec:
        base = node_modules
        package = node_modules / spec["npm"] / "package.json"
        if not package.is_file():
            raise NoticeError(f"{package} not found; run pnpm install in app/ first")
        version = json.loads(package.read_text(encoding="utf-8"))["version"]
    texts = [read(base / name) for name in spec["files"]]
    if "until" in spec:
        texts = [text.split(spec["until"])[0].strip() for text in texts]
    return Entry(spec["group"], spec["name"], version, spec["license"], spec["source"], spec.get("attribution", ""), texts)


def shipped_packages(metadata: dict, root: str) -> set[str]:
    """Package ids reachable from `root` through normal and build dependencies."""
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    root_id = next(package["id"] for package in metadata["packages"] if package["name"] == root)
    seen: set[str] = set()
    stack = [root_id]
    while stack:
        package = stack.pop()
        if package in seen:
            continue
        seen.add(package)
        for dependency in nodes[package]["deps"]:
            if any(kind["kind"] in (None, "build") for kind in dependency["dep_kinds"]):
                stack.append(dependency["pkg"])
    return seen


def license_files(directory: Path, declared: str | None) -> list[Path]:
    files = []
    for path in sorted(directory.iterdir()):
        if not LICENSE_NAME.match(path.name):
            continue
        if path.is_file():
            files.append(path)
        elif path.is_dir():
            files.extend(sorted(child for child in path.iterdir() if child.is_file()))
    if declared and (directory / declared).is_file() and directory / declared not in files:
        files.append(directory / declared)
    return files


def overrides(specs: list[dict], third_party: Path) -> dict[str, list[str]]:
    """License texts for crates that publish none, by crate name (notices.json `crate_licenses`)."""
    texts = {}
    for spec in specs:
        files = [read(third_party / name) for name in spec["files"]]
        texts.update((crate, files) for crate in spec["crates"])
    return texts


def crate_entries(metadata: dict, ids: set[str], fallback: dict[str, list[str]]) -> list[Entry]:
    entries = []
    for package in metadata["packages"]:
        # Workspace crates are EchoNote itself; vendored C code is listed in notices.json.
        if package["id"] not in ids or package["source"] is None:
            continue
        directory = Path(package["manifest_path"]).parent
        texts = [read(path) for path in license_files(directory, package.get("license_file"))]
        if not texts:
            if package["name"] not in fallback:
                raise NoticeError(f"{package['name']} {package['version']} ships no license file; add it to crate_licenses")
            texts = fallback[package["name"]]
        license = package.get("license") or "see the license file"
        source = package.get("repository") or f"https://crates.io/crates/{package['name']}"
        entries.append(Entry("Rust crates", package["name"], package["version"], license, source, texts=texts))
    return sorted(entries, key=lambda entry: (entry.name, entry.version))


def cargo_entries(fallback: dict[str, list[str]], targets: tuple[str, ...] = TARGETS) -> list[Entry]:
    packages: dict[str, dict] = {}
    ids: set[str] = set()
    for target in targets:
        output = subprocess.check_output(
            ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", target],
            cwd=REPO_ROOT,
        )
        metadata = json.loads(output)
        ids |= shipped_packages(metadata, ROOT_PACKAGE)
        packages.update((package["id"], package) for package in metadata["packages"])
    return crate_entries({"packages": list(packages.values())}, ids, fallback)


def digest(text: str) -> str:
    return hashlib.sha256(" ".join(text.split()).encode()).hexdigest()


def render(entries: list[Entry]) -> str:
    """A component table, then each distinct license text once with the components it covers."""
    lines = [
        "EchoNote third-party notices",
        "",
        "EchoNote includes the components below. Each is distributed under its own license;",
        "the texts follow the table. Generated by scripts/dev/gen_notices.py.",
        "",
    ]
    group = None
    for entry in entries:
        if entry.group != group:
            group = entry.group
            lines += ["", f"== {group} ==", ""]
        lines.append(f"- {entry.title} ({entry.license}) {entry.source}")
        if entry.attribution:
            lines.append(f"  {entry.attribution}")
    texts: dict[str, tuple[str, list[str]]] = {}
    for entry in entries:
        for text in entry.texts:
            key = digest(text)
            if key not in texts:
                texts[key] = (text, [])
            if entry.title not in texts[key][1]:
                texts[key][1].append(entry.title)
    lines += ["", "", "== License texts ==", ""]
    for text, users in texts.values():
        lines += ["-" * 72, "Applies to: " + ", ".join(users), "-" * 72, "", text, ""]
    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=OUT)
    args = parser.parse_args()
    notices = json.loads((THIRD_PARTY / "notices.json").read_text(encoding="utf-8"))
    try:
        entries = [component(spec, THIRD_PARTY, NODE_MODULES) for spec in notices["components"]]
        entries += cargo_entries(overrides(notices["crate_licenses"], THIRD_PARTY))
    except (NoticeError, subprocess.CalledProcessError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    text = render(entries)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(text, encoding="utf-8")
    crates = sum(entry.group == "Rust crates" for entry in entries)
    print(f"wrote {args.out} ({len(entries)} components, {crates} crates, {len(text) / 1e3:.0f} kB)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
