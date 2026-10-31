#!/bin/sh
# Live transcription demo on macOS. Builds the demo as an ad-hoc signed app bundle (so macOS
# attributes the microphone and system-audio prompts to it), starts it and shows the transcript
# in this terminal. Ctrl+C stops recording; the remaining audio is transcribed before exit.
#
# Usage: scripts/dev/live_demo.sh [max_minutes]
# Output: $ECHONOTE_DEV_DATA/results/live-<timestamp>/{mic.wav,sys.wav,transcript.jsonl}
set -eu

MAX_MINUTES="${1:-120}"
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DATA_DIR="${ECHONOTE_DEV_DATA:-$HOME/.cache/echonote-dev}"
OUT_DIR="$DATA_DIR/results/live-$(date +%Y%m%d-%H%M%S)"
APP="$DATA_DIR/probe/EchoNoteLiveDemo.app"
export SHERPA_ONNX_ARCHIVE_DIR="${SHERPA_ONNX_ARCHIVE_DIR:-$DATA_DIR/sherpa-onnx}"

cargo build --quiet --manifest-path "$REPO_ROOT/Cargo.toml" --release -p echonote-core --example live_demo

BIN="$REPO_ROOT/target/release/examples/live_demo"
# codesign rewrites the bundled copy, so compare against the unsigned binary of the last bundle.
# A new binary changes the ad-hoc cdhash and macOS asks for both permissions again.
STAMP="$DATA_DIR/probe/live_demo.unsigned"
if [ ! -d "$APP" ] || ! cmp -s "$BIN" "$STAMP"; then
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp "$BIN" "$APP/Contents/MacOS/live_demo"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>com.rebill.echonote.live-demo</string>
  <key>CFBundleName</key><string>EchoNote Live Demo</string>
  <key>CFBundleExecutable</key><string>live_demo</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>14.2</string>
  <key>LSUIElement</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>EchoNote records your microphone to transcribe meetings locally.</string>
  <key>NSAudioCaptureUsageDescription</key><string>EchoNote records system audio so the other side of a call is transcribed.</string>
</dict>
</plist>
PLIST
codesign --force --sign - "$APP" 2>/dev/null
cp "$BIN" "$STAMP"
fi

mkdir -p "$OUT_DIR"
: > "$OUT_DIR/stdout.log"
echo "EchoNote 实时转录演示：麦克风显示为「我」，系统音频显示为「对方」。"
echo "首次运行会弹出麦克风和系统音频录制授权，请点「允许」。按 Ctrl+C 结束。"
echo
# The app draws straight onto this terminal so its one-line status can be redrawn in place;
# stdout.log keeps a plain copy and is tailed only if the terminal cannot be opened.
TTY="$(tty 2>/dev/null || true)"
COLUMNS_NOW="$(stty size 2>/dev/null | awk '{print $2}')"
if [ -n "$TTY" ] && [ "$TTY" != "not a tty" ]; then
  open -W -n "$APP" --env "ECHONOTE_DEMO_TTY=$TTY" --env "ECHONOTE_DEMO_COLUMNS=${COLUMNS_NOW:-100}" \
    --stdout "$OUT_DIR/stdout.log" --stderr "$OUT_DIR/stderr.log" --args "$OUT_DIR" "$MAX_MINUTES" &
else
  open -W -n "$APP" --stdout "$OUT_DIR/stdout.log" --stderr "$OUT_DIR/stderr.log" --args "$OUT_DIR" "$MAX_MINUTES" &
fi
OPEN_PID=$!
# Background jobs ignore SIGINT here, so Ctrl+C only reaches this shell: ask the app to stop.
trap 'touch "$OUT_DIR/stop"' INT TERM
TAIL_PID=""
# The app prints "@tty" first when it can draw on the terminal; wait for its first line.
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
  [ -s "$OUT_DIR/stdout.log" ] && break
  sleep 0.5
done
if ! grep -q "^@tty" "$OUT_DIR/stdout.log"; then
  tail -n +1 -f "$OUT_DIR/stdout.log" &
  TAIL_PID=$!
fi
while kill -0 "$OPEN_PID" 2>/dev/null; do
  wait "$OPEN_PID" 2>/dev/null || true
done
if [ -n "$TAIL_PID" ]; then
  sleep 0.5
  kill "$TAIL_PID" 2>/dev/null || true
fi
echo
# sherpa-onnx logs harmless buffer growth on stderr; show anything else.
if grep -v "circular-buffer" "$OUT_DIR/stderr.log" | grep -q .; then
  echo
  echo "错误输出："
  grep -v "circular-buffer" "$OUT_DIR/stderr.log"
fi
