import { rmSync, mkdirSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { build } from "esbuild";

const outdir = join(process.cwd(), ".test-dist");
rmSync(outdir, { recursive: true, force: true });
mkdirSync(outdir, { recursive: true });

const testFiles = readdirSync(join(process.cwd(), "tests"), { withFileTypes: true })
  .filter((entry) => entry.isFile() && entry.name.endsWith(".test.ts"))
  .map((entry) => `tests/${entry.name}`)
  .sort();

if (testFiles.length === 0) {
  throw new Error("No plugin unit tests were found.");
}

for (const testFile of testFiles) {
  await build({
    entryPoints: [testFile],
    outfile: join(outdir, testFile.replace(/^tests\//, "").replace(/\.ts$/, ".cjs")),
    bundle: true,
    platform: "node",
    format: "cjs",
    target: "node20",
    external: ["obsidian"]
  });
}

const testOutputs = testFiles.map((testFile) =>
  join(outdir, testFile.replace(/^tests\//, "").replace(/\.ts$/, ".cjs"))
);

const result = spawnSync("node", ["--test", ...testOutputs], { stdio: "inherit" });
process.exit(result.status ?? 1);
