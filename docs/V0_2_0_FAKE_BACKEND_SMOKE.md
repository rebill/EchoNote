# v0.2.0 Fake Backend Smoke Test

Run this before release to verify the Companion discovery and plugin runtime path without MLX or real audio.

```bash
node scripts/v0_2_0_fake_backend_smoke.mjs
```

The script:

- validates the v0.3.0 setup API fallback response contract and setup fixtures;
- starts the Python ASR service with `--backend fake` on a free localhost port;
- verifies `/health`, `/model/load`, `/model/status`, and `/transcribe`;
- writes a temporary v1 `companion.json`;
- verifies the plugin discovery reader resolves it as `available`;
- verifies the plugin resolves Companion and does not start plugin-managed Python.
- verifies legacy Manual settings do not resolve to a plugin-managed ASR path.

If `asr-service/.venv/bin/python` is unavailable, set:

```bash
ECHONOTE_ASR_PYTHON=/path/to/python node scripts/v0_2_0_fake_backend_smoke.mjs
```

Manual Obsidian smoke path:

1. Open EchoNote and click `Set Up EchoNote`.
2. Confirm EchoNote shows service `Running` and writes `~/Library/Application Support/EchoNote/companion.json`.
3. Open the Obsidian EchoNote status panel and confirm Companion status is `available`.
4. Start a short meeting and stop it after one chunk.
5. Confirm the transcript contains `fake transcript for chunk`.
