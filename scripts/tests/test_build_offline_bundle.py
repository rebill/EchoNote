from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_offline_bundle as builder  # noqa: E402


class OfflineBundleBuilderTest(unittest.TestCase):
    def test_materializes_symlinks_and_hashes_every_file(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source = root / "source"
            source.mkdir()
            blob = root / "blob.bin"
            blob.write_bytes(b"model weights")
            (source / "weights.bin").symlink_to(blob)
            (source / "config.json").write_text("{}", encoding="utf-8")
            destination = root / "bundle" / "models" / "asr"

            builder.materialize_tree(source, destination)
            manifest = builder.build_file_manifest(root / "bundle")

            copied = destination / "weights.bin"
            self.assertTrue(copied.is_file())
            self.assertFalse(copied.is_symlink())
            by_path = {entry["path"]: entry for entry in manifest}
            self.assertEqual(
                by_path["models/asr/weights.bin"]["sha256"],
                hashlib.sha256(b"model weights").hexdigest(),
            )

    def test_rejects_remote_requirements(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            requirements = Path(raw) / "requirements.txt"
            requirements.write_text(
                "example @ https://example.invalid/example.whl\n", encoding="utf-8"
            )

            with self.assertRaisesRegex(ValueError, "remote or editable"):
                builder.validate_requirements(requirements)

    def test_generated_manifest_json_shape_is_serializable(self) -> None:
        manifest = {
            "schemaVersion": 1,
            "bundleVersion": "0.9.0-test",
            "platform": builder.current_platform(),
            "pythonVersion": "3.11",
            "wheelsPath": "wheels",
            "requirementsPath": "requirements-offline.txt",
            "models": [],
            "files": [],
        }
        self.assertEqual(json.loads(json.dumps(manifest))["schemaVersion"], 1)

    def test_cli_builds_a_complete_fixture_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            wheelhouse = root / "wheelhouse"
            wheelhouse.mkdir()
            (wheelhouse / "fixture-1.0-py3-none-any.whl").write_bytes(b"wheel")
            requirements = root / "requirements.txt"
            requirements.write_text("fixture==1.0\n", encoding="utf-8")
            asr = root / "asr"
            asr.mkdir()
            (asr / "config.json").write_text("{}", encoding="utf-8")
            diarization = root / "diarization"
            diarization.mkdir()
            (diarization / "config.yaml").write_text("pipeline: {}\n", encoding="utf-8")
            output = root / "offline-bundle"

            completed = subprocess.run(
                [
                    sys.executable,
                    str(Path(builder.__file__).resolve()),
                    "--output",
                    str(output),
                    "--wheelhouse",
                    str(wheelhouse),
                    "--requirements",
                    str(requirements),
                    "--asr-model",
                    f"qwen3-0.6b-4bit={asr}",
                    "--diarization-model",
                    str(diarization),
                    "--bundle-version",
                    "0.9.0-test",
                ],
                check=False,
                capture_output=True,
                text=True,
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            manifest = json.loads((output / builder.MANIFEST_NAME).read_text(encoding="utf-8"))
            self.assertEqual(manifest["bundleVersion"], "0.9.0-test")
            self.assertEqual(len(manifest["files"]), 4)


if __name__ == "__main__":
    unittest.main()
