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
const out = arg("--out"), exe = arg("--exe"), cache = arg("--runtime-cache"), compat = arg("--fs-safe-compat"), compatLicense = arg("--fs-safe-license"), version = arg("--version");
if (!out || !exe || !cache || !compat || !compatLicense || !version) {
  throw new Error("usage: node scripts/build-openclaw-portable.mjs --out <dir> --exe <U-King.exe> --runtime-cache <verified runtime> --fs-safe-compat <audited fs-safe 0.8.2 dist> --fs-safe-license <MIT license file> --version <version>");
}
if (!/^[0-9]+\.[0-9]+\.[0-9]+-portable\.[0-9]+$/.test(version)) throw new Error("version must be an explicit portable prerelease, e.g. 1.3.1-portable.1");
const root = path.resolve(out, `U-King-OpenClaw-Portable-${version}-win-x64`);
if (path.dirname(root) !== path.resolve(out)) throw new Error("portable output escapes requested directory");
const oc = path.join(root, "U-King", "OpenClaw");
const sha = async (file) => createHash("sha256").update(await readFile(file)).digest("hex");
const compatRoot = path.resolve(compat);
if ((await stat(compatRoot)).isDirectory() === false) throw new Error("fs-safe compatibility input must be a directory");
const pinned = JSON.parse(await readFile(new URL("../src-tauri/resources/openclaw2-runtime.json", import.meta.url), "utf8"));
const digestTree = async (dir) => {
  const rows = [];
  const visit = async (base, current) => {
    for (const entry of await readdir(current, { withFileTypes: true })) {
      const full = path.join(current, entry.name);
      if (entry.isDirectory()) await visit(base, full);
      else if (entry.isFile()) rows.push(`${path.relative(base, full).replaceAll("\\", "/")}\0${await sha(full)}\n`);
      else throw new Error(`refusing non-file fs-safe sidecar entry: ${full}`);
    }
  };
  await visit(dir, dir);
  return createHash("sha256").update(rows.sort().join("")).digest("hex");
};
const installed = JSON.parse(await readFile(path.join(cache, "installed.json"), "utf8"));
const nodeArchive = path.join(cache, "node-v24.15.0-win-x64.zip");
const ocArchive = path.join(cache, "openclaw-2026.8.1.tgz");
if (await sha(nodeArchive) !== pinned.node.windows_x64_sha256 || installed.node_sha256 !== pinned.node.windows_x64_sha256) throw new Error("runtime cache Node archive does not match the source-pinned runtime manifest");
const integrity = createHash("sha512").update(await readFile(ocArchive)).digest("base64");
if (`sha512-${integrity}` !== pinned.openclaw.integrity || installed.openclaw_integrity !== pinned.openclaw.integrity || installed.openclaw_version !== pinned.openclaw_version) throw new Error("runtime cache OpenClaw archive does not match the source-pinned runtime manifest");
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
const configWriterHash = await sha(ioPath);
const workspace = (await readdir(dist)).find((name) => /^workspace-fs-.*\.js$/.test(name));
if (!workspace) throw new Error("pinned OpenClaw workspace bundle not found");
const workspacePath = path.join(dist, workspace);
const workspaceBefore = await readFile(workspacePath, "utf8");
if (createHash("sha256").update(workspaceBefore).digest("hex") !== "ea0436681164e0dce0d1be6ba57e5bc17a262d412b562ecac98e77a648d8712f") {
  throw new Error("unexpected OpenClaw workspace bundle hash; refusing compatibility patch");
}
const functionNeedle = 'async function updateWorkspaceFile(rootDir, browserPath, content, expectedHash) {\n\tconst workspaceRoot = await openWorkspaceRoot(rootDir);';
const strictNeedle = 'renameIdentity: "strict"';
if (workspaceBefore.split(functionNeedle).length !== 2 || workspaceBefore.split(strictNeedle).length !== 2) {
  throw new Error("unexpected OpenClaw workspace writer shape; refusing compatibility patch");
}
const sidecarDir = path.join(dist, "uking-compat", "fs-safe08", "dist");
await cp(compatRoot, sidecarDir, { recursive: true, dereference: false });
const compatRootImpl = path.join(sidecarDir, "root-impl.js");
const compatRootImplHash = await sha(compatRootImpl);
if (compatRootImplHash !== "6f701d377943880178b1478881a613db48e6a949f42a453007301bc61ea9f9fb") {
  throw new Error("audited fs-safe sidecar root-impl.js hash mismatch");
}
const compatTreeHash = await digestTree(compatRoot);
if (compatTreeHash !== "02761bf6d5c75c3be9d0fea1897eeb81ee884700be373c2ceef4b34c61712e7c") {
  throw new Error("audited fs-safe sidecar tree hash mismatch");
}
const workspaceAfter = [
  'import { root as ukingPortableRoot } from "./uking-compat/fs-safe08/dist/root.js";',
  'import { configureFsSafeNative as ukingPortableNative } from "./uking-compat/fs-safe08/dist/config.js";',
  'ukingPortableNative({ mode: "off" });',
  workspaceBefore
    .replace(functionNeedle, 'async function updateWorkspaceFile(rootDir, browserPath, content, expectedHash) {\n\tconst ukingPortableCompat = process.platform === "win32" && process.env.UKING_PORTABLE_COMPAT_EXFAT === "1";\n\tconst workspaceRoot = ukingPortableCompat ? await ukingPortableRoot(rootDir, { hardlinks: "reject", maxBytes: WORKSPACE_PREVIEW_MAX_BYTES, nonBlockingRead: true, symlinks: "reject" }).catch(() => undefined) : await openWorkspaceRoot(rootDir);')
    .replace(strictNeedle, 'renameIdentity: ukingPortableCompat ? "verify-content-with-lock" : "strict"'),
].join("\n");
await writeFile(workspacePath, workspaceAfter);
const workspaceHash = await sha(workspacePath);
await writeFile(path.join(root, "PATCHES.md"), `# OpenClaw portable runtime\n\nOpenClaw 2026.8.1 and its locked @openclaw/fs-safe 0.5.6 are copied from the verified input archives. Only its workspace writer is patched at build time: on Windows with the explicit package-owned \`UKING_PORTABLE_COMPAT_EXFAT=1\` switch it imports the bundled fs-safe 0.8.2 sidecar and requests \`verify-content-with-lock\`; every other path retains upstream strict behavior. The sidecar is source revision 524e2a2dd50c390f924a0360c6c71ddf74f70f42 plus patches/fs-safe08-windows-compat.patch.\n`);
await cp(path.join(oc, "runtime", "app", "node_modules", "openclaw", "LICENSE"), path.join(root, "LICENSES", "OpenClaw-MIT.txt"));
await cp(compatLicense, path.join(root, "LICENSES", "fs-safe-MIT.txt"));
await writeFile(path.join(root, "LICENSES", "U-King-NOTICE.txt"), "U-King source: https://github.com/dongsheng123132/u-king\nOpenClaw: MIT; see OpenClaw-MIT.txt.\nfs-safe compatibility sidecar: MIT; see fs-safe-MIT.txt.\n");
await writeFile(path.join(root, "启动 OpenClaw.cmd"), "@echo off\r\nsetlocal\r\n\"%~dp0U-King.exe\" action run runtime.openclaw2.launch --yes --json --no-input\r\nendlocal\r\n");
await writeFile(path.join(root, "README.txt"), "U-King OpenClaw 绿色预览包。双击 U-King.exe 后，使用专用页面完成一键配置、充值、启动和进入。充值只在系统浏览器打开页面，不会自动支付。首次包不含账户、密钥或历史数据。\r\n");

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
const manifest = { schema_version: 1, version, root_name: path.basename(root), openclaw: { version: pinned.openclaw_version, fs_safe: "0.5.6", config_writer: `dist/${io}`, sha256: configWriterHash, workspace_writer: `dist/${workspace}`, workspace_before_sha256: createHash("sha256").update(workspaceBefore).digest("hex"), workspace_after_sha256: workspaceHash, patched: true }, fs_safe_compat: { version: "0.8.2", source_revision: "524e2a2dd50c390f924a0360c6c71ddf74f70f42", tree_sha256: compatTreeHash, root_impl_sha256: compatRootImplHash, native_mode: "off" }, files: (await files(root)).length };
await writeFile(path.join(root, "runtime-manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
const entries = await files(root);
const sums = await Promise.all(entries.filter(([rel]) => rel !== "SHA256SUMS.txt").map(async ([rel, full]) => `${await sha(full)}  ${rel}`));
await writeFile(path.join(root, "SHA256SUMS.txt"), `${sums.sort().join("\n")}\n`);
const zip = `${root}.zip`;
try { await stat(zip); throw new Error(`refusing to overwrite existing release ZIP: ${zip}`); } catch (error) { if (error?.code !== "ENOENT") throw error; }
execFileSync("C:\\Windows\\System32\\tar.exe", ["-a", "-c", "-f", zip, path.basename(root)], { cwd: out, stdio: "inherit" });
console.log(JSON.stringify({ ok: true, root, zip, sha256: await sha(zip) }));
