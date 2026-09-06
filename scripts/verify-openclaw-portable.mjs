#!/usr/bin/env node
import { createHash } from "node:crypto";
import { readFile, readdir, stat } from "node:fs/promises";
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
  const children = await readdir(path.join(root, rel));
  if (children.length) throw new Error(`initial mutable directory is not empty: ${rel}`);
}
async function walk(dir) {
  const out = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const file = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...await walk(file));
    else if (entry.isFile()) out.push(file);
    else throw new Error(`non-regular package entry: ${file}`);
  }
  return out;
}
// Initial packages must not carry a real device/API token. Deliberately scan
// values, rather than just filenames; source examples such as `sk-example`
// are below the minimum credential length and do not mask this check.
for (const file of await walk(root)) {
  if ((await stat(file)).size > 2_000_000) continue;
  const text = await readFile(file, "utf8").catch(() => "");
  if (/\b(?:sk|xp)_[A-Za-z0-9_-]{20,}\b|\bsk-[A-Za-z0-9_-]{20,}\b/.test(text)) {
    const rel = path.relative(root, file).replaceAll("\\", "/");
    // This upstream example intentionally contains a fake Authorization
    // string to test redaction. Pin both its exact path and full file hash;
    // no other dependency file is exempt from credential scanning.
    if (rel === "U-King/OpenClaw/runtime/app/node_modules/@mistralai/mistralai/examples/src/observability/redaction_policies.ts"
      && createHash("sha256").update(await readFile(file)).digest("hex") === "20d241c6200facc29d41635c10a8978babd4f1a9a7db90ee764558e7e3472540") {
      continue;
    }
    throw new Error(`credential-like value found in package: ${path.relative(root, file)}`);
  }
}
console.log(JSON.stringify({ ok: true, root }));
