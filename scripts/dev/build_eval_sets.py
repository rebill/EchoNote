# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy>=1.26", "pyarrow>=15", "soundfile>=0.12"]
# ///
"""Build deterministic EchoNote evaluation sets from assets fetched by fetch_assets.py.

Outputs (under ECHONOTE_DEV_DATA/eval, never committed):
  aishell1-test-s12/   every 12th AISHELL-1 test utterance (about 600, Mandarin read speech)
  librispeech-clean-s13/  every 13th LibriSpeech test-clean utterance (about 200, English)
  meeting-zh-dual/     synthetic two-party meeting from AISHELL-1 dev speakers:
                       mic.wav, sys.wav, mic_bleed.wav (mic plus speaker echo of sys), refs.json
  continuous-zh/       one AISHELL-1 dev speaker with 40-120 ms pauses, like podcast or video
                       playback: the VAD never closes a segment on its own. mic.wav, refs.json

Run: uv run scripts/dev/build_eval_sets.py [--only NAME ...]
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import random
import zipfile
from pathlib import Path

import numpy as np
import pyarrow.parquet as pq
import soundfile as sf

SR = 16_000
SEED = 20261031


def data_dir() -> Path:
    configured = os.environ.get("ECHONOTE_DEV_DATA")
    return Path(configured).expanduser() if configured else Path.home() / ".cache" / "echonote-dev"


def write_manifest(out_dir: Path, rows: list[dict[str, str]]) -> str:
    manifest = out_dir / "manifest.jsonl"
    text = "".join(json.dumps(row, ensure_ascii=False) + "\n" for row in rows)
    manifest.write_text(text, encoding="utf-8")
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def aishell_archive(root: Path) -> tuple[zipfile.ZipFile, dict[str, str]]:
    archive = zipfile.ZipFile(root / "eval/raw/aishell1/speech_asr_aishell1_testsets.zip")
    transcripts: dict[str, str] = {}
    raw = archive.read("speech_asr_aishell1_testsets/transcript/data.text").decode("utf-8")
    for line in raw.splitlines():
        utterance_id, _, text = line.partition(" ")
        transcripts[utterance_id] = text.strip()
    return archive, transcripts


def build_aishell(root: Path, stride: int = 12) -> None:
    archive, transcripts = aishell_archive(root)
    names = sorted(name for name in archive.namelist() if "/wav/test/" in name and name.endswith(".wav"))
    out_dir = root / "eval/aishell1-test-s12"
    (out_dir / "wav").mkdir(parents=True, exist_ok=True)
    rows = []
    for name in names[::stride]:
        utterance_id = Path(name).stem
        if utterance_id not in transcripts:
            continue
        target = out_dir / "wav" / f"{utterance_id}.wav"
        if not target.exists():
            target.write_bytes(archive.read(name))
        rows.append({"id": utterance_id, "audio": f"wav/{utterance_id}.wav", "text": transcripts[utterance_id]})
    digest = write_manifest(out_dir, rows)
    print(f"aishell1-test-s12: {len(rows)} utterances, manifest sha256 {digest}")


def build_librispeech(root: Path, stride: int = 13) -> None:
    table = pq.read_table(root / "eval/raw/librispeech/test.clean-0000.parquet")
    columns = set(table.column_names)
    id_column = "id" if "id" in columns else "file"
    order = sorted(range(table.num_rows), key=lambda index: str(table.column(id_column)[index].as_py()))
    out_dir = root / "eval/librispeech-clean-s13"
    (out_dir / "wav").mkdir(parents=True, exist_ok=True)
    rows = []
    for index in order[::stride]:
        utterance_id = Path(str(table.column(id_column)[index].as_py())).stem
        audio = table.column("audio")[index].as_py()
        target = out_dir / "wav" / f"{utterance_id}.wav"
        if not target.exists():
            samples, rate = sf.read(io.BytesIO(audio["bytes"]), dtype="float32")
            if rate != SR:
                raise RuntimeError(f"{utterance_id}: unexpected sample rate {rate}")
            sf.write(target, samples, SR, subtype="PCM_16")
        rows.append({"id": utterance_id, "audio": f"wav/{utterance_id}.wav", "text": table.column("text")[index].as_py()})
    digest = write_manifest(out_dir, rows)
    print(f"librispeech-clean-s13: {len(rows)} utterances, manifest sha256 {digest}")


def normalize(samples: np.ndarray, target_dbfs: float = -23.0) -> np.ndarray:
    rms = float(np.sqrt(np.mean(samples**2))) or 1e-9
    return np.clip(samples * (10 ** (target_dbfs / 20) / rms), -1.0, 1.0).astype(np.float32)


def delayed(samples: np.ndarray, delay_ms: float) -> np.ndarray:
    shift = int(SR * delay_ms / 1000)
    return np.concatenate([np.zeros(shift, np.float32), samples[: len(samples) - shift]])


def build_meeting(root: Path, minutes: float = 10.0) -> None:
    """Two AISHELL-1 dev speakers alternate turns; about 12% of turns overlap the previous one.

    mic_bleed.wav approximates a laptop without headphones: sys played through the speakers is
    picked up by the mic through three decaying low-passed echo paths plus a noise floor.
    """
    archive, transcripts = aishell_archive(root)
    by_speaker: dict[str, list[str]] = {}
    for name in sorted(n for n in archive.namelist() if "/wav/dev/" in n and n.endswith(".wav")):
        if Path(name).stem in transcripts:
            by_speaker.setdefault(name.split("/")[-2], []).append(name)
    speakers = sorted(by_speaker)[:2]
    rng = random.Random(SEED)
    queues = {speaker: rng.sample(by_speaker[speaker], len(by_speaker[speaker])) for speaker in speakers}
    owners = {"mic": speakers[0], "sys": speakers[1]}

    capacity = int((minutes * 60 + 30) * SR)
    buffers = {track: np.zeros(capacity, np.float32) for track in owners}
    refs: list[dict[str, object]] = []
    cursor, track, last_end = SR, "mic", SR
    while cursor < minutes * 60 * SR:
        for _ in range(rng.randint(1, 3)):
            name = queues[owners[track]].pop()
            samples, rate = sf.read(io.BytesIO(archive.read(name)), dtype="float32")
            assert rate == SR, name
            samples = normalize(samples)
            end = cursor + len(samples)
            buffers[track][cursor:end] += samples
            refs.append({"track": track, "start_ms": cursor * 1000 // SR, "end_ms": end * 1000 // SR,
                         "text": transcripts[Path(name).stem], "id": Path(name).stem})
            last_end = end
            cursor = end + int(rng.uniform(0.25, 0.7) * SR)
        gap = -rng.uniform(0.3, 1.0) if rng.random() < 0.12 else rng.uniform(0.4, 1.5)
        cursor = max(SR, last_end + int(gap * SR))
        track = "sys" if track == "mic" else "mic"

    length = last_end + SR
    mic, sys_track = buffers["mic"][:length], buffers["sys"][:length]
    speaker_path = np.convolve(sys_track, np.ones(4, np.float32) / 4, mode="same")
    noise = np.random.default_rng(SEED).normal(0, 0.001, length).astype(np.float32)
    bleed = mic + 0.35 * delayed(speaker_path, 25) + 0.12 * delayed(speaker_path, 60) + 0.05 * delayed(speaker_path, 110) + noise

    out_dir = root / "eval/meeting-zh-dual"
    out_dir.mkdir(parents=True, exist_ok=True)
    for name, samples in {"mic": mic, "sys": sys_track, "mic_bleed": np.clip(bleed, -1, 1)}.items():
        sf.write(out_dir / f"{name}.wav", samples, SR, subtype="PCM_16")
    text = json.dumps(refs, ensure_ascii=False, indent=0)
    (out_dir / "refs.json").write_text(text, encoding="utf-8")
    digest = hashlib.sha256(text.encode("utf-8")).hexdigest()
    overlaps = sum(1 for a, b in zip(refs, refs[1:]) if a["track"] != b["track"] and b["start_ms"] < a["end_ms"])
    print(f"meeting-zh-dual: {length / SR / 60:.1f} min, {len(refs)} utterances, {overlaps} overlapping turns, "
          f"speakers {speakers}, refs sha256 {digest}")


def trim_silence(samples: np.ndarray, margin_ms: float = 30.0) -> np.ndarray:
    frame = SR // 100
    frames = len(samples) // frame
    energy = 10 * np.log10(np.mean(samples[: frames * frame].reshape(frames, frame) ** 2, axis=1) + 1e-10)
    voiced = np.flatnonzero(energy > energy.max() - 35)
    margin = int(margin_ms * SR / 1000)
    return samples[max(voiced[0] * frame - margin, 0) : min((voiced[-1] + 1) * frame + margin, len(samples))]


def build_continuous(root: Path, minutes: float = 5.0) -> None:
    """Back-to-back utterances of the third AISHELL-1 dev speaker, edges trimmed."""
    archive, transcripts = aishell_archive(root)
    by_speaker: dict[str, list[str]] = {}
    for name in sorted(n for n in archive.namelist() if "/wav/dev/" in n and n.endswith(".wav")):
        if Path(name).stem in transcripts:
            by_speaker.setdefault(name.split("/")[-2], []).append(name)
    speaker = sorted(by_speaker)[2]
    rng = random.Random(SEED + 1)
    pieces, refs, cursor = [np.zeros(SR // 2, np.float32)], [], SR // 2
    for name in by_speaker[speaker]:
        if cursor >= minutes * 60 * SR:
            break
        samples, rate = sf.read(io.BytesIO(archive.read(name)), dtype="float32")
        assert rate == SR, name
        samples = normalize(trim_silence(samples))
        refs.append({"track": "mic", "start_ms": cursor * 1000 // SR, "end_ms": (cursor + len(samples)) * 1000 // SR,
                     "text": transcripts[Path(name).stem], "id": Path(name).stem})
        gap = np.zeros(int(rng.uniform(0.04, 0.12) * SR), np.float32)
        pieces += [samples, gap]
        cursor += len(samples) + len(gap)
    pieces.append(np.zeros(SR // 2, np.float32))
    out_dir = root / "eval/continuous-zh"
    out_dir.mkdir(parents=True, exist_ok=True)
    sf.write(out_dir / "mic.wav", np.concatenate(pieces), SR, subtype="PCM_16")
    text = json.dumps(refs, ensure_ascii=False, indent=0)
    (out_dir / "refs.json").write_text(text, encoding="utf-8")
    digest = hashlib.sha256(text.encode("utf-8")).hexdigest()
    print(f"continuous-zh: {cursor / SR / 60:.1f} min, {len(refs)} utterances, speaker {speaker}, refs sha256 {digest}")


BUILDERS = {
    "aishell1-test-s12": build_aishell,
    "librispeech-clean-s13": build_librispeech,
    "meeting-zh-dual": build_meeting,
    "continuous-zh": build_continuous,
}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--only", nargs="*", choices=sorted(BUILDERS), help="build only these sets")
    args = parser.parse_args()
    root = data_dir()
    for name, build in BUILDERS.items():
        if not args.only or name in args.only:
            build(root)


if __name__ == "__main__":
    main()
