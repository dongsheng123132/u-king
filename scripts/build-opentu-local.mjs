#!/usr/bin/env node
/** Build the pinned OpenTu browser bundle for packaging; never Docker. */
import { createHash } from "node:crypto";
import { cp, mkdir, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const lock = JSON.parse(await readFile(path.join(root, "third_party", "opentu", "lock.json"), "utf8"));
const source = path.resolve(process.env.OPENTU_SOURCE_DIR || path.join("D:", "CodexWorktrees", "opentu-v1.1.6-source"));
// The customer app downloads this optional package only after an explicit
// click.  Never copy minified OpenTu assets into Tauri resources: only the
// small, reviewed release catalogue is packaged with the executable.
// Keep release artifacts outside the frontend `dist/` tree: Vite may clean
// that directory and Tauri treats it as web assets.
const releaseDir = path.join(root, "outputs", "creator-components");
const bundleId = "opentu-1.1.6-uking.1";
const archivePath = path.join(releaseDir, `${bundleId}.tar.gz`);
const catalogueCandidate = path.join(releaseDir, `${bundleId}.json`);
const patches = path.join(root, "third_party", "opentu", "patches");

function run(command, args, cwd) {
  const r = spawnSync(command, args, { cwd, stdio: "inherit", shell: process.platform === "win32" });
  if (r.status !== 0) throw new Error(`${command} ${args.join(" ")} failed (${r.status})`);
}
function succeeds(command, args, cwd) {
  return spawnSync(command, args, { cwd, stdio: "ignore", shell: process.platform === "win32" }).status === 0;
}
function runPinnedPnpm(args, cwd) {
  // Do not silently consume whatever pnpm happens to be on PATH. Corepack
  // resolves the exact version recorded in lock.json without modifying it.
  run("corepack", [`pnpm@${lock.pnpm}`, ...args], cwd);
}
function sha256(file) { return readFile(file).then((data) => createHash("sha256").update(data).digest("hex")); }
async function listFiles(root, relative = "") {
  const entries = await readdir(path.join(root, relative), { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const child = path.join(relative, entry.name);
    if (entry.isDirectory()) files.push(...await listFiles(root, child));
    else if (entry.isFile()) files.push(child);
  }
  return files;
}
async function writeIntegrityManifest(stage) {
  // This is verified by the U-King loopback host before it serves the canvas.
  // Do not list this manifest itself: its content necessarily changes its hash.
  const files = (await listFiles(stage))
    .filter((relative) => relative !== ".uking-integrity.json")
    .sort();
  const entries = await Promise.all(files.map(async (relative) => ({
    path: relative.replaceAll(path.sep, "/"),
    sha256: await sha256(path.join(stage, relative)),
  })));
  await writeFile(path.join(stage, ".uking-integrity.json"), `${JSON.stringify({
    schema: 1,
    upstream: lock.upstream,
    tag: lock.tag,
    commit: lock.commit,
    files: entries,
  }, null, 2)}\n`, "utf8");
}
function validateReleaseUrl(value) {
  if (!value || /[\s\u0000-\u001f]/.test(value)) throw new Error("OPENTU_RELEASE_URL must be a concrete HTTPS release URL");
  const url = new URL(value);
  if (url.protocol !== "https:" || !url.hostname || url.username || url.password || url.hash) {
    throw new Error("OPENTU_RELEASE_URL must be a credential-free HTTPS URL without a fragment");
  }
  return url.toString();
}
async function createArchive(stage, destination) {
  // List only files. Directories are reconstructed by the installer and count
  // as zero archive entries, avoiding platform-specific `./` root entries that
  // the secure extractor intentionally rejects.
  const files = (await listFiles(stage)).sort().map((item) => item.replaceAll(path.sep, "/"));
  // Windows bsdtar can silently stop reading a UTF-8 `-T` file list at a
  // non-ASCII name. Pass the already-validated names as argv instead, so the
  // archive and its integrity manifest always describe the same file set.
  const result = spawnSync("tar", ["-czf", destination, "-C", stage, ...files], { encoding: "utf8", shell: false });
  if (result.status !== 0) throw new Error(`tar archive failed: ${result.stderr || result.stdout || result.status}`);
}

if (!existsSync(path.join(source, ".git"))) throw new Error(`OpenTu source is missing: ${source}. Set OPENTU_SOURCE_DIR to a clean checkout.`);
const head = spawnSync("git", ["rev-parse", "HEAD"], { cwd: source, encoding: "utf8", shell: process.platform === "win32" });
if (head.status !== 0 || head.stdout.trim() !== lock.commit) throw new Error(`OpenTu must be exactly ${lock.commit}; got ${head.stdout.trim() || "unknown"}`);
if (await sha256(path.join(source, "pnpm-lock.yaml")) !== lock.pnpm_lock_sha256) throw new Error("OpenTu pnpm lock hash differs from pinned lock.json");
if (await sha256(path.join(source, "LICENSE")) !== lock.license_sha256) throw new Error("OpenTu LICENSE hash differs from pinned lock.json");
const sourceStatus = spawnSync("git", ["status", "--porcelain"], { cwd: source, encoding: "utf8", shell: process.platform === "win32" });
if (sourceStatus.status !== 0 || sourceStatus.stdout.trim()) throw new Error("OpenTu source must be clean before applying U-King patches");
const pnpmVersion = spawnSync("corepack", [`pnpm@${lock.pnpm}`, "--version"], { cwd: source, encoding: "utf8", shell: process.platform === "win32" });
if (pnpmVersion.status !== 0 || pnpmVersion.stdout.trim() !== lock.pnpm) throw new Error(`pnpm ${lock.pnpm} is required; got ${pnpmVersion.stdout.trim() || "unknown"}`);

const patchFiles = (await readdir(patches)).filter((entry) => entry.endsWith(".patch")).sort();
const applied = [];
let stage;
for (const entry of patchFiles) {
  run("git", ["apply", "--check", path.join(patches, entry)], source);
  run("git", ["apply", path.join(patches, entry)], source);
  applied.push(entry);
}
try {
  runPinnedPnpm(["install", "--frozen-lockfile"], source);
  runPinnedPnpm(["exec", "nx", "run", "web:build"], source);
  const output = path.join(source, ...lock.output.split("/"));
  if (!existsSync(path.join(output, "index.html"))) throw new Error("OpenTu build did not produce index.html");
  stage = path.join(releaseDir, `.${bundleId}.stage`);
  await rm(stage, { recursive: true, force: true });
  await mkdir(stage, { recursive: true });
  await cp(output, stage, { recursive: true });
  await cp(path.join(source, "LICENSE"), path.join(stage, "LICENSE"));
  await writeFile(path.join(stage, "OPENTU-SOURCE.txt"), [
    `OpenTu upstream: ${lock.upstream}`,
    `tag: ${lock.tag}`,
    `commit: ${lock.commit}`,
    "This archive is an optional local U-King creator-canvas component.",
    "Its contents are verified by .uking-integrity.json before activation.",
    "",
  ].join("\n"), "utf8");
  if (!existsSync(path.join(stage, "index.html"))) throw new Error("staged OpenTu bundle is incomplete");
  await writeIntegrityManifest(stage);
  await mkdir(releaseDir, { recursive: true });
  await rm(archivePath, { force: true });
  await createArchive(stage, archivePath);
  // The archive deliberately contains only regular files; its secure Rust
  // extractor creates parents itself. `file_count` is therefore the complete
  // archive-entry count (files plus zero directory entries), not a source-tree
  // estimate that could drift from the bytes customers download.
  const archiveEntries = await listFiles(stage);
  const unpackedBytes = (await Promise.all(archiveEntries.map((entry) => stat(path.join(stage, entry))))).reduce((total, item) => total + item.size, 0);
  const metadata = {
    schema: 1,
    component: "opentu",
    upstream_version: lock.tag.replace(/^v/, ""),
    upstream_commit: lock.commit,
    bundle_id: bundleId,
    bridge_schema: 1,
    archive_format: "tar.gz",
    archive_bytes: (await stat(archivePath)).size,
    archive_sha256: await sha256(archivePath),
    integrity_manifest_sha256: await sha256(path.join(stage, ".uking-integrity.json")),
    unpacked_bytes: unpackedBytes,
    file_count: archiveEntries.length,
  };
  const releaseUrl = process.env.OPENTU_RELEASE_URL;
  if (releaseUrl) {
    await writeFile(catalogueCandidate, `${JSON.stringify({ ...metadata, url: validateReleaseUrl(releaseUrl) }, null, 2)}\n`, "utf8");
    console.log(`OpenTu ${lock.tag} archive and verified catalogue candidate built in ${releaseDir}`);
  } else {
    console.log(`OpenTu ${lock.tag} archive built in ${releaseDir}; set OPENTU_RELEASE_URL after uploading to generate the commit-ready trusted catalogue entry.`);
  }
} finally {
  // The stage contains minified third-party assets and is never a release
  // artifact. Clean it on both success and failure before restoring patches.
  if (stage) await rm(stage, { recursive: true, force: true });
  // A failed build must leave the source checkout reusable. Successfully applied
  // bridge patches are reset as well: source ownership stays upstream.
  for (const entry of applied.reverse()) {
    const patch = path.join(patches, entry);
    if (succeeds("git", ["apply", "--reverse", "--check", patch], source)) {
      run("git", ["apply", "--reverse", patch], source);
    } else if (!succeeds("git", ["apply", "--check", patch], source)) {
      throw new Error(`OpenTu patch cleanup cannot prove a clean source: ${entry}`);
    }
  }
}
