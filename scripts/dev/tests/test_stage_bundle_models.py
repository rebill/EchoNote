from __future__ import annotations

import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import stage_bundle_models as stager  # noqa: E402

LIVE = b"live model weights"
TOKENS = b"tokens"
VAD = b"vad weights"


def entry(path: str, data: bytes) -> dict:
    return {"path": path, "size": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def manifest(mirror: str = "https://mirror.example/live") -> dict:
    return {
        "models": [
            {"role": "live", "dir": "live", "mirrors": [mirror], "files": [entry("model.onnx", LIVE), entry("tokens.txt", TOKENS)]},
            {"role": "vad", "dir": "vad", "mirrors": ["https://mirror.example/vad"], "files": [entry("vad.onnx", VAD)]},
            {"role": "accurate", "dir": "big", "mirrors": [], "files": [entry("huge.onnx", b"not bundled")]},
        ]
    }


def write(root: Path, files: dict[str, bytes]) -> None:
    for name, data in files.items():
        (root / name).parent.mkdir(parents=True, exist_ok=True)
        (root / name).write_bytes(data)


class StageBundleModelsTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.source = Path(self.tmp.name) / "source"
        self.out = Path(self.tmp.name) / "out"
        self.fetched: list[str] = []

    def fetch_from(self, files: dict[str, bytes]) -> stager.Fetch:
        def fetch(url: str, path: Path) -> None:
            self.fetched.append(url)
            name = url.removeprefix("https://mirror.example/")
            if name not in files:
                raise OSError("404")
            path.write_bytes(files[name])

        return fetch

    def test_copies_matching_local_files_and_only_the_bundled_roles(self) -> None:
        write(self.source, {"live/model.onnx": LIVE, "live/tokens.txt": TOKENS, "vad/vad.onnx": VAD, "big/huge.onnx": b"x"})
        counts = stager.stage(manifest(), self.source, self.out, None)
        self.assertEqual(counts, {"copied": 3})
        self.assertEqual((self.out / "live" / "model.onnx").read_bytes(), LIVE)
        self.assertFalse((self.out / "big").exists())
        self.assertEqual(stager.stage(manifest(), self.source, self.out, None), {"kept": 3})

    def test_downloads_what_is_missing_or_wrong_locally(self) -> None:
        write(self.source, {"live/model.onnx": b"corrupt", "live/tokens.txt": TOKENS})
        fetch = self.fetch_from({"live/model.onnx": LIVE, "vad/vad.onnx": VAD})
        counts = stager.stage(manifest(), self.source, self.out, fetch)
        self.assertEqual(counts, {"copied": 1, "downloaded": 2})
        self.assertEqual((self.out / "live" / "model.onnx").read_bytes(), LIVE)
        self.assertEqual([path.name for path in self.out.rglob("*.part")], [])

    def test_a_download_that_does_not_match_is_not_kept(self) -> None:
        fetch = self.fetch_from({"live/model.onnx": b"tampered"})
        with self.assertRaisesRegex(stager.StageError, "every mirror failed"):
            stager.stage(manifest(), None, self.out, fetch)
        self.assertFalse((self.out / "live" / "model.onnx").exists())
        self.assertEqual([path.name for path in self.out.rglob("*.part")], [])

    def test_offline_fails_without_a_local_copy(self) -> None:
        with self.assertRaisesRegex(stager.StageError, "downloads are off"):
            stager.stage(manifest(), None, self.out, None)

    def test_plain_http_mirrors_are_skipped(self) -> None:
        fetch = self.fetch_from({})
        with self.assertRaises(stager.StageError):
            stager.stage(manifest("http://mirror.example/live"), None, self.out, fetch)
        self.assertEqual(self.fetched, [])

    def test_stale_files_are_removed(self) -> None:
        write(self.source, {"live/model.onnx": LIVE, "live/tokens.txt": TOKENS, "vad/vad.onnx": VAD})
        write(self.out, {"old-model/model.onnx": b"old", "live/extra.txt": b"x", "vad/vad.onnx.part": b"x"})
        stager.stage(manifest(), self.source, self.out, None)
        files = sorted(str(path.relative_to(self.out)) for path in self.out.rglob("*") if path.is_file())
        self.assertEqual(files, ["live/model.onnx", "live/tokens.txt", "vad/vad.onnx"])
        self.assertFalse((self.out / "old-model").exists())

    def test_the_real_manifest_bundles_the_paths_the_app_looks_for(self) -> None:
        models = stager.bundled(json.loads(stager.MANIFEST.read_text(encoding="utf-8")))
        paths = {f"{model['dir']}/{file['path']}" for model in models for file in model["files"]}
        live = "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09"
        self.assertEqual(paths, {f"{live}/model.int8.onnx", f"{live}/tokens.txt", "vad/silero_vad.onnx"})


if __name__ == "__main__":
    unittest.main()
