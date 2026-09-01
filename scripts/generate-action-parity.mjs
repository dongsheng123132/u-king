#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const generatedDirectory = path.join(root, "src", "generated");
const manifestPath = path.join(generatedDirectory, "action-parity.json");
const actionParityCli = path.join(root, "node_modules", "action-parity", "bin", "action-parity.mjs");
const args = new Set(process.argv.slice(2));
const known = new Set(["--check", "--verify", "--json", "--print-manifest"]);

for (const argument of args) {
  if (!known.has(argument)) failUsage(`unknown option: ${argument}`);
}
if (args.has("--check") && args.has("--verify")) {
  failUsage("choose either --check or --verify");
}
if (args.has("--print-manifest") && args.size !== 1) {
  failUsage("--print-manifest cannot be combined with other options");
}

try {
  const bindingSnapshot = runUkingJson(["action", "bindings", "--json", "--no-input"]);
  const manifest = hostManifest(
    runUkingJson(["action", "manifest", "--json", "--no-input"]),
    bindingSnapshot
  );
  if (args.has("--print-manifest")) {
    process.stdout.write(`${JSON.stringify(manifest)}\n`);
  } else {
    await generateOrCheck(manifest, bindingSnapshot);
  }
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  if (args.has("--json")) {
    process.stdout.write(`${JSON.stringify({ ok: false, data: null, error: message })}\n`);
  } else {
    process.stderr.write(`${message}\n`);
  }
  process.exitCode = 1;
}

async function generateOrCheck(manifest, bindingSnapshot) {
  const verify = args.has("--verify");
  const check = args.has("--check") || verify;
  const expectedManifest = `${stableStringify(manifest, 2)}\n`;
  const manifestStatus = await fileStatus(manifestPath, expectedManifest);

  if (!check) {
    await mkdir(generatedDirectory, { recursive: true });
    await atomicWrite(manifestPath, expectedManifest);
  }

  let client = { ok: false, files: [{ path: path.join(generatedDirectory, "action-client.ts"), status: "missing" }] };
  if (!check || manifestStatus !== "missing") {
    client = runActionParity(check);
  }

  let bindings = null;
  let conformance = null;
  if (verify && manifestStatus === "current" && client.ok) {
    bindings = bindingSnapshot;
    // 全量 conformance，不带 `--only runtime.`。之前那行把 doc./browser./app.*（含会花钱的
    // imagefix）30 个动作晾在闸外 —— 全量跑实测 81 pass / 0 fail，没有理由只查 70%。
    conformance = runUkingJson(["action", "conformance", "--json", "--no-input"]);
  }

  const ok = (!check || manifestStatus === "current") && client.ok &&
    (!verify || (bindings?.ok === true && conformance?.ok === true));
  const data = {
    mode: verify ? "verify" : check ? "check" : "generate",
    manifest: { path: manifestPath, status: check ? manifestStatus : "generated" },
    client: client.files,
    actions: manifest.actions.length,
    bindings: bindings === null ? null : {
      ok: bindings.ok,
      bound: bindings.bound?.length ?? 0,
      unbound: bindings.unbound?.length ?? 0,
      stale: bindings.stale?.length ?? 0
    },
    conformance: conformance === null ? null : {
      ok: conformance.ok,
      pass: conformance.pass,
      fail: conformance.fail,
      skip: conformance.skip,
      not_ready: conformance.not_ready?.length ?? 0
    }
  };

  if (args.has("--json")) {
    process.stdout.write(`${JSON.stringify({ ok, data, error: ok ? null : "action_parity_drift_or_failure" })}\n`);
  } else {
    process.stdout.write(`${data.manifest.status}\t${manifestPath}\n`);
    for (const file of client.files) process.stdout.write(`${file.status}\t${file.path}\n`);
  }
  process.exitCode = ok ? 0 : 1;
}

function hostManifest(full, bindingSnapshot) {
  const guiBound = new Set((bindingSnapshot.bound ?? []).map((binding) => binding.id));
  const actions = full.actions
    // `runtime.*` 是设备管理动作；`media.*` 是客户显式提交的内容动作。
    // 两者都必须出现在 CLI/MCP 的同一份契约里，不能因为前缀不同把识图链藏掉。
    .filter((action) => action.id.startsWith("runtime.") || action.id.startsWith("media."))
    .map((action) => ({
      ...action,
      bindings: action.bindings.filter((binding) =>
        binding.surface === "cli" || (binding.surface === "desktop" && guiBound.has(action.id))
      )
    }));
  return {
    ...full,
    generated_from: {
      generator: "U-King Action Registry host projection",
      revision: full.application.version
    },
    surfaces: full.surfaces
      .filter((surface) => surface.id === "desktop" || surface.id === "cli")
      .map((surface) => surface.id === "desktop" ? {
        ...surface,
        required_for_parity: false,
        exclusion_reason: "U-King is migrating existing GUI controls incrementally; only Actions with verified data-action-id markers are declared on desktop."
      } : surface),
    actions,
    state: {
      ...full.state,
      queries: (full.state?.queries ?? []).filter((id) => id.startsWith("runtime.") || id.startsWith("media.")),
      events: (full.state?.events ?? []).filter((id) => id.startsWith("runtime.") || id.startsWith("media."))
    }
  };
}

function runUkingJson(commandArgs) {
  const result = spawnSync(
    "cargo",
    ["run", "--quiet", "--manifest-path", path.join(root, "src-tauri", "Cargo.toml"), "--", ...commandArgs],
    { cwd: root, encoding: "utf8", windowsHide: true }
  );
  if (result.status !== 0) {
    throw new Error(`U-King Registry export failed (${result.status}): ${result.stderr.trim()}`);
  }
  try {
    return JSON.parse(result.stdout);
  } catch {
    throw new Error(`U-King Registry returned invalid JSON: ${result.stdout.slice(0, 200)}`);
  }
}

function runActionParity(check) {
  const cliArgs = [
    actionParityCli,
    "generate",
    manifestPath,
    "--out-dir",
    generatedDirectory,
    "--typescript",
    "--json",
    ...(check ? ["--check"] : [])
  ];
  const result = spawnSync(process.execPath, cliArgs, { cwd: root, encoding: "utf8", windowsHide: true });
  let envelope;
  try {
    envelope = JSON.parse(result.stdout);
  } catch {
    throw new Error(`ActionParity returned invalid JSON: ${result.stdout.slice(0, 200)}`);
  }
  if (result.status !== 0 && envelope.data === null) {
    throw new Error(envelope.message ?? envelope.error ?? `ActionParity exited ${result.status}`);
  }
  return envelope.data;
}

async function fileStatus(file, expected) {
  try {
    return normalizeLineEndings(await readFile(file, "utf8")) === normalizeLineEndings(expected)
      ? "current"
      : "drifted";
  } catch (error) {
    if (error?.code === "ENOENT") return "missing";
    throw error;
  }
}

function normalizeLineEndings(value) {
  return value.replace(/\r\n/g, "\n");
}

function stableStringify(value, space = 0) {
  return JSON.stringify(canonicalize(value), null, space);
}

function canonicalize(value) {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonicalize(value[key])]));
  }
  return value;
}

async function atomicWrite(target, content) {
  const temporary = `${target}.tmp-${process.pid}`;
  await writeFile(temporary, content, "utf8");
  await rename(temporary, target);
}

function failUsage(message) {
  if (process.argv.includes("--json")) {
    process.stdout.write(`${JSON.stringify({ ok: false, data: null, error: "invalid_usage", message })}\n`);
  } else {
    process.stderr.write(`${message}\n`);
  }
  process.exit(2);
}
