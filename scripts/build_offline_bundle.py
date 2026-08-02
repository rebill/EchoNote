#!/usr/bin/env python3
"""Build a portable, integrity-checked EchoNote offline installation bundle."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import stat
import sys
import tempfile
from pathlib import Path

MANIFEST_NAME = "echonote-offline-manifest.json"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--wheelhouse", type=Path, required=True)
    parser.add_argument("--requirements", type=Path, required=True)
    parser.add_argument(
        "--asr-model",
        action="append",
        required=True,
        metavar="PRESET=PATH",
        help="ASR preset and local model directory; may be repeated.",
    )
    parser.add_argument("--diarization-model", type=Path, required=True)
    parser.add_argument("--bundle-version", default="0.9.0")
    parser.add_argument(
        "--python-version",
        default=f"{sys.version_info.major}.{sys.version_info.minor}",
        help="Python major.minor used to build the wheelhouse.",
    )
    parser.add_argument("--force", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    output = args.output.expanduser().resolve()
    wheelhouse = require_directory(args.wheelhouse, "wheelhouse")
    requirements = require_file(args.requirements, "requirements")
    diarization = require_directory(args.diarization_model, "diarization model")
    asr_models = parse_asr_models(args.asr_model)
    validate_requirements(requirements)
    if not any(path.suffix == ".whl" for path in wheelhouse.iterdir() if path.is_file()):
        raise ValueError("Wheelhouse must contain at least one .whl file.")
    if output.exists() and not args.force:
        raise ValueError(f"Output already exists: {output}. Use --force to replace it.")

    output.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=".echonote-offline-", dir=output.parent))
    try:
        materialize_tree(wheelhouse, staging / "wheels")
        shutil.copy2(requirements, staging / "requirements-offline.txt")

        models = []
        for preset, source in asr_models:
            destination = staging / "models" / safe_component(preset)
            materialize_tree(source, destination)
            models.append(
                {"role": "asr", "preset": preset, "path": relative(staging, destination)}
            )

        diarization_destination = staging / "models" / "speaker-diarization-community-1"
        materialize_tree(diarization, diarization_destination)
        if not (diarization_destination / "config.yaml").is_file():
            raise ValueError("Diarization model must contain config.yaml.")
        models.append(
            {
                "role": "diarization",
                "preset": None,
                "path": relative(staging, diarization_destination),
            }
        )

        manifest = {
            "schemaVersion": 1,
            "bundleVersion": args.bundle_version,
            "platform": current_platform(),
            "pythonVersion": args.python_version,
            "wheelsPath": "wheels",
            "requirementsPath": "requirements-offline.txt",
            "models": models,
            "files": build_file_manifest(staging),
        }
        (staging / MANIFEST_NAME).write_text(
            json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )

        if output.exists():
            shutil.rmtree(output)
        os.replace(staging, output)
        print(f"Created offline bundle: {output}")
        print(f"Platform: {manifest['platform']}; Python: {manifest['pythonVersion']}")
        print(f"Verified files recorded: {len(manifest['files'])}")
        return 0
    except Exception:
        shutil.rmtree(staging, ignore_errors=True)
        raise


def parse_asr_models(values: list[str]) -> list[tuple[str, Path]]:
    models: list[tuple[str, Path]] = []
    presets: set[str] = set()
    for value in values:
        preset, separator, raw_path = value.partition("=")
        if not separator or not preset.strip() or not raw_path.strip():
            raise ValueError(f"Invalid --asr-model value: {value!r}; expected PRESET=PATH.")
        preset = preset.strip()
        if preset in presets:
            raise ValueError(f"Duplicate ASR preset: {preset}")
        presets.add(preset)
        models.append((preset, require_directory(Path(raw_path), f"ASR model {preset}")))
    return models


def require_directory(path: Path, label: str) -> Path:
    resolved = path.expanduser().resolve()
    if not resolved.is_dir():
        raise ValueError(f"{label.capitalize()} directory not found: {resolved}")
    return resolved


def require_file(path: Path, label: str) -> Path:
    resolved = path.expanduser().resolve()
    if not resolved.is_file():
        raise ValueError(f"{label.capitalize()} file not found: {resolved}")
    return resolved


def validate_requirements(path: Path) -> None:
    forbidden = ("http://", "https://", "git+", "--index-url", "--extra-index-url", "-e ")
    for line_number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip().lower()
        if line and not line.startswith("#") and any(value in line for value in forbidden):
            raise ValueError(
                f"Requirements line {line_number} can access a remote or editable source: {raw}"
            )


def materialize_tree(source: Path, destination: Path) -> None:
    destination.mkdir(parents=True, exist_ok=True)
    copy_directory(source.resolve(), destination, set())


def copy_directory(source: Path, destination: Path, active: set[Path]) -> None:
    resolved = source.resolve()
    if resolved in active:
        raise ValueError(f"Symlink cycle detected at {source}")
    active.add(resolved)
    try:
        for entry in sorted(source.iterdir(), key=lambda item: item.name):
            target = entry.resolve() if entry.is_symlink() else entry
            mode = target.stat().st_mode
            destination_entry = destination / entry.name
            if stat.S_ISDIR(mode):
                destination_entry.mkdir(exist_ok=False)
                copy_directory(target, destination_entry, active)
            elif stat.S_ISREG(mode):
                shutil.copy2(target, destination_entry, follow_symlinks=True)
            else:
                raise ValueError(f"Unsupported special file in bundle source: {entry}")
    finally:
        active.remove(resolved)


def safe_component(name: str) -> str:
    name = name.strip()
    if not name or name in {".", ".."} or "/" in name or "\\" in name:
        raise ValueError(f"Unsafe model directory name: {name!r}")
    return name


def current_platform() -> str:
    system = platform.system().lower()
    os_name = "macos" if system == "darwin" else system
    machine = platform.machine().lower()
    arch = {"arm64": "aarch64", "amd64": "x86_64"}.get(machine, machine)
    return f"{os_name}-{arch}"


def build_file_manifest(root: Path) -> list[dict[str, object]]:
    entries = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"Materialized bundle unexpectedly contains a symlink: {path}")
        if path.is_file() and path.name != MANIFEST_NAME:
            entries.append(
                {
                    "path": relative(root, path),
                    "size": path.stat().st_size,
                    "sha256": sha256(path),
                }
            )
    return entries


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def relative(root: Path, path: Path) -> str:
    return path.relative_to(root).as_posix()


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2) from error
