#!/usr/bin/env node
/**
 * U-King 官网发布器。
 *
 * 默认只做发布计划：校验源码版本、四个产物、Mac Info.plist 和哈希，并写出不含
 * 凭据的 manifest。只有显式传入 `publish` 才会碰远端；二进制先逐个暂存、校验、
 * 备份并原子替换，所有二进制成功后才更新 version.json。
 *
 * 私有配置必须是被 gitignore 的 JSON（或仓库外文件），例如：
 * {
 *   "version": "1.3.2",
 *   "artifactDir": "D:/release/1.3.2",
 *   "websiteDir": "D:/work/u-king/website",
 *   "targets": [{ "host": "release-host", "downloadDir": "/srv/download", "versionPaths": ["/srv/site/uking/version.json"], "sudo": false }],
 *   "oss": { "binary": "ossutil", "bucketPrefix": "oss://example-bucket/uking" }
 * }
 */
import { createHash, randomUUID } from "node:crypto";
import { execFile } from "node:child_process";
import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(SCRIPT_DIR, "..");
const COMMAND_TIMEOUT_MS = 10 * 60 * 1000;
const MANIFEST_DIR_NAME = ".uking-release-manifests";
const ARTIFACTS = [
  { name: "U-King.exe", kind: "windows-portable" },
  { name: "U-King-Setup.exe", kind: "windows-setup" },
  { name: "U-King-Mac.zip", kind: "mac-zip" },
  { name: "U-King-Mac.dmg", kind: "mac-dmg" },
];

function fail(message) {
  throw new Error(message);
}

function usage() {
  return [
    "Usage:",
    "  node scripts/release-uking.mjs [plan] --config <ignored-config.json> [--manifest <path>]",
    "  node scripts/release-uking.mjs publish --config <ignored-config.json> [--manifest <path>]",
    "",
    "Without `publish` this command never uploads or changes a remote host.",
  ].join("\n");
}

function parseArgs(argv) {
  let mode = "plan";
  let modeWasSet = false;
  let configPath;
  let manifestPath;
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "plan" || arg === "publish") {
      if (modeWasSet) fail(`发布模式重复：${arg}`);
      mode = arg;
      modeWasSet = true;
    } else if (arg === "--config") {
      configPath = argv[++index];
      if (!configPath) fail("--config 缺少路径");
    } else if (arg === "--manifest") {
      manifestPath = argv[++index];
      if (!manifestPath) fail("--manifest 缺少路径");
    } else if (arg === "--help" || arg === "-h") {
      console.log(usage());
      process.exit(0);
    } else {
      fail(`未知参数：${arg}\n\n${usage()}`);
    }
  }
  if (!configPath) fail(`必须提供 --config\n\n${usage()}`);
  return { mode, configPath, manifestPath };
}

function run(command, args, options = {}) {
  return new Promise((resolve, reject) => {
    execFile(command, args, {
      cwd: ROOT,
      encoding: "utf8",
      maxBuffer: 16 * 1024 * 1024,
      timeout: COMMAND_TIMEOUT_MS,
      windowsHide: true,
      ...options,
    }, (error, stdout, stderr) => {
      if (error) {
        const detail = String(stderr || stdout || error.message).trim();
        const timedOut = error.killed ? "（超时或被终止）" : "";
        reject(new Error(`${command} failed${timedOut}${detail ? `: ${detail}` : ""}`));
        return;
      }
      resolve(String(stdout));
    });
  });
}

async function exitsZero(command, args, options = {}) {
  try {
    await run(command, args, options);
    return true;
  } catch {
    return false;
  }
}

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function requireString(value, field) {
  if (typeof value !== "string" || value.trim() === "") fail(`${field} 必须是非空字符串`);
  if (/[\0\r\n]/.test(value)) fail(`${field} 不允许控制字符`);
  return value;
}

function requireRemotePath(value, field) {
  requireString(value, field);
  if (!value.startsWith("/")) fail(`${field} 必须是绝对远端路径`);
  if (/\s/.test(value)) fail(`${field} 不允许空白字符`);
  if (value === "/" || /(?:^|\/)\.\.(?:\/|$)/.test(value) || /\/{2,}/.test(value)) fail(`${field} 不允许根目录、.. 或重复斜杠`);
  return value.replace(/\/+$/, "");
}

