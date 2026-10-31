# /// script
# requires-python = ">=3.10"
# dependencies = ["numpy>=1.26", "soundfile>=0.12"]
# ///
"""Analyze a guided capture-probe run (M0 Spike B / C).

A guided run (PROBE_GUIDE=1) is split into thirds: speaker playback only, local speech only,
both. This script aligns mic.wav and sys.wav on the shared clock, writes 16 kHz copies for
`echonote-bench live`, and reports per-phase levels and the speaker-to-mic echo path.

Usage: uv run scripts/dev/analyze_capture_probe.py <probe_dir> [--margin 2.0]
Output: <probe_dir>/{mic16.wav,sys16.wav,analysis.json}
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np
import soundfile as sf

SR = 16_000
FRAME = 320  # 20 ms
PHASES = ("playback_only", "speech_only", "both")


def resample_to_16k(samples: np.ndarray, rate: int) -> np.ndarray:
    if rate == SR:
        return samples.astype(np.float32)
    if rate % SR:
        raise SystemExit(f"unsupported rate {rate}")
    factor = rate // SR
    taps = np.arange(-96, 97)
    lowpass = np.sinc(taps / factor) / factor * np.blackman(len(taps))
    return np.convolve(samples, lowpass, mode="same")[::factor].astype(np.float32)


def dbfs(samples: np.ndarray) -> float:
    return round(float(10 * np.log10(np.mean(samples.astype(np.float64) ** 2) + 1e-12)), 1)


def log_energy(samples: np.ndarray) -> np.ndarray:
    frames = len(samples) // FRAME
    power = np.mean(samples[: frames * FRAME].reshape(frames, FRAME).astype(np.float64) ** 2, axis=1)
    return 10 * np.log10(power + 1e-10)


def best_envelope_corr(mic: np.ndarray, sys: np.ndarray, max_lag_frames: int = 10) -> tuple[float, int]:
    """Same feature as the bench dedup: mic log energy vs sys lagged 0..200 ms."""
    a, b = log_energy(mic), log_energy(sys)
    best = (-1.0, 0)
    for lag in range(max_lag_frames + 1):
        x, y = a[lag:], b[: len(b) - lag] if lag else b
        n = min(len(x), len(y))
        if n < 10 or np.std(x[:n]) == 0 or np.std(y[:n]) == 0:
            continue
        corr = float(np.corrcoef(x[:n], y[:n])[0, 1])
        best = max(best, (corr, lag))
    return round(best[0], 3), best[1] * 20


def echo_path(mic: np.ndarray, sys: np.ndarray, max_lag_s: float = 0.5) -> dict:
    """Waveform cross-correlation peak and the residual after a least-squares FIR echo model."""
    n = len(mic)
    size = 1 << int(np.ceil(np.log2(2 * n)))
    xcorr = np.fft.irfft(np.fft.rfft(mic, size) * np.conj(np.fft.rfft(sys, size)), size)
    max_lag = int(max_lag_s * SR)
    lags = np.concatenate([xcorr[: max_lag + 1], xcorr[-max_lag:]])
    lag_values = np.concatenate([np.arange(max_lag + 1), np.arange(-max_lag, 0)])
    peak = int(np.argmax(np.abs(lags)))
    norm = np.sqrt(np.sum(mic.astype(np.float64) ** 2) * np.sum(sys.astype(np.float64) ** 2)) + 1e-12
    delay = int(lag_values[peak])

    # 128 ms Wiener FIR starting 16 ms before the peak: an upper bound on what a linear echo
    # canceller with that tail length removes.
    taps = 2048
    # Arrival-time alignment is only accurate to a few buffers, so the peak may be at a negative lag.
    start = delay - 256
    sys64 = sys.astype(np.float64)
    if start >= 0:
        x = np.concatenate([np.zeros(start), sys64[: n - start]])
    else:
        x = np.concatenate([sys64[-start:], np.zeros(-start)])
    d = mic.astype(np.float64)
    spectrum = np.fft.rfft(x, size)
    autocorr = np.fft.irfft(spectrum * np.conj(spectrum), size)[:taps]
    cross = np.fft.irfft(np.fft.rfft(d, size) * np.conj(spectrum), size)[:taps]
    index = np.arange(taps)
    toeplitz = autocorr[np.abs(index[:, None] - index[None, :])] + 1e-6 * autocorr[0] * np.eye(taps)
    weights = np.linalg.solve(toeplitz, cross)
    residual = d - np.convolve(x, weights)[:n]
    return {
        "delay_ms": round(delay / SR * 1000, 1),
        "xcorr_peak_norm": round(float(np.abs(lags[peak]) / norm), 3),
        "linear_erle_db": round(float(10 * np.log10(np.mean(d**2) / (np.mean(residual**2) + 1e-12))), 1),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("probe_dir", type=Path)
    parser.add_argument("--margin", type=float, default=2.0, help="seconds trimmed at phase edges")
    args = parser.parse_args()

    report = json.loads((args.probe_dir / "report.json").read_text())
    # WASAPI loopback sends nothing while the output is silent; concatenated frames then run
    # ahead of the wall clock and alignment after the first gap is invalid.
    missing_s = {name: round(report[name]["wall_duration_s"] - report[name]["frames_duration_s"], 3) for name in ("mic", "sys")}
    for name, missing in missing_s.items():
        if missing > 0.1:
            print(f"warning: {name} delivered {missing:.2f}s fewer frames than wall time; alignment after its first gap is invalid")
    tracks = {}
    for name in ("mic", "sys"):
        samples, rate = sf.read(args.probe_dir / f"{name}.wav", dtype="float32")
        if samples.ndim > 1:
            samples = samples.mean(axis=1)
        # First callback arrives one period after its first sample was captured.
        first_sample_s = (report[name]["startup_ms"] - report[name]["mean_period_ms"]) / 1000
        tracks[name] = (resample_to_16k(samples, rate), first_sample_s)

    # Put both tracks on the probe clock: sample 0 is t0 of the run.
    aligned = {}
    for name, (samples, first_s) in tracks.items():
        aligned[name] = np.concatenate([np.zeros(int(round(first_s * SR)), np.float32), samples])
    length = min(len(track) for track in aligned.values())
    for name in aligned:
        aligned[name] = aligned[name][:length]
        sf.write(args.probe_dir / f"{name}16.wav", aligned[name], SR, subtype="PCM_16")

    third = report["seconds"] / 3
    phases = {}
    for index, phase in enumerate(PHASES):
        start = int((index * third + args.margin) * SR)
        end = min(int(((index + 1) * third - args.margin) * SR), length)
        mic, sys = aligned["mic"][start:end], aligned["sys"][start:end]
        corr, corr_lag_ms = best_envelope_corr(mic, sys)
        entry = {
            "window_s": [round(start / SR, 1), round(end / SR, 1)],
            "mic_dbfs": dbfs(mic),
            "sys_dbfs": dbfs(sys),
            "mic_floor_dbfs": round(float(np.percentile(log_energy(mic), 10)), 1),
            "envelope_corr": corr,
            "envelope_corr_lag_ms": corr_lag_ms,
        }
        if phase == "playback_only" and dbfs(sys) > -50:
            entry["echo"] = echo_path(mic, sys)
            entry["speaker_to_mic_db"] = round(entry["mic_dbfs"] - entry["sys_dbfs"], 1)
        phases[phase] = entry

    analysis = {"aligned_s": round(length / SR, 2), "relative_drift_ppm": report["relative_drift_ppm"], "phases": phases}
    (args.probe_dir / "analysis.json").write_text(json.dumps(analysis, indent=2) + "\n")
    print(json.dumps(analysis, indent=2))


if __name__ == "__main__":
    main()
