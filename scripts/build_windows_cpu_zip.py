#!/usr/bin/env python3
"""Build a self-contained EchoNote Windows CPU ASR ZIP bundle."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import tempfile
import zipfile
from pathlib import Path

MODEL_REPOSITORY = "Systran/faster-whisper-small"
MODEL_REVISION = "536b0662742c02347bc0e980a01041f333bce120"
MODEL_LICENSE = "MIT"
MODEL_DIRECTORY_NAME = "faster-whisper-small"
MANIFEST_NAME = "bundle-manifest.json"
CHECKSUMS_NAME = "SHA256SUMS.txt"
TARGET_PYTHON = "3.11.9"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--wheelhouse", type=Path, required=True)
    parser.add_argument("--requirements", type=Path, required=True)
    parser.add_argument("--service-wheel", type=Path, required=True)
    parser.add_argument("--bundle-version", default="0.9.0")
    parser.add_argument("--force", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    output = args.output.expanduser().resolve()
    if output.suffix.lower() != ".zip":
        raise ValueError("Output path must end in .zip")
    if output.exists() and not args.force:
        raise ValueError(f"Output already exists: {output}; use --force to replace it")

    build_windows_cpu_zip(
        output=output,
        model=require_model(args.model),
        wheelhouse=require_directory(args.wheelhouse, "wheelhouse"),
        requirements=require_file(args.requirements, "requirements"),
        service_wheel=require_file(args.service_wheel, "service wheel"),
        bundle_version=args.bundle_version,
    )
    print(f"Created Windows CPU bundle: {output}")
    print(f"SHA-256: {sha256(output)}")
    print(f"Size: {output.stat().st_size} bytes")
    return 0


def build_windows_cpu_zip(
    *,
    output: Path,
    model: Path,
    wheelhouse: Path,
    requirements: Path,
    service_wheel: Path,
    bundle_version: str,
) -> None:
    repository_root = Path(__file__).resolve().parents[1]
    service_root = repository_root / "asr-service"
    archive_root = f"EchoNote-ASR-Windows-CPU-{bundle_version}-py311"
    output.parent.mkdir(parents=True, exist_ok=True)

    staging_parent = Path(tempfile.mkdtemp(prefix=".echonote-windows-cpu-", dir=output.parent))
    staging = staging_parent / archive_root
    staging.mkdir()
    temporary_output = output.with_name(f".{output.name}.tmp")
    try:
        copy_file(service_root / "install-windows-cpu.ps1", staging / "install-windows-cpu.ps1")
        copy_file(service_root / "run-windows-cpu.ps1", staging / "run-windows-cpu.ps1")
        copy_file(service_root / "verify-windows-cpu.ps1", staging / "verify-windows-cpu.ps1")
        copy_file(service_root / "README.md", staging / "README.md")
        copy_file(repository_root / "License", staging / "LICENSE-ECHONOTE.txt")
        copy_file(requirements, staging / "requirements-windows-cpu.txt")

        bundled_wheelhouse = staging / "wheelhouse"
        copy_tree(wheelhouse, bundled_wheelhouse)
        copy_file(service_wheel, bundled_wheelhouse / service_wheel.name)
        if not any(bundled_wheelhouse.glob("*.whl")):
            raise ValueError("Wheelhouse does not contain any .whl files")

        copy_tree(model, staging / "models" / MODEL_DIRECTORY_NAME, excluded_names={".cache"})

        file_entries = build_file_manifest(staging)
        manifest = {
            "schemaVersion": 1,
            "bundleType": "echonote-windows-cpu-asr",
            "bundleVersion": bundle_version,
            "target": {
                "operatingSystem": "Windows Server 2016 x64",
                "pythonVersion": TARGET_PYTHON,
                "host": "127.0.0.1",
            },
            "backend": {
                "name": "faster-whisper",
                "device": "cpu",
                "computeType": "int8",
                "workers": 1,
            },
            "model": {
                "repository": MODEL_REPOSITORY,
                "revision": MODEL_REVISION,
                "license": MODEL_LICENSE,
                "path": f"models/{MODEL_DIRECTORY_NAME}",
            },
            "files": file_entries,
        }
        manifest_path = staging / MANIFEST_NAME
        manifest_path.write_text(
            json.dumps(manifest, ensure_ascii=True, indent=2) + "\n",
            encoding="utf-8",
        )
        write_checksums(staging, staging / CHECKSUMS_NAME)

        if temporary_output.exists():
            temporary_output.unlink()
        write_zip(staging, temporary_output, archive_root)
        os.replace(temporary_output, output)
    finally:
        temporary_output.unlink(missing_ok=True)
        shutil.rmtree(staging_parent, ignore_errors=True)


def require_model(path: Path) -> Path:
    model = require_directory(path, "model")
    for required_name in ("config.json", "model.bin", "tokenizer.json"):
        if not (model / required_name).is_file():
            raise ValueError(f"Model is missing {required_name}: {model}")
    return model


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


def copy_tree(source: Path, destination: Path, *, excluded_names: set[str] | None = None) -> None:
    excluded_names = excluded_names or set()
    destination.mkdir(parents=True, exist_ok=True)
    for entry in sorted(source.iterdir(), key=lambda item: item.name):
        if entry.name in excluded_names:
            continue
        if entry.is_symlink():
            raise ValueError(f"Bundle inputs must not contain symlinks: {entry}")
        destination_entry = destination / entry.name
        if entry.is_dir():
            copy_tree(entry, destination_entry, excluded_names=excluded_names)
        elif entry.is_file():
            copy_file(entry, destination_entry)
        else:
            raise ValueError(f"Unsupported special file in bundle input: {entry}")


def copy_file(source: Path, destination: Path) -> None:
    if source.is_symlink():
        raise ValueError(f"Bundle inputs must not contain symlinks: {source}")
    if not source.is_file():
        raise ValueError(f"Bundle input file not found: {source}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)


def build_file_manifest(root: Path) -> list[dict[str, object]]:
    entries: list[dict[str, object]] = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"Staged bundle unexpectedly contains a symlink: {path}")
        if path.is_file() and path.name not in {MANIFEST_NAME, CHECKSUMS_NAME}:
            entries.append(
                {
                    "path": path.relative_to(root).as_posix(),
                    "size": path.stat().st_size,
                    "sha256": sha256(path),
                }
            )
    return entries


def write_checksums(root: Path, output: Path) -> None:
    lines = []
    for path in sorted(root.rglob("*")):
        if path.is_file() and path != output:
            lines.append(f"{sha256(path)}  {path.relative_to(root).as_posix()}")
    output.write_text("\n".join(lines) + "\n", encoding="ascii")


def write_zip(source_root: Path, output: Path, archive_root: str) -> None:
    with zipfile.ZipFile(output, "w", allowZip64=True) as archive:
        for source in sorted(path for path in source_root.rglob("*") if path.is_file()):
            relative_path = source.relative_to(source_root).as_posix()
            info = zipfile.ZipInfo(f"{archive_root}/{relative_path}", date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = (
                zipfile.ZIP_STORED
                if source.suffix.lower() in {".bin", ".whl", ".zip"}
                else zipfile.ZIP_DEFLATED
            )
            info.external_attr = 0o644 << 16
            with source.open("rb") as source_handle, archive.open(info, "w") as target_handle:
                shutil.copyfileobj(source_handle, target_handle, length=1024 * 1024)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, zipfile.BadZipFile) as error:
        print(f"error: {error}", file=os.sys.stderr)
        raise SystemExit(2) from error