function rejectCredentialFields(value, trail = "config") {
  if (Array.isArray(value)) {
    value.forEach((entry, index) => rejectCredentialFields(entry, `${trail}[${index}]`));
  } else if (isObject(value)) {
    for (const [key, entry] of Object.entries(value)) {
      if (/(password|secret|token|credential|private.?key|access.?key)/i.test(key)) {
        fail(`${trail}.${key} 看起来像凭据；发布配置不可包含凭据`);
      }
      rejectCredentialFields(entry, `${trail}.${key}`);
    }
  }
}

function resolveConfigPath(baseDir, configuredPath, field) {
  return path.resolve(baseDir, requireString(configuredPath, field));
}

async function readConfig(configArg) {
  const configPath = path.resolve(process.cwd(), configArg);
  let config;
  try {
    config = JSON.parse(await fs.readFile(configPath, "utf8"));
  } catch (error) {
    fail(`读取发布配置失败：${error instanceof Error ? error.message : String(error)}`);
  }
  if (!isObject(config)) fail("发布配置根节点必须是对象");
  rejectCredentialFields(config);
  const relativeToRoot = path.relative(ROOT, configPath);
  if (!relativeToRoot.startsWith("..") && !path.isAbsolute(relativeToRoot)) {
    const ignored = await exitsZero("git", ["check-ignore", "-q", "--", relativeToRoot]);
    if (!ignored) fail("仓库内发布配置必须被 gitignore；不要把部署信息写进公开仓");
  }
  const configDir = path.dirname(configPath);
  const version = requireString(config.version, "config.version");
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) fail("config.version 不是可接受的语义版本");
  if (!Array.isArray(config.targets) || config.targets.length === 0) fail("config.targets 至少需要一个发布目标");
  const targets = config.targets.map((target, index) => {
    if (!isObject(target)) fail(`config.targets[${index}] 必须是对象`);
    const host = requireString(target.host, `config.targets[${index}].host`);
    if (/\s/.test(host)) fail(`config.targets[${index}].host 不允许空白字符`);
    if (host.startsWith("-")) fail(`config.targets[${index}].host 不允许以 - 开头`);
    if (host.includes("://") || /^[^@\s:]+:[^@\s]+@/.test(host)) fail(`config.targets[${index}].host 不允许内嵌凭据`);
    const downloadDir = requireRemotePath(target.downloadDir, `config.targets[${index}].downloadDir`);
    if (!Array.isArray(target.versionPaths) || target.versionPaths.length === 0) fail(`config.targets[${index}].versionPaths 至少需要一项`);
    if (target.sudo !== undefined && typeof target.sudo !== "boolean") fail(`config.targets[${index}].sudo 必须是布尔值`);
    return {
      host,
      downloadDir,
      sudo: target.sudo === true,
      versionPaths: target.versionPaths.map((entry, pathIndex) => requireRemotePath(entry, `config.targets[${index}].versionPaths[${pathIndex}]`)),
    };
  });
  if (!isObject(config.oss)) fail("config.oss 必须是对象");
  const ossBinary = requireString(config.oss.binary, "config.oss.binary");
  const ossPrefix = requireString(config.oss.bucketPrefix, "config.oss.bucketPrefix").replace(/\/+$/, "");
  if (!ossPrefix.startsWith("oss://") || /\s/.test(ossPrefix)) fail("config.oss.bucketPrefix 必须是无空白的 oss:// 前缀");
  if (/^oss:\/\/[^/]*@/.test(ossPrefix)) fail("config.oss.bucketPrefix 不允许内嵌凭据");
  return {
    configPath,
    version,
    artifactDir: resolveConfigPath(configDir, config.artifactDir, "config.artifactDir"),
    websiteDir: resolveConfigPath(configDir, config.websiteDir, "config.websiteDir"),
    targets,
    oss: { binary: ossBinary, bucketPrefix: ossPrefix },
  };
}

async function sha256(filePath) {
  const contents = await fs.readFile(filePath);
  return createHash("sha256").update(contents).digest("hex");
}

async function trackedSourceState() {
  const dirty = (await run("git", ["status", "--porcelain", "--untracked-files=no"])).trim();
  if (dirty) fail("已跟踪源码存在未提交修改；请在独立、干净的发布树运行");
  return { trackedClean: true, commit: (await run("git", ["rev-parse", "HEAD"])).trim() };
}

