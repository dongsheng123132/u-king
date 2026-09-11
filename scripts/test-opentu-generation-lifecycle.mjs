#!/usr/bin/env node
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, mkdir, readFile, rm, copyFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import ts from "typescript";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const source = path.resolve(process.argv[2] || process.env.OPENTU_SOURCE_DIR || "");
if (!process.argv[2] && !process.env.OPENTU_SOURCE_DIR) {
  throw new Error("Pass the pinned OpenTu checkout as an argument or set OPENTU_SOURCE_DIR.");
}

const appPath = "apps/web/src/app/app.tsx";
const runtimePath = "packages/drawnix/src/components/startup/DrawnixDeferredRuntime.tsx";
const sourceApp = path.join(source, appPath);
const sourceRuntime = path.join(source, runtimePath);
const bridgePatches = [
  "0001-uking-local-project-bridge.patch",
  "0002-uking-xiapan-provider.patch",
  "0003-uking-portable-canvas-media.patch",
  "0004-uking-bridge-conflict-recovery.patch",
].map((entry) => path.join(root, "third_party", "opentu", "patches", entry));
const lifecyclePatch = path.join(root, "third_party", "opentu", "deferred", "0006-uking-generation-lifecycle.patch");

function git(cwd, args) {
  const result = spawnSync("git", args, { cwd, encoding: "utf8", shell: false });
  if (result.error || result.status !== 0) {
    throw new Error(`git ${args.join(" ")} failed: ${result.stderr || result.error?.message || result.status}`);
  }
}

function markedSource(sourceText, name) {
  const begin = `// uking-generation-lifecycle:${name}-begin`;
  const end = `// uking-generation-lifecycle:${name}-end`;
  const start = sourceText.indexOf(begin);
  const finish = sourceText.indexOf(end);
  assert.ok(start >= 0 && finish > start, `missing ${name} lifecycle source marker`);
  return sourceText.slice(start + begin.length, finish);
}

function evaluateTypeScript(sourceText, globals = {}) {
  const output = ts.transpileModule(sourceText, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const context = { exports: {}, ...globals };
  vm.runInNewContext(output, context, { timeout: 1_000 });
  return context.exports;
}

const temp = await mkdtemp(path.join(tmpdir(), "uking-opentu-generation-lifecycle-"));
try {
  const targetApp = path.join(temp, appPath);
  const targetRuntime = path.join(temp, runtimePath);
  await mkdir(path.dirname(targetApp), { recursive: true });
  await mkdir(path.dirname(targetRuntime), { recursive: true });
  await copyFile(sourceApp, targetApp);
  await copyFile(sourceRuntime, targetRuntime);
  git(temp, ["init", "-q"]);
  git(temp, ["add", appPath, runtimePath]);

  for (const patch of bridgePatches) {
    const args = ["apply", "--unidiff-zero", `--include=${appPath}`, "--check", patch];
    git(temp, args);
    git(temp, args.filter((arg) => arg !== "--check"));
  }
  git(temp, ["apply", "--unidiff-zero", "--check", lifecyclePatch]);
  git(temp, ["apply", "--unidiff-zero", lifecyclePatch]);

  const patchedApp = await readFile(targetApp, "utf8");
  assert.match(patchedApp, /type: 'uking:bridge:generation-state', active/);
  assert.match(patchedApp, /capabilities: \{ generationState: true \}/);
  assert.match(patchedApp, /UKING_TASK_STORAGE_READY_EVENT/);
  const publisher = evaluateTypeScript(markedSource(patchedApp, "publisher"), {
    TaskStatus: { PENDING: "pending", PROCESSING: "processing", COMPLETED: "completed", FAILED: "failed", CANCELLED: "cancelled" },
  });
  assert.equal(publisher.getUkingGenerationState([], false), undefined, "new bridge stays unknown before queue recovery");
  assert.equal(publisher.getUkingGenerationState([{ type: "chat", status: "processing" }], true), true, "paid text generation is active");
  assert.equal(publisher.getUkingGenerationState([{ type: "image", status: "completed" }], true), false, "terminal task releases the guard");
  assert.equal(publisher.getUkingGenerationState([], true), false, "deleting the active task releases the guard");

  const host = await readFile(path.join(root, "src", "CreatorCanvas.tsx"), "utf8");
  const lifecycle = evaluateTypeScript(markedSource(host, "host"));
  const frame = {};
  assert.equal(lifecycle.isCurrentCanvasMessage({ origin: "http://localhost:1", source: frame }, "http://localhost:1", frame), true);
  assert.equal(lifecycle.isCurrentCanvasMessage({ origin: "http://invalid.test", source: frame }, "http://localhost:1", frame), false, "wrong origin cannot change state");
  assert.equal(lifecycle.isCurrentCanvasMessage({ origin: "http://localhost:1", source: {} }, "http://localhost:1", frame), false, "wrong source cannot change state");
  assert.equal(lifecycle.nextGenerationState("unknown", { type: "uking:bridge:ready", capabilities: { generationState: true } }), "unknown", "new bridge remains locked until queue state arrives");
  assert.equal(lifecycle.nextGenerationState("unknown", { type: "uking:bridge:ready" }), "idle", "legacy bridge remains usable");
  assert.equal(lifecycle.nextGenerationState("unknown", { type: "uking:bridge:ready", capabilities: { generationState: "true" } }), "unknown", "malformed capability cannot unlock");
  assert.equal(lifecycle.nextGenerationState("active", { type: "uking:bridge:ready", capabilities: { generationState: true } }), "active", "duplicate ready cannot clear an active task");
  assert.equal(lifecycle.nextGenerationState("unknown", { type: "uking:bridge:generation-state", active: true }), "active");
  assert.equal(lifecycle.nextGenerationState("active", { type: "other" }), "active", "a long active task has no timer-based unlock");
  assert.equal(lifecycle.nextGenerationState("active", { type: "uking:bridge:generation-state", active: false }), "idle", "terminal queue state unlocks");
  assert.equal(lifecycle.nextGenerationState("active", { type: "uking:bridge:generation-state", active: "false" }), "active", "malformed state cannot unlock");
  assert.match(host, /const canvasOperationBlocked = busy \|\| saving \|\| Boolean\(saveError\) \|\| generationLocked/);
  assert.doesNotMatch(host, /toggleFullscreen\(\)} disabled=/);
  console.log("OpenTu generation lifecycle bridge contract passed.");
} finally {
  await rm(temp, { recursive: true, force: true });
}
