#!/usr/bin/env node
/**
 * Offline provider catalog checks. No network calls, credentials, or user config.
 * Run with the bundled Node.js: node scripts/verify-provider-endpoints.mjs
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { PROVIDER_TEMPLATES } from "../src/lib/providerTemplates.ts";
import { resolveProviderPresentation } from "../src/lib/providerPresentation.ts";

const failures = [];
let checks = 0;
function check(name, verify) {
  checks++;
  try {
    verify();
  } catch (error) {
    failures.push({ name, error: error.message });
  }
}

for (const file of ["../src-tauri/skills/install-windows.json", "../website/skills/install-windows.json"]) {
  check(`${file}: template fields match the UI catalog`, () => {
    const manifest = JSON.parse(readFileSync(new URL(file, import.meta.url), "utf8"));
    assert.deepEqual(manifest.provider_templates, PROVIDER_TEMPLATES);
  });
}

check("template names are unique", () => {
  assert.equal(new Set(PROVIDER_TEMPLATES.map((template) => template.name)).size, PROVIDER_TEMPLATES.length);
});

for (const template of PROVIDER_TEMPLATES) {
  check(`${template.name}: usable base addresses and recognized brand`, () => {
    for (const base of [template.openai_base, template.anthropic_base].filter(Boolean)) {
      const url = new URL(base);
      assert.equal(url.protocol, "https:");
      assert.equal(base, base.trim());
      assert.equal(url.username, "");
      assert.equal(url.password, "");
      assert.equal(url.search, "");
      assert.equal(url.hash, "");
      assert.ok(!/\/(?:chat\/completions|responses|messages)\/?$/.test(url.pathname), "base must not include a request route");
    }
    assert.notEqual(resolveProviderPresentation(template).logo, "unknown");
  });
}

const mimo = PROVIDER_TEMPLATES.find((template) => template.name === "小米 MiMo");
const plan = PROVIDER_TEMPLATES.find((template) => template.name === "小米 MiMo（Token Plan）");
check("MiMo pay-as-you-go and Token Plan have separate credentials and endpoints", () => {
  assert.ok(mimo && plan);
  assert.equal(mimo.openai_base, "https://api.xiaomimimo.com/v1");
  assert.equal(mimo.anthropic_base, "https://api.xiaomimimo.com/anthropic");
  assert.equal(plan.openai_base, "https://token-plan-cn.xiaomimimo.com/v1");
  assert.equal(plan.anthropic_base, "https://token-plan-cn.xiaomimimo.com/anthropic");
  assert.ok(mimo.key_hint.includes("sk-"));
  assert.ok(plan.key_hint.includes("tp-") && plan.key_hint.includes("ttp-"));
  assert.equal(mimo.model, "mimo-v2.6-flash");
  assert.equal(plan.model, "mimo-v2.6-flash");
});

const officialHosts = [
  "api.xiaomimimo.com",
  "token-plan-cn.xiaomimimo.com",
  "token-plan-sgp.xiaomimimo.com",
  "token-plan-ams.xiaomimimo.com",
];
for (const host of officialHosts) {
  for (const field of ["openai_base", "anthropic_base"]) {
    check(`${host}: ${field} identifies the MiMo brand`, () => {
      assert.deepEqual(resolveProviderPresentation({ [field]: `https://${host}/v1` }), { logo: "xiaomi-mimo", group: "vendor" });
    });
  }
}
check("URL parsing accepts equivalent uppercase hosts and the standard HTTPS port", () => {
  assert.equal(resolveProviderPresentation({ openai_base: "https://API.XIAOMIMIMO.COM:443/v1" }).logo, "xiaomi-mimo");
});

for (const base of [
  "https://api.xiaomimimo.com.example.com/v1",
  "https://fake-api.xiaomimimo.com/v1",
  "https://token-plan-fake.xiaomimimo.com/v1",
  "https://api.xiaomimimo.com@evil.example/v1",
  "https://evil.example/?next=https://api.xiaomimimo.com/v1",
  "ftp://api.xiaomimimo.com/v1",
  "api.xiaomimimo.com/v1",
  "not a URL",
]) {
  check(`${base}: does not inherit the official MiMo brand`, () => {
    assert.deepEqual(resolveProviderPresentation({ openai_base: base, name: "小米 MiMo" }), { logo: "unknown", group: "unknown" });
  });
}

console.log(JSON.stringify({ ok: failures.length === 0, checks, templates: PROVIDER_TEMPLATES.length, failures }, null, 2));
if (failures.length) process.exitCode = 1;
