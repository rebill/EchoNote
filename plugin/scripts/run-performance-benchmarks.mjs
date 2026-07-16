import { rmSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { build } from "esbuild";

const outdir = join(process.cwd(), ".benchmark-dist");
const outfile = join(outdir, "performance-benchmark.cjs");
rmSync(outdir, { recursive: true, force: true });
mkdirSync(outdir, { recursive: true });

await build({
  entryPoints: ["benchmarks/performance-benchmark.ts"],
  outfile,
  bundle: true,
  platform: "node",
  format: "cjs",
  target: "node20"
});

const result = spawnSync(process.execPath, ["--expose-gc", outfile], { stdio: "inherit" });
process.exit(result.status ?? 1);
