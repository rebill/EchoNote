# EchoNote v0.4.0 QA Notes

## Automated Verification

Run on 2026-06-01:

```bash
asr-service/.venv/bin/python -m unittest discover asr-service/tests
npm run typecheck --prefix plugin
npm test --prefix plugin
npm run build --prefix plugin
npm run typecheck --prefix companion
npm run build --prefix companion
cargo test --manifest-path companion/src-tauri/Cargo.toml
node scripts/v0_2_0_fake_backend_smoke.mjs
npm run tauri:build --prefix companion
```

Covered:

- Adaptive chunking cuts on silence after minimum duration.
- Adaptive chunking force-cuts continuous speech at 15 seconds.
- `/transcribe` returns `text` and `turns`.
- `/transcript/finalize` returns no-speaker turns when diarization is disabled or unavailable.
- Speaker assignment and conservative merge logic.
- Companion discovery remains backward-compatible and accepts v0.4 capabilities.
- Companion redacts API keys, bearer tokens, Hugging Face env tokens, and raw `hf_` tokens in diagnostics/log lines.
- Fake-backend end-to-end service and plugin discovery smoke path.

Re-run on 2026-06-01 after the pyannote access retry: all automated verification commands above still pass.

Additional local preparation completed:

```bash
asr-service/.venv/bin/pip install -e 'asr-service[diarization]'
asr-service/.venv/bin/python scripts/v0_4_0_real_diarization_smoke.py
```

Result:

- `pyannote.audio` installs and imports successfully in `asr-service/.venv`.
- The smoke script generates a local two-speaker TTS WAV and calls the real `/transcript/finalize` endpoint.
- After Hugging Face access was granted, `pyannote/speaker-diarization-community-1` loads successfully.
- The ASR adapter handles the community pipeline `DiarizeOutput` shape via `exclusive_speaker_diarization`.
- The real smoke test returned `diarization_status=available`, two speakers, and turn speakers `Speaker 1` / `Speaker 2`.

## Release Packaging

Run on 2026-06-02:

```bash
asr-service/.venv/bin/python -m unittest discover asr-service/tests
npm run typecheck --prefix plugin
npm test --prefix plugin
npm run typecheck --prefix companion
cargo test --manifest-path companion/src-tauri/Cargo.toml
node scripts/v0_2_0_fake_backend_smoke.mjs
asr-service/.venv/bin/python scripts/v0_4_0_real_diarization_smoke.py
npm run package --prefix plugin
npm run tauri:build --prefix companion
```

Artifacts:

- `dist/echonote-v0.4.0.zip`
- `dist/EchoNote-v0.4.0-macos.zip`
