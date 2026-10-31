"""Run the M0 benchmark matrix and write JSON reports plus a Markdown summary.

Prerequisites: fetch_assets.py, build_eval_sets.py (uv run), and
`cargo build --release -p echonote-bench`. Works on macOS and Windows with the standard library.

Usage: python scripts/dev/run_m0_bench.py [--only PREFIX ...] [--limit N]
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import platform
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]


def data_dir() -> Path:
    configured = os.environ.get("ECHONOTE_DEV_DATA")
    return Path(configured).expanduser() if configured else Path.home() / ".cache" / "echonote-dev"


def jobs(data: Path) -> list[tuple[str, list[str]]]:
    zh = ["--manifest", str(data / "eval/aishell1-test-s12/manifest.jsonl"), "--cn-numerals", "--t2s", str(data / "eval/t2s.json")]
    en = ["--manifest", str(data / "eval/librispeech-clean-s13/manifest.jsonl")]
    meeting = data / "eval/meeting-zh-dual"
    dual = ["--mic", str(meeting / "mic.wav"), "--sys", str(meeting / "sys.wav"), "--refs", str(meeting / "refs.json"),
            "--cn-numerals", "--t2s", str(data / "eval/t2s.json")]
    bleed = ["--mic", str(meeting / "mic_bleed.wav")] + dual[2:]
    longform = data / "audio/longform"
    continuous = ["--mic", str(data / "eval/continuous-zh/mic.wav"), "--refs", str(data / "eval/continuous-zh/refs.json"),
                  "--cn-numerals", "--t2s", str(data / "eval/t2s.json")]
    return [
        ("eval-zh-sense-voice-t1", ["eval", "--model", "sense-voice", "--no-itn", "--threads", "1", *zh]),
        ("eval-zh-sense-voice-t2", ["eval", "--model", "sense-voice", "--no-itn", "--threads", "2", *zh]),
        ("eval-zh-sense-voice-t4", ["eval", "--model", "sense-voice", "--no-itn", "--threads", "4", *zh]),
        ("eval-zh-paraformer-t2", ["eval", "--model", "paraformer", "--threads", "2", *zh]),
        ("eval-zh-qwen3-asr-t2", ["eval", "--model", "qwen3-asr", "--threads", "2", *zh]),
        ("eval-zh-qwen3-asr-t4", ["eval", "--model", "qwen3-asr", "--threads", "4", *zh]),
        ("eval-zh-funasr-nano-t2", ["eval", "--model", "funasr-nano", "--no-itn", "--threads", "2", *zh]),
        ("eval-en-sense-voice-t2", ["eval", "--model", "sense-voice", "--threads", "2", *en]),
        ("eval-en-qwen3-asr-t2", ["eval", "--model", "qwen3-asr", "--threads", "2", *en]),
        ("eval-en-funasr-nano-t2", ["eval", "--model", "funasr-nano", "--threads", "2", *en]),
        ("live-dual-sense-voice-none-t2", ["live", "--model", "sense-voice", "--no-itn", "--preview", "none", *dual]),
        ("live-dual-sense-voice-redecode-t2", ["live", "--model", "sense-voice", "--no-itn", "--preview", "redecode", *dual]),
        ("live-dual-sense-voice-redecode-t1", ["live", "--model", "sense-voice", "--no-itn", "--threads", "1", "--preview", "redecode", *dual]),
        ("live-dual-sense-voice-redecode-i1.0", ["live", "--model", "sense-voice", "--no-itn", "--preview", "redecode", "--preview-interval", "1.0", *dual]),
        ("live-dual-sense-voice-redecode-max6", ["live", "--model", "sense-voice", "--no-itn", "--preview", "redecode", "--max-speech", "6", *dual]),
        ("live-dual-sense-voice-streaming-t2", ["live", "--model", "sense-voice", "--no-itn", "--preview", "streaming", *dual]),
        ("live-dual-sense-voice-redecode-ten-vad", ["live", "--model", "sense-voice", "--no-itn", "--vad", "ten", "--preview", "redecode", *dual]),
        ("live-bleed-sense-voice-redecode-t2", ["live", "--model", "sense-voice", "--no-itn", "--preview", "redecode", *bleed]),
        ("live-dual-paraformer-redecode-t2", ["live", "--model", "paraformer", "--preview", "redecode", *dual]),
        ("live-dual-qwen3-asr-none-t2", ["live", "--model", "qwen3-asr", "--preview", "none", *dual]),
        ("live-dual-qwen3-asr-none-t4", ["live", "--model", "qwen3-asr", "--threads", "4", "--preview", "none", *dual]),
        ("live-continuous-sense-voice-redecode-cap12", ["live", "--model", "sense-voice", "--no-itn", "--preview", "redecode", *continuous]),
        ("live-continuous-sense-voice-redecode-cap8", ["live", "--model", "sense-voice", "--no-itn", "--preview", "redecode", "--hard-cap", "8", *continuous]),
        ("live-continuous-sense-voice-redecode-nocap", ["live", "--model", "sense-voice", "--no-itn", "--preview", "redecode", "--hard-cap", "0", *continuous]),
        ("live-continuous-sense-voice-streaming-cap12", ["live", "--model", "sense-voice", "--no-itn", "--preview", "streaming", *continuous]),
        ("live-longform-zh-sense-voice-redecode", ["live", "--model", "sense-voice", "--preview", "redecode", "--mic", str(longform / "lei-jun-test.wav")]),
        ("live-longform-en-sense-voice-redecode", ["live", "--model", "sense-voice", "--preview", "redecode", "--mic", str(longform / "Obama.wav")]),
    ]


def bench_binary(explicit: str | None) -> Path:
    if explicit:
        return Path(explicit)
    name = "echonote-bench.exe" if os.name == "nt" else "echonote-bench"
    return REPO_ROOT / "target" / "release" / name


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--only", nargs="*", default=[], help="run jobs whose name starts with any prefix")
    parser.add_argument("--limit", type=int, help="limit utterances for eval jobs (smoke runs)")
    parser.add_argument("--bench", help="path to echonote-bench")
    args = parser.parse_args()

    data = data_dir()
    binary = bench_binary(args.bench)
    if not binary.is_file():
        parser.error(f"{binary} not found; run cargo build --release -p echonote-bench")
    stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    out_dir = data / "results" / f"m0-{platform.node()}-{stamp}"
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "machine.json").write_text(json.dumps(machine_info(), indent=2) + "\n", encoding="utf-8")

    selected = [job for job in jobs(data) if not args.only or any(job[0].startswith(prefix) for prefix in args.only)]
    for index, (name, job_args) in enumerate(selected, 1):
        command = [str(binary), *job_args]
        if job_args[0] == "eval":
            command += ["--hyp-out", str(out_dir / f"{name}.hyp.jsonl")]
            if args.limit:
                command += ["--limit", str(args.limit)]
        else:
            command += ["--finals-out", str(out_dir / f"{name}.finals.jsonl")]
        print(f"[{index}/{len(selected)}] {name}", flush=True)
        with (out_dir / f"{name}.log").open("w", encoding="utf-8") as log:
            result = subprocess.run(command, stdout=subprocess.PIPE, stderr=log, text=True, encoding="utf-8")
        if result.returncode != 0:
            print(f"  FAILED (exit {result.returncode}); see {name}.log", flush=True)
            continue
        (out_dir / f"{name}.json").write_text(result.stdout, encoding="utf-8")
    print(f"results: {out_dir}")
    return 0


def machine_info() -> dict[str, object]:
    info: dict[str, object] = {"platform": platform.platform(), "machine": platform.machine(), "cpus": os.cpu_count()}
    try:
        if sys.platform == "darwin":
            info["cpu"] = subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"], text=True).strip()
            info["memory_gb"] = int(subprocess.check_output(["sysctl", "-n", "hw.memsize"], text=True)) / 2**30
        elif os.name == "nt":
            info["cpu"] = platform.processor()
    except (OSError, subprocess.CalledProcessError):
        pass
    return info


if __name__ == "__main__":
    sys.exit(main())
