#!/bin/sh
# Soak run for PRD A6 (a 3-hour meeting): the app, optimized and with the debug switches on
# (cargo profile `soak`), records the synthetic meeting-zh-dual set looped in real time
# through the real UI in a scratch library. The UI scrolls the live list every minute and
# times the frames, then stops and times until everything is stored. This script samples the
# app process's memory and CPU every 30 s and prints a summary. Run it when the computer is
# otherwise idle, and keep the EchoNote window visible: WebKit paints nothing in a hidden or
# covered window, and the scroll check then reports "not measured". Nothing leaves this
# computer, and the user's library is not touched. Memory is the app process only (the
# WebView's helper processes are not counted) and includes about 77 MB of fake-input samples.
#
# Usage: scripts/dev/soak.sh [seconds]   (default 10800, 3 hours)
# Needs: $ECHONOTE_DEV_DATA/eval/meeting-zh-dual and models (scripts/dev/fetch_assets.py,
#        scripts/dev/build_eval_sets.py).
# Output: $ECHONOTE_DEV_DATA/results/soak-<timestamp>/{report.json,samples.csv,summary.json}
set -eu

SECONDS_TO_RUN="${1:-10800}"
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DATA_DIR="${ECHONOTE_DEV_DATA:-$HOME/.cache/echonote-dev}"
EVAL="$DATA_DIR/eval/meeting-zh-dual"
OUT="$DATA_DIR/results/soak-$(date +%Y%m%d-%H%M%S)"
export SHERPA_ONNX_ARCHIVE_DIR="${SHERPA_ONNX_ARCHIVE_DIR:-$DATA_DIR/sherpa-onnx}"
mkdir -p "$OUT/input" "$OUT/library"
ln -s "$EVAL/mic_bleed.wav" "$OUT/input/mic.wav"
ln -s "$EVAL/sys.wav" "$OUT/input/sys.wav"

(cd "$REPO_ROOT/app" && pnpm build >/dev/null)
cargo build --quiet --manifest-path "$REPO_ROOT/Cargo.toml" -p echonote-app --profile soak --features custom-protocol

ECHONOTE_DATA_DIR="$OUT/library" ECHONOTE_FAKE_INPUT="$OUT/input" ECHONOTE_SMOKE="$OUT/report.json" \
  ECHONOTE_SOAK_SECONDS="$SECONDS_TO_RUN" \
  "$REPO_ROOT/target/soak/echonote-app" >"$OUT/stdout.log" 2>"$OUT/stderr.log" &
pid=$!
started=$(date +%s)
limit=$((SECONDS_TO_RUN + 600))
echo "elapsed_s,rss_kb,cpu_time" >"$OUT/samples.csv"
while kill -0 "$pid" 2>/dev/null; do
  elapsed=$(($(date +%s) - started))
  if [ "$elapsed" -gt "$limit" ]; then
    echo "soak run timed out after ${limit}s" >&2
    kill "$pid"
    exit 1
  fi
  line=$(ps -o rss=,time= -p "$pid" 2>/dev/null || true)
  [ -n "$line" ] && echo "$elapsed,$(echo "$line" | awk '{print $1 "," $2}')" >>"$OUT/samples.csv"
  sleep 30
done

if [ ! -s "$OUT/report.json" ]; then
  echo "no report: the app exited before the soak finished; see $OUT" >&2
  exit 1
fi
python3 - "$OUT" <<'PY'
import csv, json, pathlib, statistics, sys
out = pathlib.Path(sys.argv[1])
report = json.loads((out / "report.json").read_text())

def seconds(cpu: str) -> float:
    parts = [float(part) for part in cpu.replace("-", ":").split(":")]
    total = 0.0
    for part in parts:
        total = total * 60 + part
    return total

rows = [(int(r["elapsed_s"]), int(r["rss_kb"]) / 1024, seconds(r["cpu_time"]))
        for r in csv.DictReader((out / "samples.csv").open())]
# The first sample can catch the process before it has loaded anything.
rows = [row for row in rows if row[1] >= 10]
summary = {"seconds": report.get("seconds"), "error": report.get("error"), "samples": len(rows)}
# Memory after the first 5 minutes (model loaded, buffers grown): medians of the first and
# last tenth, and the least-squares slope. Growth is judged on 30 minutes or more only.
steady = [row for row in rows if row[0] >= 300]
summary["rss_peak_mb"] = round(max((row[1] for row in rows), default=0))
summary["steady_s"] = steady[-1][0] - steady[0][0] if len(steady) >= 2 else 0
if len(steady) >= 4:
    tenth = max(1, len(steady) // 10)
    summary["rss_start_mb"] = round(statistics.median(row[1] for row in steady[:tenth]))
    summary["rss_end_mb"] = round(statistics.median(row[1] for row in steady[-tenth:]))
    xs, ys = [row[0] / 3600 for row in steady], [row[1] for row in steady]
    mx, my = statistics.fmean(xs), statistics.fmean(ys)
    slope = sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / sum((x - mx) ** 2 for x in xs)
    summary["rss_slope_mb_per_h"] = round(slope, 1)
if len(rows) >= 2:
    span = rows[-1][0] - rows[0][0]
    summary["cpu_share_of_one_core"] = round((rows[-1][2] - rows[0][2]) / max(span, 1), 3)
# Frames only count while the window was on screen (a hidden WebView paints nothing).
live = [sample for sample in report.get("live") or [] if sample.get("painted")]
summary["live_scroll_samples"] = len(live)
summary["visibility"] = report.get("visibility")
if live:
    summary["live_scroll_p95_ms_worst"] = max(sample["p95Ms"] for sample in live)
    summary["live_rows_rendered_max"] = max(sample["rows"] for sample in live)
summary["segments"] = report.get("segments")
summary["stop_to_stored_ms"] = report.get("stopToStoredMs")
summary["detail_scroll"] = report.get("detailScroll")
summary["csp_violations"] = report.get("cspViolations")
# A6: memory does not grow, the list scrolls smoothly, stop finishes within 3 s. The growth
# and frame thresholds are this script's reading of "does not grow" and "smooth".
# None means not measured: the window was hidden, or the run was too short.
summary["a6"] = {
    "stop_within_3s": (report.get("stopToStoredMs") or 1e9) <= 3000,
    "scroll_p95_under_50ms": summary["live_scroll_p95_ms_worst"] <= 50 if live else None,
    "memory_growth_under_20mb_per_h": summary["rss_slope_mb_per_h"] <= 20
    if summary["steady_s"] >= 1800 and "rss_slope_mb_per_h" in summary else None,
}
(out / "summary.json").write_text(json.dumps(summary, indent=2, ensure_ascii=False))
print(json.dumps(summary, indent=2, ensure_ascii=False))
PY