async function readVersionSources(websiteDir) {
  const cargoLock = await fs.readFile(path.join(ROOT, "src-tauri", "Cargo.lock"), "utf8");
  const cargoVersion = (await fs.readFile(path.join(ROOT, "src-tauri", "Cargo.toml"), "utf8")).match(/^version = "([^"]+)"/m)?.[1];
  const tsVersion = (await fs.readFile(path.join(ROOT, "src", "version.ts"), "utf8")).match(/APP_VERSION = "([^"]+)"/)?.[1];
  const lockVersion = cargoLock.match(/\[\[package\]\]\r?\nname = "u-king-mini"\r?\nversion = "([^"]+)"/)?.[1];
  const changelog = await fs.readFile(path.join(ROOT, "CHANGELOG.md"), "utf8");
  const packageVersion = JSON.parse(await fs.readFile(path.join(ROOT, "package.json"), "utf8")).version;
  const tauriVersion = JSON.parse(await fs.readFile(path.join(ROOT, "src-tauri", "tauri.conf.json"), "utf8")).version;
  const sourceWebsiteVersionPath = path.join(ROOT, "website", "version.json");
  const websiteVersionPath = path.join(websiteDir, "version.json");
  const sourceWebsiteBuffer = await fs.readFile(sourceWebsiteVersionPath);
  const websiteVersionBuffer = await fs.readFile(websiteVersionPath);
  const sourceWebsite = JSON.parse(sourceWebsiteBuffer.toString("utf8"));
  const website = JSON.parse(websiteVersionBuffer.toString("utf8"));
  return {
    packageVersion,
    tsVersion,
    tauriVersion,
    cargoVersion,
    lockVersion,
    sourceWebsiteVersion: sourceWebsite.version,
    deploymentWebsiteVersion: website.version,
    changelog,
    websiteVersionBuffer,
    websiteVersionBytes: websiteVersionBuffer.length,
    websiteVersionSha256: createHash("sha256").update(websiteVersionBuffer).digest("hex"),
  };
}

async function assertVersionConsistency(config) {
  const versions = await readVersionSources(config.websiteDir);
  const values = Object.entries({
    "package.json": versions.packageVersion,
    "src/version.ts": versions.tsVersion,
    "src-tauri/tauri.conf.json": versions.tauriVersion,
    "src-tauri/Cargo.toml": versions.cargoVersion,
    "src-tauri/Cargo.lock:u-king-mini": versions.lockVersion,
    "website/version.json": versions.sourceWebsiteVersion,
  });
  for (const [file, value] of values) {
    if (value !== config.version) fail(`${file} 版本 ${value ?? "缺失"} 与 config.version ${config.version} 不一致`);
  }
  if (versions.deploymentWebsiteVersion !== config.version) {
    fail(`config.websiteDir/version.json 版本 ${versions.deploymentWebsiteVersion ?? "缺失"} 与 config.version ${config.version} 不一致`);
  }
  const escapedVersion = config.version.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  if (!(new RegExp(`^## ${escapedVersion}（`, "m")).test(versions.changelog)) fail(`CHANGELOG.md 缺少 ${config.version} 条目`);
  return {
    buffer: versions.websiteVersionBuffer,
    bytes: versions.websiteVersionBytes,
    sha256: versions.websiteVersionSha256,
  };
}

async function macZipVersion(zipPath) {
  let tool;
  let entriesOutput;
  try {
    entriesOutput = await run("unzip", ["-Z1", zipPath], { cwd: path.dirname(zipPath) });
    tool = "unzip";
  } catch {
    try {
      entriesOutput = await run("tar", ["-tf", zipPath], { cwd: path.dirname(zipPath) });
      tool = "tar";
    } catch {
      fail("校验 U-King-Mac.zip 需要系统 unzip 或 tar");
    }
  }
  const entries = entriesOutput.split(/\r?\n/).filter(Boolean);
  const plistPath = entries.find((entry) => /(^|\/)Contents\/Info\.plist$/.test(entry));
  if (!plistPath) fail("U-King-Mac.zip 内没有 .app/Contents/Info.plist");
  const plist = tool === "unzip"
    ? await run("unzip", ["-p", zipPath, plistPath], { cwd: path.dirname(zipPath) })
    : await run("tar", ["-xOf", zipPath, plistPath], { cwd: path.dirname(zipPath) });
  const version = plist.match(/<key>\s*CFBundleShortVersionString\s*<\/key>\s*<string>\s*([^<\s]+)\s*<\/string>/)?.[1];
  if (!version) fail("无法从 U-King-Mac.zip 的 Info.plist 读取 CFBundleShortVersionString");
  return version;
}

