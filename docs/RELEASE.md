# Release Process

This checklist publishes an EchoNote release from a clean `main` branch.

## 1. Prepare

Confirm the version is consistent in:

- `plugin/manifest.json`
- `plugin/package.json`
- `asr-service/pyproject.toml`
- `companion/package.json`, when the desktop app is included
- `companion/src-tauri/Cargo.toml`, when the desktop app is included
- `versions.json`
- `CHANGELOG.md`

Confirm the working tree is clean:

```bash
git status --short --branch
```

## 2. Verify

Run plugin checks:

```bash
cd plugin
npm run typecheck
npm test
npm run build
npm run package
```

Run ASR service tests:

```bash
cd asr-service
.venv/bin/python -m unittest discover -s tests
```

When the Hugging Face token and cached community-1 model are available, also run the real diarization smoke test from the repository root:

```bash
asr-service/.venv/bin/python scripts/v0_4_0_real_diarization_smoke.py
```

For Companion releases, run Companion checks from `companion/`:

```bash
npm install
npm run typecheck
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri:build
```

Run the fake-backend smoke test from the repository root:

```bash
node scripts/v0_2_0_fake_backend_smoke.mjs
```

This verifies the setup API fallback contract, ASR fake backend health/model/transcription, desktop discovery shape, Companion-only plugin runtime resolution, and legacy Manual settings migration away from plugin-managed ASR.

Companion is source-only. Record that decision in the release notes and skip attaching `.app` or `.dmg` artifacts unless a signed and verified binary is produced in a later release.

## 3. Package

`npm run package` writes the plugin bundle to:

```text
dist/echonote/
```

Create a release archive from the repository root:

```bash
cd dist
zip -r echonote-vX.Y.Z.zip echonote
```

The release should include:

- `dist/echonote-vX.Y.Z.zip`
- `dist/echonote/main.js`
- `dist/echonote/manifest.json`
- `dist/echonote/styles.css`
- `dist/echonote/README.md`

For Companion, use the source-only artifact decision before creating the GitHub release:

| Decision | Release assets |
| --- | --- |
| Source-only | No Companion binary asset. Release notes must say users build Companion from source and still need their own Python ASR environment. |
| `.app` | Attach the macOS `.app` archive and document any unsigned-app Gatekeeper steps. |
| `.dmg` | Attach the macOS `.dmg` and document whether it is signed and notarized. |

Default to source-only. Do not attach an ad-hoc signed `.app`; `spctl` must pass before a binary Companion artifact is considered release-ready.

## 4. Branch And Tag

Create a release branch:

```bash
git switch -c release/vX.Y.Z
git push -u origin release/vX.Y.Z
```

Create the GitHub release:

```bash
gh release create vX.Y.Z \
  dist/echonote-vX.Y.Z.zip \
  dist/echonote/main.js \
  dist/echonote/manifest.json \
  dist/echonote/styles.css \
  dist/echonote/README.md \
  --repo rebill/EchoNote \
  --target release/vX.Y.Z \
  --title "EchoNote vX.Y.Z" \
  --notes-file CHANGELOG.md
```

For a more concise release body, copy only the matching version section from `CHANGELOG.md`.

For Companion releases, the release body must also state:

- That the Obsidian plugin uses Companion as its only ASR runtime.
- Whether Companion is source-only, `.app`, or `.dmg`.
- Where Companion writes discovery and logs.
- That users must run EchoNote and click `Set Up EchoNote` or `Start Service` before using ASR in Obsidian.

## 5. Verify GitHub

Confirm the release, tag, branch, and assets:

```bash
gh release view vX.Y.Z --repo rebill/EchoNote
git ls-remote --heads origin release/vX.Y.Z
git ls-remote --tags origin vX.Y.Z
```

For Obsidian plugin updates, confirm `versions.json` exists in the repository root and maps the released version to the minimum supported Obsidian version.
