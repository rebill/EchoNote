#!/bin/sh
# M2 smoke run of the desktop app with fake input: builds the frontend and a debug app with
# the embedded frontend, plays the first SECONDS of the synthetic meeting-zh-dual set (mic
# with speaker bleed, and system audio) through the real UI in a scratch library, and prints
# the UI's report. No microphone, no permission prompts, the user's library is not touched.
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

(cd "$REPO_ROOT/app" && pnpm build >/dev/null)
cargo build --quiet --manifest-path "$REPO_ROOT/Cargo.toml" -p echonote-app --features custom-protocol

started=$(date +%s)
ECHONOTE_DATA_DIR="$OUT/library" ECHONOTE_FAKE_INPUT="$OUT/input" ECHONOTE_SMOKE="$OUT/report.json" \
  ECHONOTE_EXPORT_DIR="$OUT/exports" \
  "$REPO_ROOT/target/debug/echonote-app" >"$OUT/stdout.log" 2>"$OUT/stderr.log" &
pid=$!
# Real-time playback plus model load and the stop drain.
limit=$((SECONDS_TO_PLAY + 120))
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
          f"heading={lines[0].startswith('# ')}, transcript_section={'## 转录' in lines}")
PY
