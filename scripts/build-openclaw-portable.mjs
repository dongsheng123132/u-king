#!/usr/bin/env node
/** Build a credential-free Windows x64 OpenClaw portable ZIP from explicit inputs. */
import { createHash } from "node:crypto";
import { cp, mkdir, readFile, readdir, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import { execFileSync } from "node:child_process";

const arg = (name) => {
  const i = process.argv.indexOf(name);
  return i < 0 ? null : process.argv[i + 1] ?? null;
};
const out = arg("--out"), exe = arg("--exe"), cache = arg("--runtime-cache"), version = arg("--version");
if (!out || !exe || !cache || !version) {
  throw new Error("usage: node scripts/build-openclaw-portable.mjs --out <dir> --exe <U-King.exe> --runtime-cache <verified runtime> --version <version>");
}
if (!/^[0-9]+\.[0-9]+\.[0-9]+-portable\.[0-9]+$/.test(version)) throw new Error("version must be an explicit portable prerelease, e.g. 1.3.1-portable.1");
const root = path.resolve(out, `U-King-OpenClaw-Portable-${version}-win-x64`);
if (path.dirname(root) !== path.resolve(out)) throw new Error("portable output escapes requested directory");
const oc = path.join(root, "U-King", "OpenClaw");
const sha = async (file) => createHash("sha256").update(await readFile(file)).digest("hex");
const installed = JSON.parse(await readFile(path.join(cache, "installed.json"), "utf8"));
const nodeArchive = path.join(cache, "node-v24.15.0-win-x64.zip");
const ocArchive = path.join(cache, "openclaw-2026.8.1.tgz");
if (installed.node_sha256 !== await sha(nodeArchive)) throw new Error("runtime cache Node archive hash does not match its installed manifest");
const integrity = createHash("sha512").update(await readFile(ocArchive)).digest("base64");
if (installed.openclaw_integrity !== `sha512-${integrity}` || installed.openclaw_version !== "2026.8.1") throw new Error("runtime cache OpenClaw archive integrity/version does not match its installed manifest");
try { await stat(root); throw new Error(`refusing to overwrite existing release directory: ${root}`); } catch (error) { if (error?.code !== "ENOENT") throw error; }
await mkdir(path.join(root, "LICENSES"), { recursive: true });
await cp(exe, path.join(root, "U-King.exe"));
await cp(path.join(cache, "node"), path.join(oc, "runtime", "node"), { recursive: true, dereference: false, filter: (src) => !src.endsWith(".uking-openclaw2-node-runtime.json") });
await cp(path.join(cache, "app"), path.join(oc, "runtime", "app"), { recursive: true, dereference: false });
for (const file of ["node-v24.15.0-win-x64.zip", "openclaw-2026.8.1.tgz", "installed.json"]) {
  await cp(path.join(cache, file), path.join(oc, "runtime", file));
}
await mkdir(path.join(oc, "state"), { recursive: true });
await mkdir(path.join(oc, "workspace"), { recursive: true });
await mkdir(path.join(oc, "run"), { recursive: true });
await mkdir(path.join(oc, "logs"), { recursive: true });
await mkdir(path.join(root, "U-King", "data", "uking"), { recursive: true });
await writeFile(path.join(root, "portable.json"), `${JSON.stringify({ schema_version: 1, owner: "u-king-openclaw-portable", runtime_id: "openclaw2" }, null, 2)}\n`);

const dist = path.join(oc, "runtime", "app", "node_modules", "openclaw", "dist");
const io = (await readdir(dist)).find((name) => /^io\.write-.*\.js$/.test(name));
if (!io) throw new Error("pinned OpenClaw io.write bundle not found");
const ioPath = path.join(dist, io);
const before = await sha(ioPath);
const after = await sha(ioPath);
await writeFile(path.join(root, "PATCHES.md"), `# OpenClaw portable runtime\n\nNo runtime source patch is applied. OpenClaw 2026.8.1 and its locked @openclaw/fs-safe 0.5.6 are copied from the verified input archives. The package manifest records the copied config-writer hash.\n`);
await cp(path.join(oc, "runtime", "app", "node_modules", "openclaw", "LICENSE"), path.join(root, "LICENSES", "OpenClaw-MIT.txt"));
await writeFile(path.join(root, "LICENSES", "U-King-NOTICE.txt"), "U-King source: https://github.com/dongsheng123132/u-king\nOpenClaw: MIT; see OpenClaw-MIT.txt.\n");
await writeFile(path.join(root, "启动 OpenClaw.cmd"), "@echo off\r\nsetlocal\r\n\"%~dp0U-King.exe\" action run runtime.openclaw2.launch --json --no-input\r\nendlocal\r\n");
await writeFile(path.join(root, "README.txt"), "U-King OpenClaw 绿色预览包。双击 U-King.exe 后，在“我的 AI”完成配置、充值、启动和进入。退出 OpenClaw 后再拔盘。首次包不含账户、密钥或历史数据。\r\n");

async function files(dir, base = dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const result = [];
  for (const e of entries) {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) result.push(...await files(full, base));
    else if (e.isFile()) result.push([path.relative(base, full).replaceAll("\\", "/"), full]);
    else throw new Error(`refusing non-file package entry: ${full}`);
  }
  return result;
}
const manifest = { schema_version: 1, version, root_name: path.basename(root), openclaw: { version: "2026.8.1", fs_safe: "0.5.6", config_writer: `dist/${io}`, sha256: before, patched: false }, files: (await files(root)).length };
await writeFile(path.join(root, "runtime-manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
const entries = await files(root);
const sums = await Promise.all(entries.filter(([rel]) => rel !== "SHA256SUMS.txt").map(async ([rel, full]) => `${await sha(full)}  ${rel}`));
await writeFile(path.join(root, "SHA256SUMS.txt"), `${sums.sort().join("\n")}\n`);
const zip = `${root}.zip`;
try { await stat(zip); throw new Error(`refusing to overwrite existing release ZIP: ${zip}`); } catch (error) { if (error?.code !== "ENOENT") throw error; }
execFileSync("C:\\Windows\\System32\\tar.exe", ["-a", "-c", "-f", zip, path.basename(root)], { cwd: out, stdio: "inherit" });
console.log(JSON.stringify({ ok: true, root, zip, sha256: await sha(zip) }));
