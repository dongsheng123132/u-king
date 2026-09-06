#!/usr/bin/env node
import { createHash } from "node:crypto";
import { readFile, stat } from "node:fs/promises";
import path from "node:path";
const root = process.argv[2];
if (!root) throw new Error("usage: node scripts/verify-openclaw-portable.mjs <unpacked-root>");
const sum = await readFile(path.join(root, "SHA256SUMS.txt"), "utf8");
for (const line of sum.trim().split(/\r?\n/)) {
  const [expected, rel] = line.split(/  /, 2);
  const full = path.join(root, ...rel.split("/"));
  if (!(await stat(full)).isFile()) throw new Error(`missing package file: ${rel}`);
  const actual = createHash("sha256").update(await readFile(full)).digest("hex");
  if (actual !== expected) throw new Error(`hash mismatch: ${rel}`);
}
const marker = JSON.parse(await readFile(path.join(root, "portable.json"), "utf8"));
if (marker.owner !== "u-king-openclaw-portable" || marker.runtime_id !== "openclaw2") throw new Error("unsafe portable marker");
for (const rel of ["U-King/OpenClaw/state", "U-King/OpenClaw/workspace", "U-King/OpenClaw/run", "U-King/data/uking"]) {
  const children = await (await import("node:fs/promises")).readdir(path.join(root, rel));
  if (children.length) throw new Error(`initial mutable directory is not empty: ${rel}`);
}
console.log(JSON.stringify({ ok: true, root }));
