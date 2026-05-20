# Release Process

This checklist publishes an EchoNote release from a clean `main` branch.

## 1. Prepare

Confirm the version is consistent in:

- `plugin/manifest.json`
- `plugin/package.json`
- `asr-service/pyproject.toml`
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
npm run build
npm run package
```

Run ASR service tests:

```bash
cd asr-service
.venv/bin/python -m unittest discover -s tests
```

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
- `dist/echonote/README.md`

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
  dist/echonote/README.md \
  --repo rebill/EchoNote \
  --target release/vX.Y.Z \
  --title "EchoNote vX.Y.Z" \
  --notes-file CHANGELOG.md
```

For a more concise release body, copy only the matching version section from `CHANGELOG.md`.

## 5. Verify GitHub

Confirm the release, tag, branch, and assets:

```bash
gh release view vX.Y.Z --repo rebill/EchoNote
git ls-remote --heads origin release/vX.Y.Z
git ls-remote --tags origin vX.Y.Z
```

For Obsidian plugin updates, confirm `versions.json` exists in the repository root and maps the released version to the minimum supported Obsidian version.
