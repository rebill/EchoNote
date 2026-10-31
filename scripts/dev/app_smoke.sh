#!/bin/sh
# Smoke run of the desktop app with fake input: builds the frontend and a debug app with
# the embedded frontend, plays the first SECONDS of the synthetic meeting-zh-dual set (mic
# with speaker bleed, and system audio) through the real UI in a scratch library, and prints
# the UI's report. AI features run against scripts/dev/mock_llm.py on this computer, with
# keys kept in memory; model downloads come from the dev models through
# scripts/dev/model_mirror.py; the high-accuracy pass uses the dev Qwen3-ASR (about 1.3 GB
# of memory for a minute). No microphone, no permission prompts, no keychain, no network;
# the user's library is not touched.
#
# Usage: scripts/dev/app_smoke.sh [seconds]
# Needs: $ECHONOTE_DEV_DATA/eval/meeting-zh-dual and models (scripts/dev/fetch_assets.py,
#        scripts/dev/build_eval_sets.py).
# Output: $ECHONOTE_DEV_DATA/results/app-smoke-<timestamp>/report.json
set -eu

SECONDS_TO_PLAY="${1:-45}"
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DATA_DIR="${ECHONOTE_DEV_DATA:-$HOME/.cache/echonote-dev}"
EVAL="$DATA_DIR/eval/meeting-zh-dual"
OUT="$DATA_DIR/results/app-smoke-$(date +%Y%m%d-%H%M%S)"
export SHERPA_ONNX_ARCHIVE_DIR="${SHERPA_ONNX_ARCHIVE_DIR:-$DATA_DIR/sherpa-onnx}"
mkdir -p "$OUT/input" "$OUT/library" "$OUT/exports"

python3 - "$EVAL" "$OUT/input" "$SECONDS_TO_PLAY" <<'EOF'
import sys, wave
src, dst, seconds = sys.argv[1], sys.argv[2], float(sys.argv[3])
for name, out in [("mic_bleed.wav", "mic.wav"), ("sys.wav", "sys.wav")]:
    with wave.open(f"{src}/{name}") as w:
        frames = w.readframes(int(seconds * w.getframerate()))
        params = w.getparams()
    with wave.open(f"{dst}/{out}", "wb") as o:
        o.setparams(params)
        o.writeframes(frames)
EOF

python3 "$REPO_ROOT/scripts/dev/mock_llm.py" "$OUT/mock.port" "$OUT/mock.log" &
mock=$!
# Model downloads come from the dev models through a slow local mirror.
mkdir -p "$OUT/mirror"
for dir in vad sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25; do
  [ -d "$DATA_DIR/models/$dir" ] && ln -s "$DATA_DIR/models/$dir" "$OUT/mirror/$dir"
done
python3 "$REPO_ROOT/scripts/dev/model_mirror.py" "$OUT/mirror" "$OUT/mirror.port" &
mirror=$!
trap 'kill "$mock" "$mirror" 2>/dev/null || true' EXIT
while [ ! -s "$OUT/mock.port" ] || [ ! -s "$OUT/mirror.port" ]; do sleep 0.1; done
LLM_URL="http://127.0.0.1:$(cat "$OUT/mock.port")/v1"
MIRROR_URL="http://127.0.0.1:$(cat "$OUT/mirror.port")"

(cd "$REPO_ROOT/app" && pnpm build >/dev/null)
cargo build --quiet --manifest-path "$REPO_ROOT/Cargo.toml" -p echonote-app --features custom-protocol

started=$(date +%s)
ECHONOTE_DATA_DIR="$OUT/library" ECHONOTE_FAKE_INPUT="$OUT/input" ECHONOTE_SMOKE="$OUT/report.json" \
  ECHONOTE_EXPORT_DIR="$OUT/exports" ECHONOTE_SMOKE_LLM="$LLM_URL" ECHONOTE_HIGHLIGHTS_SECONDS=10 \
  ECHONOTE_MODEL_MIRROR="$MIRROR_URL" \
  "$REPO_ROOT/target/debug/echonote-app" >"$OUT/stdout.log" 2>"$OUT/stderr.log" &
pid=$!
# Real-time playback plus model load, the stop drain, the AI checks and the Qwen3-ASR pass.
limit=$((SECONDS_TO_PLAY + 420))
while kill -0 "$pid" 2>/dev/null; do
  if [ $(($(date +%s) - started)) -gt "$limit" ]; then
    echo "smoke run timed out after ${limit}s" >&2
    kill "$pid"
    exit 1
  fi
  sleep 1
done
cat "$OUT/report.json"
# The exports' shape, without printing transcript text.
python3 - "$OUT/exports" <<'PY'
import pathlib, re, sys
for path in sorted(pathlib.Path(sys.argv[1]).iterdir()):
    lines = path.read_text(encoding="utf-8").splitlines()
    stamped = [line for line in lines if re.match(r"^\[\d{2}:\d{2}(:\d{2})?\] ", line)]
    labelled = [line for line in stamped if re.match(r"^\[[\d:]+\] (我|对方)：", line)]
    print(f"{path.suffix}: {len(lines)} lines, {len(stamped)} timestamped, {len(labelled)} labelled, "
          f"heading={lines[0].startswith('# ')}, transcript_section={'## 转录' in lines}, "
          f"notes_section={'## 概要' in lines}")
PY
# What the mock server was asked (task names only), and that the app log leaks nothing.
python3 - "$OUT" "$LLM_URL" <<'PY'
import collections, pathlib, sys
out, url = pathlib.Path(sys.argv[1]), sys.argv[2]
tasks = collections.Counter(line.split(" ", 1)[1] for line in (out / "mock.log").read_text().splitlines())
print("mock requests:", dict(tasks))
log = (out / "library/logs/app.log").read_text(encoding="utf-8")
leaks = [word for word in ["smoke-key", url, "127.0.0.1", "SMOKEFIX", "冒烟", "<transcript>"] if word in log]
print("app log leaks:", leaks or "none")
PY
