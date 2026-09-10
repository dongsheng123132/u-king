import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");

test("OpenTu local bundle is fixed to the approved MIT v1.1.6 source", async () => {
  const lock = JSON.parse(await readFile(path.join(root, "third_party/opentu/lock.json"), "utf8"));
  assert.equal(lock.tag, "v1.1.6");
  assert.equal(lock.commit, "48802871554c5b8221b4c5d70baff0b68d00df46");
  assert.equal(lock.license, "MIT");
  assert.match(lock.pnpm, /^10\.21\.0$/);
  assert.match(lock.pnpm_lock_sha256, /^[a-f0-9]{64}$/);
});

test("customer bundle script is a pinned native build, never Docker", async () => {
  const script = await readFile(path.join(root, "scripts/build-opentu-local.mjs"), "utf8");
  assert.match(script, /--frozen-lockfile/);
  assert.match(script, /git.*rev-parse/);
  assert.match(script, /corepack/);
  assert.match(script, /const releaseDir = path\.join\(root, "outputs", "creator-components"\)/);
  assert.match(script, /catalogueCandidate/);
  assert.match(script, /const archivePath = path\.join\(releaseDir, `\$\{bundleId\}\.tar\.gz`\)/);
  assert.doesNotMatch(script, /"-T", listPath/);
  assert.match(script, /\["-czf", destination, "-C", stage, \.\.\.files\]/);
  assert.match(script, /writeIntegrityManifest/);
  assert.match(script, /license_sha256/);
  assert.match(script, /\.uking-integrity\.json/);
  assert.match(script, /OpenTu patch cleanup cannot prove a clean source/);
  assert.doesNotMatch(script.toLowerCase(), /run\(\s*["']docker/);
  assert.doesNotMatch(script, /src-tauri[\\/]resources[\\/]opentu/);
  assert.doesNotMatch(script, /path\.join\(root, "dist", "creator-components"\)/);
});

test("the web surface cannot self-authorize ActionParity writes", async () => {
  const server = await readFile(path.join(root, "src-tauri/src/creator_local/mod.rs"), "utf8");
  const host = await readFile(path.join(root, "src/CreatorCanvas.tsx"), "utf8");
  assert.match(server, /host_ipc_required/);
  assert.match(server, /connect-src 'self'/);
  assert.match(server, /A canvas is an untrusted, replaceable web surface/);
  assert.doesNotMatch(server, /let allowed = \[/);
  assert.match(host, /event\.origin !== origin/);
  assert.match(host, /event\.source !== frame\.current\?\.contentWindow/);
  assert.match(host, /request\.input\.project_id !== projectId/);
  assert.match(host, /runtime\.creator\.project\.save/);
  assert.match(host, /X-Uking-Capability/);
  assert.match(host, /readLocalAssetAsDataUrl/);
  assert.doesNotMatch(host, /runtime\.creator\.image\.submit.*postMessage/s);
});

test("the pinned OpenTu patch only speaks the narrow local-project protocol", async () => {
  const patch = await readFile(path.join(root, "third_party/opentu/patches/0001-uking-local-project-bridge.patch"), "utf8");
  assert.match(patch, /uking:bridge:project\.inspect/);
  assert.match(patch, /uking:bridge:project\.save/);
  assert.match(patch, /uking:bridge:image\.insert/);
  assert.match(patch, /event\.source !== window\.parent/);
  assert.doesNotMatch(patch, /localStorage\.setItem/);
  assert.doesNotMatch(patch, /https?:\/\//);
});

test("the local host activates OpenTu bridge mode without placing capability in the URL", async () => {
  const host = await readFile(path.join(root, "src/CreatorCanvas.tsx"), "utf8");
  const patch = await readFile(path.join(root, "third_party/opentu/patches/0001-uking-local-project-bridge.patch"), "utf8");
  const modeUrl = host.slice(host.indexOf("const canvasUrl"), host.indexOf("return <div"));
  assert.match(modeUrl, /new URL\(url\)/);
  assert.match(modeUrl, /searchParams\.set\("uking_host", "1"\)/);
  assert.match(host, /src=\{canvasUrl\}/);
  assert.doesNotMatch(modeUrl, /capability/i);
  assert.match(patch, /new URLSearchParams\(window\.location\.search\)\.get\('uking_host'\) === '1'/);
});

test("canvas protects unsaved work and recovers an installed component without forcing a new project", async () => {
  const host = await readFile(path.join(root, "src/CreatorCanvas.tsx"), "utf8");
  assert.match(host, /if \(busy \|\| saving \|\| saveError\)/);
  assert.match(host, /请先修复保存问题后再卸载组件/);
  assert.match(host, /卸载损坏组件，保留项目/);
  assert.match(host, /onClick=\{\(\) => void start\(\)\}/);
  assert.match(host, /onClick=\{\(\) => void start\(\{ createNew: true \}\)\}/);
});
