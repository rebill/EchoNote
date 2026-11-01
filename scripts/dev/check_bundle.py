#!/usr/bin/env python3
"""Check a `tauri build` output: bundled models, notices, and installer size.

The models must sit where the app looks (`<resources>/models/<dir>/<file>`, sizes from
crates/echonote-models/manifest.json) and every installer must fit `app.installerMiB` in
docs/v1-performance-budgets.json. Prints a JSON report; exit status 1 on any failure.

    python3 scripts/dev/check_bundle.py <cargo release dir> <rust target>
    e.g. python3 scripts/dev/check_bundle.py target/x86_64-pc-windows-msvc/release x86_64-pc-windows-msvc

    python3 scripts/dev/check_bundle.py --installed <install dir> <rust target>
    checks the files an installer put on disk (Windows: next to the executable).
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
MANIFEST = REPO_ROOT / "crates" / "echonote-models" / "manifest.json"
BUDGETS = REPO_ROOT / "docs" / "v1-performance-budgets.json"
BUNDLED_ROLES = ("live", "vad")
MIB = 1 << 20


def layout(release: Path, target: str) -> tuple[Path | None, list[Path]]:
    """The resource directory of the built app and the installers, for a target."""
    bundle = release / "bundle"
    if "apple-darwin" in target:
        apps = sorted((bundle / "macos").glob("*.app"))
        return (apps[0] / "Contents" / "Resources" if apps else None), sorted((bundle / "dmg").glob("*.dmg"))
    # NSIS installs the resources next to the executable; `tauri build` copies them there first.
    return release, sorted((bundle / "nsis").glob("*-setup.exe"))


def resource_problems(resources: Path, manifest: dict) -> list[str]:
    problems = []
    for model in manifest["models"]:
        if model["role"] not in BUNDLED_ROLES:
            continue
        for entry in model["files"]:
            path = resources / "models" / model["dir"] / entry["path"]
            if not path.is_file():
                problems.append(f"missing {path}")
            elif path.stat().st_size != entry["size"]:
                problems.append(f"{path} is {path.stat().st_size} bytes, manifest says {entry['size']}")
    for name in ("THIRD_PARTY_NOTICES.txt", "LICENSE.txt"):
        if not (resources / name).is_file():
            problems.append(f"missing {resources / name}")
    return problems


def check(release: Path, target: str, manifest: dict, budget_mib: float) -> dict:
    resources, installers = layout(release, target)
    problems = []
    if resources is None:
        problems.append("no .app bundle")
        resources = release / "missing"
    problems += resource_problems(resources, manifest)
    if not installers:
        problems.append(f"no installer under {release / 'bundle'}")
    sizes = {path.name: round(path.stat().st_size / MIB, 1) for path in installers}
    problems += [f"{name} is {size} MiB, budget {budget_mib} MiB" for name, size in sizes.items() if size > budget_mib]
    return {"target": target, "installersMiB": sizes, "budgetMiB": budget_mib, "problems": problems}


def check_installed(directory: Path, target: str, manifest: dict) -> dict:
    resources = directory / "Contents" / "Resources" if "apple-darwin" in target else directory
    return {"target": target, "installed": str(directory), "problems": resource_problems(resources, manifest)}


def main() -> int:
    args = sys.argv[1:]
    installed = args[:1] == ["--installed"]
    if installed:
        args = args[1:]
    if len(args) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    if installed:
        report = check_installed(Path(args[0]), args[1], manifest)
    else:
        budget = json.loads(BUDGETS.read_text(encoding="utf-8"))["app"]["installerMiB"]
        report = check(Path(args[0]), args[1], manifest, budget)
    print(json.dumps(report, indent=2))
    return 1 if report["problems"] else 0


if __name__ == "__main__":
    sys.exit(main())
