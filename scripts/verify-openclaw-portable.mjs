#!/usr/bin/env node
import { createHash } from "node:crypto";
import { readFile, readdir, stat } from "node:fs/promises";
import path from "node:path";
const root = process.argv[2];
if (!root) throw new Error("usage: node scripts/verify-openclaw-portable.mjs <unpacked-root>");
const sum = await readFile(path.join(root, "SHA256SUMS.txt"), "utf8");
const HASH_WORKERS = 8;
async function mapBounded(items, work) {
  const results = new Array(items.length);
  let next = 0;
  const workers = [];
  for (let i = 0; i < Math.min(HASH_WORKERS, items.length); i += 1) {
    workers.push((async () => {
      while (next < items.length) {
        const index = next;
        next += 1;
        results[index] = await work(items[index], index);
      }
    })());
  }
  await Promise.all(workers);
  return results;
}
const listed = new Set();
const hashEntries = [];
for (const line of sum.trim().split(/\r?\n/)) {
  const [expected, rel] = line.split(/  /, 2);
  if (!/^[a-f0-9]{64}$/.test(expected) || !rel || rel.includes("\\") || rel.split("/").some((part) => !part || part === "." || part === "..")) throw new Error(`unsafe hash entry: ${line}`);
  if (listed.has(rel)) throw new Error(`duplicate hash entry: ${rel}`);
  listed.add(rel);
  hashEntries.push({ expected, rel, full: path.join(root, ...rel.split("/")) });
}
const credentialPattern = /\b(?:sk|xp)_[A-Za-z0-9_-]{20,}\b|\bsk-[A-Za-z0-9_-]{20,}\b/;
if (credentialPattern.test(sum)) throw new Error("credential-like value found in package: SHA256SUMS.txt");
await mapBounded(hashEntries, async ({ expected, rel, full }) => {
  const info = await stat(full);
  if (!info.isFile()) throw new Error(`missing package file: ${rel}`);
  // Hash verification and the small-file credential scan share this one read.
  const bytes = await readFile(full);
  const actual = createHash("sha256").update(bytes).digest("hex");
  if (actual !== expected) throw new Error(`hash mismatch: ${rel}`);
  if (info.size <= 2_000_000 && credentialPattern.test(bytes.toString("utf8"))) {
    // This upstream example intentionally contains a fake Authorization
    // string to test redaction. Pin both its exact path and full file hash;
    // no other dependency file is exempt from credential scanning.
    if (rel === "U-King/OpenClaw/runtime/app/node_modules/@mistralai/mistralai/examples/src/observability/redaction_policies.ts"
      && actual === "20d241c6200facc29d41635c10a8978babd4f1a9a7db90ee764558e7e3472540") return;
    throw new Error(`credential-like value found in package: ${rel}`);
  }
});
const marker = JSON.parse(await readFile(path.join(root, "portable.json"), "utf8"));
if (marker.owner !== "u-king-openclaw-portable" || marker.runtime_id !== "openclaw2") throw new Error("unsafe portable marker");
for (const rel of ["U-King/OpenClaw/state", "U-King/OpenClaw/workspace", "U-King/OpenClaw/run", "U-King/OpenClaw/logs", "U-King/data/uking"]) {
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
const packageFiles = await walk(root);
const actual = new Set(packageFiles.map((file) => path.relative(root, file).replaceAll("\\", "/")));
actual.delete("SHA256SUMS.txt");
for (const rel of actual) if (!listed.has(rel)) throw new Error(`package file missing from SHA256SUMS: ${rel}`);
for (const rel of listed) if (!actual.has(rel)) throw new Error(`SHA256SUMS entry missing from package: ${rel}`);
// Initial packages must not carry a real device/API token. Deliberately scan
// values, rather than just filenames; source examples such as `sk-example`
// are below the minimum credential length and do not mask this check.
console.log(JSON.stringify({ ok: true, root }));
