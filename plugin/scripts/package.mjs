import { copyFile, mkdir, rm } from "node:fs/promises";
import { resolve } from "node:path";

const pluginDir = resolve(import.meta.dirname, "..");
const repoRoot = resolve(pluginDir, "..");
const outDir = resolve(repoRoot, "dist", "echonote");

await rm(outDir, { force: true, recursive: true });
await mkdir(outDir, { recursive: true });

await copyFile(resolve(pluginDir, "main.js"), resolve(outDir, "main.js"));
await copyFile(resolve(pluginDir, "manifest.json"), resolve(outDir, "manifest.json"));
await copyFile(resolve(pluginDir, "styles.css"), resolve(outDir, "styles.css"));
await copyFile(resolve(pluginDir, "README.md"), resolve(outDir, "README.md"));

console.log(`Packaged EchoNote plugin to ${outDir}`);
