# Contributing

EchoNote contains two independently runnable parts:

- `plugin/`: the Obsidian desktop plugin.
- `asr-service/`: the local Python ASR service.

## Requirements

- macOS for end-to-end plugin and local audio testing.
- Node.js for the Obsidian plugin.
- Python 3.11+ for the ASR service.
- Obsidian Desktop for manual plugin verification.

## Setup

Install plugin dependencies:

```bash
cd plugin
npm install
```

Install ASR service dependencies:

```bash
python3 -m venv asr-service/.venv
. asr-service/.venv/bin/activate
pip install --upgrade pip
pip install -e 'asr-service[mlx]'
```

Use `pip install -e asr-service` if you only need the fake backend.

## Verification

Run these checks before opening a pull request:

```bash
cd plugin
npm run typecheck
npm run build
```

```bash
cd asr-service
.venv/bin/python -m unittest discover -s tests
```

For release packaging:

```bash
cd plugin
npm run package
```

## Manual Plugin Test

Build the plugin, then copy `plugin/main.js`, `plugin/manifest.json`, and `plugin/styles.css` into a test vault:

```bash
mkdir -p "/path/to/TestVault/.obsidian/plugins/echonote"
cp plugin/main.js plugin/manifest.json plugin/styles.css "/path/to/TestVault/.obsidian/plugins/echonote/"
```

Reload Obsidian or disable and re-enable EchoNote.

## Pull Requests

Keep pull requests focused. Include:

- A short description of the behavioral change.
- Verification commands and results.
- Screenshots or transcript excerpts for UI/audio workflow changes when useful.
- Release note impact if the change should be visible in `CHANGELOG.md`.

Do not commit local virtual environments, `node_modules`, `dist`, generated `plugin/main.js`, or vault-local test data.
