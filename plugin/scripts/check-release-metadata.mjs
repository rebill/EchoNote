import { readFileSync } from "node:fs";
import { join } from "node:path";

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

const packageJson = readJson(join(process.cwd(), "package.json"));
const packageLock = readJson(join(process.cwd(), "package-lock.json"));
const manifest = readJson(join(process.cwd(), "manifest.json"));
const versions = readJson(join(process.cwd(), "..", "versions.json"));
const version = packageJson.version;

const checks = [
  ["manifest.json", manifest.version],
  ["package-lock.json", packageLock.version],
  ["package-lock.json root package", packageLock.packages?.[""]?.version]
];

for (const [source, actual] of checks) {
  if (actual !== version) {
    throw new Error(`${source} version ${String(actual)} does not match package.json version ${version}.`);
  }
}

if (versions[version] !== manifest.minAppVersion) {
  throw new Error(
    `versions.json must map ${version} to manifest minAppVersion ${manifest.minAppVersion}.`
  );
}

console.log(`EchoNote plugin release metadata is consistent for v${version}.`);