function powershellQuote(value) {
  return `'${String(value).replaceAll("'", "''")}'`;
}

function normalizeWindowsFileVersion(raw, artifactName) {
  const match = raw.trim().match(/\d+\.\d+\.\d+(?:\.\d+)?/);
  if (!match) fail(`${artifactName} 未提供可解析的 Windows FileVersion`);
  const parts = match[0].split(".");
  if (parts.length === 4 && parts[3] === "0") parts.pop();
  return parts.join(".");
}

async function windowsFileVersion(filePath, artifactName) {
  if (process.platform !== "win32") fail(`校验 ${artifactName} 的 FileVersion 需要在 Windows 发布机运行`);
  const command = `$ErrorActionPreference='Stop'; [Console]::Write((Get-Item -LiteralPath ${powershellQuote(filePath)}).VersionInfo.FileVersion)`;
  const raw = await run("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", command]);
  return normalizeWindowsFileVersion(raw, artifactName);
}

async function readMacReleaseProof(config, source, artifacts) {
  const proofPath = path.join(config.artifactDir, "mac-release-proof.json");
  let proof;
  try {
    proof = JSON.parse(await fs.readFile(proofPath, "utf8"));
  } catch (error) {
    fail(`读取 mac-release-proof.json 失败：${error instanceof Error ? error.message : String(error)}`);
  }
  if (!isObject(proof) || proof.schema !== 1) fail("mac-release-proof.json 必须是 schema=1 的对象");
  if (proof.version !== config.version) fail(`mac-release-proof.json version=${proof.version ?? "缺失"}，期望 ${config.version}`);
  if (proof.sourceCommit !== source.commit) fail("mac-release-proof.json sourceCommit 与当前源码提交不一致");
  const zip = artifacts.find((artifact) => artifact.kind === "mac-zip");
  const dmg = artifacts.find((artifact) => artifact.kind === "mac-dmg");
  for (const [field, artifact] of [["zip", zip], ["dmg", dmg]]) {
    const evidence = proof[field];
    if (!isObject(evidence)) fail(`mac-release-proof.json 缺少 ${field} 证据`);
    if (evidence.file !== artifact.name || evidence.sha256 !== artifact.sha256 || evidence.bytes !== artifact.bytes) {
      fail(`mac-release-proof.json 的 ${field} 哈希或大小与下载产物不一致`);
    }
    if (evidence.appVersion !== config.version) fail(`mac-release-proof.json 的 ${field}.appVersion 与发布版本不一致`);
    if (typeof evidence.executableSha256 !== "string" || !/^[a-f0-9]{64}$/i.test(evidence.executableSha256)) {
      fail(`mac-release-proof.json 的 ${field}.executableSha256 无效`);
    }
  }
  if (proof.zip.executableSha256 !== proof.dmg.executableSha256) fail("Mac ZIP 与 DMG 的 app 可执行文件哈希不一致");
  return { sha256: await sha256(proofPath), sourceCommit: proof.sourceCommit, executableSha256: proof.zip.executableSha256 };
}

async function inspectArtifacts(config, source) {
  const artifacts = [];
  for (const definition of ARTIFACTS) {
    const filePath = path.join(config.artifactDir, definition.name);
    const stat = await fs.stat(filePath).catch(() => fail(`缺少发布产物：${filePath}`));
    if (!stat.isFile()) fail(`发布产物不是文件：${filePath}`);
    const artifact = { ...definition, path: filePath, bytes: stat.size, sha256: await sha256(filePath) };
    if (definition.kind.startsWith("windows-")) {
      artifact.fileVersion = await windowsFileVersion(filePath, definition.name);
      if (artifact.fileVersion !== config.version) fail(`${definition.name} 的 FileVersion=${artifact.fileVersion}，期望 ${config.version}`);
    }
    artifacts.push(artifact);
  }
  const macZip = artifacts.find((artifact) => artifact.kind === "mac-zip");
  const plistVersion = await macZipVersion(macZip.path);
  if (plistVersion !== config.version) fail(`U-King-Mac.zip 的 CFBundleShortVersionString=${plistVersion}，期望 ${config.version}`);
  return { artifacts, macZipInfoPlistVersion: plistVersion, macReleaseProof: await readMacReleaseProof(config, source, artifacts) };
}

