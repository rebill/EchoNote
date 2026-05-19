import esbuild from "esbuild";

await esbuild.build({
  bundle: true,
  entryPoints: ["src/main.ts"],
  external: ["obsidian"],
  format: "cjs",
  logLevel: "info",
  outfile: "main.js",
  platform: "node",
  target: "es2018"
});
