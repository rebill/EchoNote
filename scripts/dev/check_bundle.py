#!/usr/bin/env python3
"""Check a `tauri build` output: bundled models, notices, and installer size.

The models must sit where the app looks (`<resources>/models/<dir>/<file>`, sizes from
crates/echonote-models/manifest.json) and every installer must fit `app.installerMiB` in
docs/v1-performance-budgets.json. Prints a JSON report; exit status 1 on any failure.

    python3 scripts/dev/check_bundle.py <cargo release dir> <rust target>
    e.g. python3 scripts/dev/check_bundle.py target/x86_64-pc-windows-msvc/release x86_64-pc-windows-msvc
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


def check(release: Path, target: str, manifest: dict, budget_mib: float) -> dict:
    resources, installers = layout(release, target)
    problems = []
    if resources is None:
        problems.append("no .app bundle")
        resources = release / "missing"
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
    if not installers:
        problems.append(f"no installer under {release / 'bundle'}")
    sizes = {path.name: round(path.stat().st_size / MIB, 1) for path in installers}
    problems += [f"{name} is {size} MiB, budget {budget_mib} MiB" for name, size in sizes.items() if size > budget_mib]
    return {"target": target, "installersMiB": sizes, "budgetMiB": budget_mib, "problems": problems}


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    budget = json.loads(BUDGETS.read_text(encoding="utf-8"))["app"]["installerMiB"]
    report = check(Path(sys.argv[1]), sys.argv[2], manifest, budget)
    print(json.dumps(report, indent=2))
    return 1 if report["problems"] else 0


if __name__ == "__main__":
    sys.exit(main())