function shQuote(value) {
  const apostrophe = String.fromCharCode(39);
  return apostrophe + String(value).replaceAll(apostrophe, `${apostrophe}"${apostrophe}"${apostrophe}`) + apostrophe;
}

function remoteJoin(prefix, name) {
  return `${prefix.replace(/\/+$/, "")}/${name}`;
}

function sshArgs(target, script) {
  return ["-o", "BatchMode=yes", "-o", "ConnectTimeout=20", target.host, script];
}

function remoteCommand(target, command) {
  return target.sudo ? `sudo -n sh -c ${shQuote(command)}` : command;
}

async function remoteStage(target, localPath, remotePath, expectedHash, releaseId) {
  if (await sha256(localPath) !== expectedHash) fail(`本地冻结快照已变化：${path.basename(localPath)}`);
  const directory = path.posix.dirname(remotePath);
  const temporaryDir = path.posix.join(directory, ".uking-release-tmp", releaseId);
  const temporaryPath = path.posix.join(temporaryDir, path.posix.basename(remotePath));
  const userStagingDir = path.posix.join("/tmp", ".uking-release-tmp", releaseId);
  const userStagingPath = path.posix.join(userStagingDir, path.posix.basename(remotePath));
  await run("ssh", sshArgs(target, `set -eu; mkdir -p -- ${shQuote(userStagingDir)}`));
  await run("scp", ["-o", "BatchMode=yes", "-o", "ConnectTimeout=20", localPath, `${target.host}:${userStagingPath}`]);
  const stageScript = `set -eu; mkdir -p -- ${shQuote(temporaryDir)}; install -m 0644 -- ${shQuote(userStagingPath)} ${shQuote(temporaryPath)}; sha256sum -- ${shQuote(temporaryPath)} | awk '{print $1}'`;
  const actual = (await run("ssh", sshArgs(target, remoteCommand(target, stageScript)))).trim().split(/\s+/)[0];
  if (actual !== expectedHash) fail(`目标暂存文件哈希不匹配：${path.posix.basename(remotePath)}`);
  return { remotePath, temporaryPath, expectedHash, backupPath: path.posix.join(directory, ".uking-backup", releaseId, path.posix.basename(remotePath)) };
}

async function remoteCommit(target, staged) {
  const commands = ["set -eu"];
  for (const file of staged) {
    commands.push(`mkdir -p -- ${shQuote(path.posix.dirname(file.backupPath))}`);
    commands.push(`if [ -f ${shQuote(file.remotePath)} ]; then cp -p -- ${shQuote(file.remotePath)} ${shQuote(file.backupPath)}; fi`);
    commands.push(`mv -f -- ${shQuote(file.temporaryPath)} ${shQuote(file.remotePath)}`);
    commands.push(`test "$(sha256sum -- ${shQuote(file.remotePath)} | awk '{print $1}')" = ${shQuote(file.expectedHash)}`);
  }
  await run("ssh", sshArgs(target, remoteCommand(target, commands.join("; "))));
}

async function freezeReleaseInputs(inspected, websiteVersion) {
  const snapshotDir = await fs.mkdtemp(path.join(os.tmpdir(), "uking-release-"));
  const artifacts = [];
  for (const artifact of inspected.artifacts) {
    const snapshotPath = path.join(snapshotDir, artifact.name);
    await fs.copyFile(artifact.path, snapshotPath);
    if (await sha256(snapshotPath) !== artifact.sha256) fail(`冻结 ${artifact.name} 时内容发生变化；中止发布`);
    artifacts.push({ ...artifact, path: snapshotPath });
  }
  if (!Buffer.isBuffer(websiteVersion.buffer) || websiteVersion.bytes !== websiteVersion.buffer.length) fail("冻结的 version.json 字节快照无效");
  const metadataHash = createHash("sha256").update(websiteVersion.buffer).digest("hex");
  if (metadataHash !== websiteVersion.sha256) fail("冻结的 version.json 哈希不一致");
  const metadataPath = path.join(snapshotDir, "version.json");
  await fs.writeFile(metadataPath, websiteVersion.buffer, { flag: "wx" });
  if (await sha256(metadataPath) !== websiteVersion.sha256) fail("冻结 version.json 后哈希不一致");
  const metadata = { name: "version.json", path: metadataPath, bytes: websiteVersion.bytes, sha256: websiteVersion.sha256 };
  return { snapshotDir, artifacts, metadata, macZipInfoPlistVersion: inspected.macZipInfoPlistVersion, macReleaseProof: inspected.macReleaseProof };
}

