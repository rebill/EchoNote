from __future__ import annotations

import hashlib
import json
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build_windows_cpu_zip as builder  # noqa: E402


class WindowsCpuZipBuilderTest(unittest.TestCase):
    def test_builds_a_complete_integrity_checked_archive(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            model = create_model(root / "model")
            wheelhouse = root / "wheelhouse"
            wheelhouse.mkdir()
            dependency = wheelhouse / "dependency-1.0-py3-none-any.whl"
            dependency.write_bytes(b"dependency wheel")
            service_wheel = root / "echonote_asr-0.9.0-py3-none-any.whl"
            service_wheel.write_bytes(b"service wheel")
            requirements = root / "requirements.txt"
            requirements.write_text(
                f"dependency==1.0 --hash=sha256:{hashlib.sha256(dependency.read_bytes()).hexdigest()}\n",
                encoding="ascii",
            )
            output = root / "EchoNote-ASR-Windows-CPU.zip"

            builder.build_windows_cpu_zip(
                output=output,
                model=model,
                wheelhouse=wheelhouse,
                requirements=requirements,
                service_wheel=service_wheel,
                bundle_version="0.9.0-test",
            )

            self.assertTrue(output.is_file())
            archive_root = "EchoNote-ASR-Windows-CPU-0.9.0-test-py311"
            with zipfile.ZipFile(output) as archive:
                names = set(archive.namelist())
                self.assertIn(f"{archive_root}/install-windows-cpu.ps1", names)
                self.assertIn(f"{archive_root}/run-windows-cpu.ps1", names)
                self.assertIn(f"{archive_root}/verify-windows-cpu.ps1", names)
                self.assertIn(f"{archive_root}/models/faster-whisper-small/model.bin", names)
                self.assertIn(f"{archive_root}/wheelhouse/{service_wheel.name}", names)
                manifest = json.loads(archive.read(f"{archive_root}/{builder.MANIFEST_NAME}"))
                checksums = archive.read(f"{archive_root}/{builder.CHECKSUMS_NAME}").decode("ascii")

            self.assertEqual(manifest["target"]["pythonVersion"], "3.11.9")
            self.assertEqual(manifest["model"]["revision"], builder.MODEL_REVISION)
            self.assertIn("models/faster-whisper-small/model.bin", checksums)

    def test_requires_a_complete_model(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            model = Path(raw)
            (model / "config.json").write_text("{}", encoding="utf-8")

            with self.assertRaisesRegex(ValueError, "missing model.bin"):
                builder.require_model(model)

    def test_rejects_symlinks_in_bundle_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source = root / "source"
            source.mkdir()
            target = root / "target.txt"
            target.write_text("target", encoding="utf-8")
            (source / "link.txt").symlink_to(target)

            with self.assertRaisesRegex(ValueError, "must not contain symlinks"):
                builder.copy_tree(source, root / "destination")


def create_model(path: Path) -> Path:
    path.mkdir()
    (path / "config.json").write_text("{}", encoding="utf-8")
    (path / "model.bin").write_bytes(b"model")
    (path / "tokenizer.json").write_text("{}", encoding="utf-8")
    (path / ".cache").mkdir()
    (path / ".cache" / "metadata").write_text("excluded", encoding="utf-8")
    return path


if __name__ == "__main__":
    unittest.main()
