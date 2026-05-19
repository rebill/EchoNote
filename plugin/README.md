# EchoNote Obsidian Plugin

This directory contains the Obsidian TypeScript plugin.

## Install

```bash
npm install
```

## Build

```bash
npm run build
```

Build output:

```text
main.js
manifest.json
```

## Type Check

```bash
npm run typecheck
```

## Manual Vault Install

```bash
mkdir -p "/path/to/Vault/.obsidian/plugins/echonote"
cp main.js manifest.json "/path/to/Vault/.obsidian/plugins/echonote/"
```

Reload Obsidian or disable/enable EchoNote after copying.