async function ossDownloadedHash(oss, objectPath, localDir) {
  const localPath = path.join(localDir, randomUUID());
  await run(oss.binary, ["cp", objectPath, localPath, "--force"]);
  return sha256(localPath);
}

async function publishToOss(oss, artifact, releaseId) {
  if (await sha256(artifact.path) !== artifact.sha256) fail(`本地冻结快照已变化：${artifact.name}`);
  const verificationDir = await fs.mkdtemp(path.join(os.tmpdir(), "uking-oss-verify-"));
  const destination = remoteJoin(oss.bucketPrefix, artifact.name);
  const backup = remoteJoin(oss.bucketPrefix, `.uking-backup/${releaseId}/${artifact.name}`);
  const temporary = remoteJoin(oss.bucketPrefix, `.uking-release-tmp/${releaseId}/${artifact.name}`);
  const oldHash = await ossDownloadedHash(oss, destination, verificationDir);
  await run(oss.binary, ["cp", destination, backup, "--force"]);
  if (await ossDownloadedHash(oss, backup, verificationDir) !== oldHash) fail(`OSS 旧对象备份核验失败：${artifact.name}`);
  await run(oss.binary, ["cp", artifact.path, temporary, "--force"]);
  if (await ossDownloadedHash(oss, temporary, verificationDir) !== artifact.sha256) fail(`OSS 暂存对象哈希不匹配：${artifact.name}`);
  await run(oss.binary, ["cp", temporary, destination, "--force"]);
  if (await ossDownloadedHash(oss, destination, verificationDir) !== artifact.sha256) fail(`OSS 最终对象哈希不匹配：${artifact.name}`);
}

async function publish(config, plan, journal, persistJournal) {
  const releaseId = journal.releaseId;
  console.log("发布二进制：先暂存、哈希核对、备份并原子替换…");
  for (let index = 0; index < config.targets.length; index += 1) {
    const target = config.targets[index];
    const staged = [];
    for (const artifact of plan.artifacts) {
      staged.push(await remoteStage(target, artifact.path, remoteJoin(target.downloadDir, artifact.name), artifact.sha256, releaseId));
    }
    journal.inFlight = { phase: "target-binaries", targetIndex: index };
    await persistJournal();
    await remoteCommit(target, staged);
    journal.binaryTargetsCommitted.push(index);
    journal.inFlight = null;
    await persistJournal();
  }
  for (const artifact of plan.artifacts) {
    journal.inFlight = { phase: "oss-binary", artifact: artifact.name };
    await persistJournal();
    await publishToOss(config.oss, artifact, releaseId);
    journal.ossBinariesVerified.push(artifact.name);
    journal.inFlight = null;
    await persistJournal();
  }

  console.log("所有二进制已核验；现在发布 version.json…");
  for (let index = 0; index < config.targets.length; index += 1) {
    const target = config.targets[index];
    const staged = [];
    for (const remotePath of target.versionPaths) {
      staged.push(await remoteStage(target, plan.metadata.path, remotePath, plan.metadata.sha256, releaseId));
    }
    journal.inFlight = { phase: "target-metadata", targetIndex: index };
    await persistJournal();
    await remoteCommit(target, staged);
    journal.metadataTargetsCommitted.push(index);
    journal.inFlight = null;
    await persistJournal();
  }
  journal.inFlight = { phase: "oss-metadata", artifact: plan.metadata.name };
  await persistJournal();
  await publishToOss(config.oss, plan.metadata, releaseId);
  journal.ossMetadataVerified = true;
  journal.inFlight = null;
  await persistJournal();
}

function isInside(parent, candidate) {
  const relative = path.relative(parent, candidate);
  return relative !== "" && !relative.startsWith("..") && !path.isAbsolute(relative);
}

