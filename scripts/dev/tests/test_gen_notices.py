from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import gen_notices as notices  # noqa: E402


def dep(pkg: str, *kinds: str | None) -> dict:
    return {"pkg": pkg, "dep_kinds": [{"kind": kind} for kind in kinds]}


def package(root: Path, name: str, source: str | None = "registry", files: dict[str, str] | None = None) -> dict:
    directory = root / name
    directory.mkdir()
    (directory / "Cargo.toml").write_text("", encoding="utf-8")
    for file, text in (files or {}).items():
        (directory / file).write_text(text, encoding="utf-8")
    return {
        "id": name,
        "name": name,
        "version": "1.0.0",
        "license": "MIT",
        "source": source,
        "repository": f"https://example.com/{name}",
        "manifest_path": str(directory / "Cargo.toml"),
    }


class GenNoticesTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def test_normal_and_build_dependencies_ship_and_dev_dependencies_do_not(self) -> None:
        metadata = {
            "packages": [{"id": name, "name": name} for name in ["app", "runtime", "build-helper", "test-only", "nested"]],
            "resolve": {
                "nodes": [
                    {"id": "app", "deps": [dep("runtime", None), dep("build-helper", "build"), dep("test-only", "dev")]},
                    {"id": "runtime", "deps": [dep("nested", None, "dev")]},
                    {"id": "build-helper", "deps": []},
                    {"id": "test-only", "deps": []},
                    {"id": "nested", "deps": []},
                ]
            },
        }
        self.assertEqual(notices.shipped_packages(metadata, "app"), {"app", "runtime", "build-helper", "nested"})

    def test_crates_read_their_license_files_and_fall_back_to_overrides(self) -> None:
        metadata = {
            "packages": [
                package(self.root, "filed", files={"LICENSE-MIT": "MIT text", "README.md": "not a license"}),
                package(self.root, "unfiled"),
                package(self.root, "workspace", source=None),
            ]
        }
        entries = notices.crate_entries(metadata, {"filed", "unfiled", "workspace"}, {"unfiled": ["override text"]})
        self.assertEqual([(entry.name, entry.texts) for entry in entries], [("filed", ["MIT text"]), ("unfiled", ["override text"])])
        with self.assertRaisesRegex(notices.NoticeError, "unfiled 1.0.0 ships no license file"):
            notices.crate_entries(metadata, {"unfiled"}, {})

    def test_each_license_text_is_printed_once_for_everything_it_covers(self) -> None:
        entries = [
            notices.Entry("Native", "a", "1", "MIT", "https://a", texts=["MIT  text\n"]),
            notices.Entry("Native", "b", "2", "MIT", "https://b", attribution="Built from source.", texts=["MIT text"]),
            notices.Entry("Rust crates", "c", "3", "Apache-2.0", "https://c", texts=["Apache text"]),
        ]
        text = notices.render(entries)
        self.assertEqual(text.count("Applies to: "), 2)
        self.assertIn("Applies to: a 1, b 2\n", text)
        self.assertIn("  Built from source.\n", text)
        self.assertLess(text.index("== Native =="), text.index("== Rust crates =="))

    def test_frontend_components_use_the_installed_version(self) -> None:
        modules = self.root / "node_modules"
        (modules / "lib").mkdir(parents=True)
        (modules / "lib" / "package.json").write_text(json.dumps({"version": "4.5.6"}), encoding="utf-8")
        (modules / "lib" / "LICENSE.md").write_text("Core license\n# Bundled\nother licenses", encoding="utf-8")
        spec = {"group": "Frontend", "name": "lib", "npm": "lib", "license": "MIT", "source": "s", "files": ["lib/LICENSE.md"], "until": "# Bundled"}
        entry = notices.component(spec, self.root, modules)
        self.assertEqual((entry.version, entry.texts), ("4.5.6", ["Core license"]))
        with self.assertRaisesRegex(notices.NoticeError, "pnpm install"):
            notices.component(spec, self.root, self.root / "missing")

    def test_every_vendored_file_in_notices_json_exists(self) -> None:
        manifest = json.loads((notices.THIRD_PARTY / "notices.json").read_text(encoding="utf-8"))
        vendored = [spec for spec in manifest["components"] if "npm" not in spec] + manifest["crate_licenses"]
        missing = [name for spec in vendored for name in spec["files"] if not (notices.THIRD_PARTY / name).is_file()]
        self.assertEqual(missing, [])


if __name__ == "__main__":
    unittest.main()
