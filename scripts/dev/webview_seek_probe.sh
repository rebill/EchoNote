#!/bin/sh
# M1: seek accuracy in a 3-hour Ogg/Opus recording in the system WKWebView (Tauri's web view).
# Writes a timecoded file with the recording writer (`opus_tool timecode`) plus the same audio
# as WAV, seeks to positions spread over each file and decodes the position the web view plays
# after each seek. Playback goes through a zero-gain Web Audio node, so nothing is audible.
# Needs Xcode command line tools and about 400 MB of free disk for the WAV copy.
#
# Usage: scripts/dev/webview_seek_probe.sh [length_s]
# Output: $ECHONOTE_DEV_DATA/results/webview-seek-<timestamp>/result.json
set -eu

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DATA_DIR="${ECHONOTE_DEV_DATA:-$HOME/.cache/echonote-dev}"
LENGTH_S="${1:-10800}"
OUT="$DATA_DIR/results/webview-seek-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$OUT"

(cd "$REPO_ROOT" && cargo build --release -q -p echonote-core --example opus_tool)
"$REPO_ROOT/target/release/examples/opus_tool" timecode "$OUT/long.opus" "$LENGTH_S" "$OUT/long.wav"
echo "const LENGTH_S = $LENGTH_S;" > "$OUT/config.js"
cp "$REPO_ROOT/scripts/dev/webview_seek_probe.html" "$OUT/probe.html"
swiftc -O "$REPO_ROOT/scripts/dev/webview_audio_probe.swift" -o "$OUT/webview_probe"
"$OUT/webview_probe" "$OUT" 600 | tee "$OUT/result.json"
# The files are synthetic and rebuilt on every run.
rm -f "$OUT/long.opus" "$OUT/long.wav"