async function assertNewOutputPath(outputPath, outputRoot) {
  if (!isInside(outputRoot, outputPath) || path.dirname(outputPath) !== outputRoot || path.extname(outputPath) !== ".json") {
    fail(`manifest 必须写在专用目录 ${outputRoot} 内，且必须是 .json 文件`);
  }
  await fs.mkdir(outputRoot, { recursive: true });
  if ((await fs.lstat(outputRoot)).isSymbolicLink()) fail(`manifest 专用目录不能是符号链接：${outputRoot}`);
  try {
    await fs.lstat(outputPath);
    fail(`输出文件已经存在，拒绝覆盖：${outputPath}`);
  } catch (error) {
    if (error instanceof Error && error.code === "ENOENT") return;
    throw error;
  }
}

async function writeNewJson(outputPath, value) {
  await fs.writeFile(outputPath, `${JSON.stringify(value, null, 2)}\n`, { encoding: "utf8", flag: "wx" });
}

async function replaceOwnedJson(outputPath, value) {
  const temporary = path.join(path.dirname(outputPath), `.${path.basename(outputPath)}.${randomUUID()}.tmp`);
  await fs.writeFile(temporary, `${JSON.stringify(value, null, 2)}\n`, { encoding: "utf8", flag: "wx" });
  await fs.rename(temporary, outputPath);
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const config = await readConfig(args.configPath);
  const source = await trackedSourceState();
  const websiteVersion = await assertVersionConsistency(config);
  const inspected = await inspectArtifacts(config, source);
  const frozen = await freezeReleaseInputs(inspected, websiteVersion);
  const manifest = {
    schemaVersion: 1,
    generatedAt: new Date().toISOString(),
    mode: args.mode,
    version: config.version,
    commit: source.commit,
    trackedSourceClean: source.trackedClean,
    hostPlatform: `${process.platform}-${process.arch}`,
    websiteVersionSha256: frozen.metadata.sha256,
    macZipInfoPlistVersion: frozen.macZipInfoPlistVersion,
    macReleaseProof: frozen.macReleaseProof,
    artifacts: frozen.artifacts.map(({ name, kind, bytes, sha256, fileVersion }) => ({ name, kind, bytes, sha256, ...(fileVersion ? { fileVersion } : {}) })),
  };
  const outputRoot = path.join(config.artifactDir, MANIFEST_DIR_NAME);
  const manifestPath = args.manifestPath
    ? path.resolve(process.cwd(), args.manifestPath)
    : path.join(outputRoot, `release-manifest-${config.version}-${randomUUID()}.json`);
  await assertNewOutputPath(manifestPath, outputRoot);
  await writeNewJson(manifestPath, manifest);
  console.log(`发布计划就绪：${config.version}，已校验 4 个产物、Windows FileVersion 与 Mac 构建证明。`);
  console.log(`manifest: ${manifestPath}`);
  if (args.mode === "publish") {
    const releaseId = `${config.version}-${randomUUID()}`;
    const journalPath = path.join(outputRoot, `release-journal-${config.version}-${releaseId.slice(config.version.length + 1)}.json`);
    await assertNewOutputPath(journalPath, outputRoot);
    const journal = {
      schemaVersion: 1,
      releaseId,
      version: config.version,
      commit: source.commit,
      status: "publishing",
      binaryTargetsCommitted: [],
      ossBinariesVerified: [],
      metadataTargetsCommitted: [],
      ossMetadataVerified: false,
      inFlight: null,
    };
    await writeNewJson(journalPath, journal);
    const persistJournal = () => replaceOwnedJson(journalPath, journal);
    try {
      await publish(config, frozen, journal, persistJournal);
      journal.status = "completed";
      await persistJournal();
      console.log("发布完成：二进制与 version.json 已按受控顺序上传。\n");
    } catch (error) {
      journal.status = "failed";
      await persistJournal().catch(() => {});
      console.error(`发布中断；已提交目标的版本化备份仍保留在远端。journal: ${journalPath}`);
      throw error;
    }
  } else {
    console.log("默认 plan 模式：未联网、未上传、未修改远端。\n");
  }
}

main().catch((error) => {
  console.error(`release-uking: ${error instanceof Error ? error.message : String(error)}`);
  process.exit(1);
});
