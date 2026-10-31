#!/bin/sh
# M0 Spike E: encode a 20 s test tone in each candidate recording format and report what the
# system WKWebView can play, seek and decode. Needs ffmpeg with libopus and Xcode command line tools.
#
# Usage: scripts/dev/webview_audio_probe.sh
# Output: $ECHONOTE_DEV_DATA/results/webview-audio-<timestamp>/result.json
set -eu

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DATA_DIR="${ECHONOTE_DEV_DATA:-$HOME/.cache/echonote-dev}"
OUT="$DATA_DIR/results/webview-audio-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$OUT"
cd "$OUT"

ff() { ffmpeg -hide_banner -loglevel error -y "$@"; }
ff -f lavfi -i "sine=frequency=440:duration=20" -ar 16000 -ac 1 tone.wav
ff -i tone.wav -c:a libopus -b:a 24k -f ogg ogg_opus.ogg
ff -i tone.wav -c:a libopus -b:a 24k webm_opus.webm
ff -i tone.wav -c:a libopus -b:a 24k mp4_opus.mp4
ff -i tone.wav -c:a aac -b:a 32k aac.m4a
# AudioToolbox writes Opus into CAF only; its Ogg writer fails with 'pck?'.
afconvert -f caff -d opus@16000 -b 24000 tone.wav caf_opus.caf

python3 - <<'EOF'
import base64, json, pathlib
samples = [
    ("ogg_opus", "ogg_opus.ogg", 'audio/ogg; codecs="opus"'),
    ("webm_opus", "webm_opus.webm", 'audio/webm; codecs="opus"'),
    ("mp4_opus", "mp4_opus.mp4", 'audio/mp4; codecs="opus"'),
    ("caf_opus", "caf_opus.caf", 'audio/x-caf; codecs="opus"'),
    ("aac_m4a", "aac.m4a", 'audio/mp4; codecs="mp4a.40.2"'),
    ("wav_pcm", "tone.wav", "audio/wav"),
]
rows = []
for name, file, mime in samples:
    data = pathlib.Path(file).read_bytes()
    rows.append({"name": name, "file": file, "mime": mime, "bytes": len(data), "base64": base64.b64encode(data).decode()})
pathlib.Path("samples.js").write_text("const SAMPLES = " + json.dumps(rows) + ";\n")
print("sizes:", {row["name"]: row["bytes"] for row in rows})
EOF

cp "$REPO_ROOT/scripts/dev/webview_audio_probe.html" probe.html
swiftc -O "$REPO_ROOT/scripts/dev/webview_audio_probe.swift" -o webview_probe
./webview_probe "$OUT" | tee result.json
