from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import check_bundle as checker  # noqa: E402

MANIFEST = {
    "models": [
        {"role": "live", "dir": "live", "files": [{"path": "model.onnx", "size": 4}]},
        {"role": "vad", "dir": "vad", "files": [{"path": "vad.onnx", "size": 2}]},
        {"role": "accurate", "dir": "big", "files": [{"path": "huge.onnx", "size": 9}]},
    ]
}


def write(root: Path, files: dict[str, bytes]) -> None:
    for name, data in files.items():
        (root / name).parent.mkdir(parents=True, exist_ok=True)
        (root / name).write_bytes(data)


class CheckBundleTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.release = Path(self.tmp.name)

    def resources(self, base: str) -> dict[str, bytes]:
        return {
            f"{base}models/live/model.onnx": b"1234",
            f"{base}models/vad/vad.onnx": b"12",
            f"{base}THIRD_PARTY_NOTICES.txt": b"n",
            f"{base}LICENSE.txt": b"l",
        }

    def test_a_complete_macos_bundle_passes(self) -> None:
        write(self.release, self.resources("bundle/macos/EchoNote.app/Contents/Resources/") | {"bundle/dmg/EchoNote.dmg": b"x" * 10})
        report = checker.check(self.release, "aarch64-apple-darwin", MANIFEST, 1)
        self.assertEqual(report["problems"], [])
        self.assertEqual(report["installersMiB"], {"EchoNote.dmg": 0.0})

    def test_windows_reads_the_resources_next_to_the_executable(self) -> None:
        files = self.resources("")
        files["models/live/model.onnx"] = b"123"
        del files["LICENSE.txt"]
        write(self.release, files | {"bundle/nsis/EchoNote_1.0.0_x64-setup.exe": b"x" * (2 << 20)})
        problems = checker.check(self.release, "x86_64-pc-windows-msvc", MANIFEST, 1)["problems"]
        self.assertEqual(len(problems), 3)
        self.assertIn("is 3 bytes, manifest says 4", problems[0])
        self.assertIn("LICENSE.txt", problems[1])
        self.assertIn("is 2.0 MiB, budget 1 MiB", problems[2])

    def test_a_missing_bundle_is_reported(self) -> None:
        problems = checker.check(self.release, "x86_64-apple-darwin", MANIFEST, 300)["problems"]
        self.assertEqual(problems[0], "no .app bundle")
        self.assertTrue(problems[-1].startswith("no installer"))


if __name__ == "__main__":
    unittest.main()
