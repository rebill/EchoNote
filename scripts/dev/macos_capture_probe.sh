#!/bin/sh
# M0 Spike B: build the capture probe as an ad-hoc signed app bundle so macOS attributes the
# microphone and system-audio permission prompts to it, then record for N seconds.
#
# Usage: scripts/dev/macos_capture_probe.sh [seconds]
#   PROBE_GUIDE=1 shows notifications splitting the run into thirds:
#   playback only, speech only, both. PROBE_CLIPS=<dir> plays <dir>/clip_a.wav in the first
#   third and clip_b.wav in the last instead of relying on manual playback.
# Output: $ECHONOTE_DEV_DATA/results/capture-probe-<timestamp>/{mic.wav,sys.wav,report.json}
set -eu

SECONDS_TO_RECORD="${1:-30}"
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DATA_DIR="${ECHONOTE_DEV_DATA:-$HOME/.cache/echonote-dev}"
OUT_DIR="$DATA_DIR/results/capture-probe-$(date +%Y%m%d-%H%M%S)"
APP="$DATA_DIR/probe/EchoNoteCaptureProbe.app"

cargo build --manifest-path "$REPO_ROOT/Cargo.toml" --release -p echonote-core --example capture_probe

BIN="$REPO_ROOT/target/release/examples/capture_probe"
# codesign rewrites the bundled copy, so compare against the unsigned binary of the last bundle.
# Re-signing changes the cdhash and macOS asks for both permissions again.
STAMP="$DATA_DIR/probe/capture_probe.unsigned"
if [ ! -d "$APP" ] || ! cmp -s "$BIN" "$STAMP"; then
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp "$BIN" "$APP/Contents/MacOS/capture_probe"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>com.rebill.echonote.capture-probe</string>
  <key>CFBundleName</key><string>EchoNote Capture Probe</string>
  <key>CFBundleExecutable</key><string>capture_probe</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>14.2</string>
  <key>LSUIElement</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>EchoNote records your microphone to transcribe meetings locally.</string>
  <key>NSAudioCaptureUsageDescription</key><string>EchoNote records system audio so the other side of a call is transcribed.</string>
</dict>
</plist>
PLIST
codesign --force --sign - "$APP"
cp "$BIN" "$STAMP"
fi

notify() {
  osascript -e "display notification \"$2\" with title \"EchoNote 采集探针\" subtitle \"$1\"" >/dev/null 2>&1 || true
}
if [ "${PROBE_GUIDE:-0}" = 1 ]; then
  THIRD=$((SECONDS_TO_RECORD / 3))
  CLIPS="${PROBE_CLIPS:-}"
  (
    if [ -n "$CLIPS" ]; then
      # Scripted playback: clip_a alone, speech alone, clip_b over speech.
      sleep 1
      notify "第 1 段（${THIRD}s）" "播放测试音频，请保持安静"; afplay "$CLIPS/clip_a.wav" &
      sleep "$THIRD"; notify "第 2 段（${THIRD}s）" "请朗读第 1 段文字"
      sleep "$THIRD"; notify "第 3 段（${THIRD}s）" "播放测试音频，同时朗读第 2 段文字"; afplay "$CLIPS/clip_b.wav" &
    else
      notify "第 1 段（${THIRD}s）" "用扬声器播放有人声的视频，自己不说话"
      sleep "$THIRD"; notify "第 2 段（${THIRD}s）" "暂停播放，自己说话"
      sleep "$THIRD"; notify "第 3 段（${THIRD}s）" "恢复播放，同时说话"
    fi
    sleep "$THIRD"; notify "结束" "录制完成"
  ) &
fi

mkdir -p "$OUT_DIR"
echo "Recording ${SECONDS_TO_RECORD}s into $OUT_DIR. Allow the permission prompts, play audio, and speak."
open -W -n "$APP" ${ECHONOTE_TAP_DEBUG:+--env ECHONOTE_TAP_DEBUG=1} --stdout "$OUT_DIR/stdout.log" --stderr "$OUT_DIR/stderr.log" --args "$OUT_DIR" "$SECONDS_TO_RECORD"
cat "$OUT_DIR/report.json"
