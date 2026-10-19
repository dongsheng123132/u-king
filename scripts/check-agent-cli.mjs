/**
 * Installed CLI startup audit. No model requests, login, or changes to real config.
 * Each child uses an isolated home, data/config/cache/temp and cleared credentials.
 * Run: node scripts/check-agent-cli.mjs [--out <report.json>] [--only <command,...>]
 *      [--sandbox-root <directory>] (useful when the system TEMP itself is broken)
 *      [--mimo-db] [--compare-default-temp] (local SQLite smoke; no model calls)
 * Reports startup only; a passing --help is not evidence of working model access.
 */
import { spawn } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir, homedir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const windows = process.platform === "win32";
const home = homedir();
const flags = process.argv.slice(2);
const option = (name) => flags.includes(name) ? flags[flags.indexOf(name) + 1] : undefined;
const rs = readFileSync(path.join(repo, "src-tauri/src/tools.rs"), "utf8");
const registry = rs.split("pub const TOOL_SPECS: &[ToolSpec] = &[")[1]?.split("\n];")[0] ?? "";
const registered = new Set([...registry.matchAll(/\bcmd:\s*"([^"]+)"/g)].map((m) => m[1]));
const manifest = JSON.parse(readFileSync(path.join(repo, "src-tauri/skills/install-windows.json"), "utf8"));
const manifestCommands = new Set(Object.values(manifest.tools).map((tool) => tool.verify_cmd?.match(/^([\w-]+)\s+--version\b/)?.[1])
  .filter((cmd) => cmd && cmd !== "powershell"));
const commands = [...new Set([...registered, ...manifestCommands,
  "cline", "aider", "goose", "amp", "droid", "copilot", "openhands",
])];
const dirs = [...new Set([
  path.join(home, ".uking/runtime/node"),
  path.join(home, ".uking/runtime/python/Scripts"),
  process.env.APPDATA && path.join(process.env.APPDATA, "npm"),
  path.join(home, "bin"), path.join(home, ".local/bin"),
  path.join(home, ".kimi-code/bin"), path.join(home, ".grok/bin"), path.join(home, ".muse/bin"),
  ...(process.env.PATH ?? "").split(path.delimiter),
].filter(Boolean))];
const extensions = windows ? [".cmd", ".exe", ".bat", ".ps1", ""] : [""];
const safeText = (s) => s.replace(/\x1b\[[0-9;?]*[a-zA-Z]/g, "")
  .replaceAll(repo.replaceAll("\\", "\\\\"), "<repo>").replaceAll(home.replaceAll("\\", "\\\\"), "<home>")
  .replaceAll(repo, "<repo>").replaceAll(home, "<home>")
  .replace(/\b(?:sk|tp|key)-[\w-]{12,}\b/gi, "[redacted]").trim();

function resolve(cmd) {
  for (const dir of dirs) for (const ext of extensions) {
    let candidate = path.join(dir, cmd + ext);
    if (!existsSync(candidate)) continue;
    // U-King's forwarding shims use USERPROFILE; follow them before sandboxing.
    if (windows && candidate.endsWith(".cmd")) {
      const body = readFileSync(candidate, "utf8");
      if (body.includes("U-King CLI command guard")) {
        const target = body.match(/(?:call\s+)?"([^"\r\n]+)"\s+%\*/i)?.[1];
        if (target) candidate = target.replace(/%([^%]+)%/g, (_, key) => process.env[key] ?? "");
      }
    }
    if (existsSync(candidate)) return candidate;
  }
  return null;
}

async function run(executable, args, sandbox, tempOverride) {
  const childEnv = { ...process.env };
  for (const name of Object.keys(childEnv)) {
    if (/KEY|TOKEN|SECRET|AUTH|PASSWORD|^OPENCODE_|^CLAUDE_|^CODEX_|^GEMINI_|^QWEN_|^KIMI_|^MIMO_|^DSH_|^HERMES_/i.test(name)) delete childEnv[name];
  }
  Object.assign(childEnv, {
    HOME: sandbox, USERPROFILE: sandbox, UKING_TEST_HOME: sandbox,
    APPDATA: path.join(sandbox, "AppData"), LOCALAPPDATA: path.join(sandbox, "LocalAppData"),
    XDG_CONFIG_HOME: path.join(sandbox, "config"), XDG_DATA_HOME: path.join(sandbox, "data"),
    XDG_CACHE_HOME: path.join(sandbox, "cache"), XDG_STATE_HOME: path.join(sandbox, "state"),
    TEMP: tempOverride ?? path.join(sandbox, "tmp"), TMP: tempOverride ?? path.join(sandbox, "tmp"),
    OPENCLAW_HOME: path.join(sandbox, "openclaw"), DSH_HOME: path.join(sandbox, "dsh"),
    GEMINI_CLI_HOME: sandbox, NO_COLOR: "1", CI: "1", DO_NOT_TRACK: "1",
    DISABLE_AUTOUPDATER: "1", DISABLE_TELEMETRY: "1", OTEL_SDK_DISABLED: "true",
    PATH: dirs.join(path.delimiter),
  });
  const started = Date.now();
  return new Promise((resolveResult) => {
    let stdout = "", stderr = "", timedOut = false, settled = false;
    let child;
    if (windows && /\.(cmd|bat)$/i.test(executable)) {
      child = spawn(process.env.ComSpec ?? "C:\\Windows\\System32\\cmd.exe",
        ["/d", "/s", "/c", `""${executable}" ${args.map((arg) => /[\s&|<>^]/.test(arg) ? `"${arg}"` : arg).join(" ")}"`],
        { cwd: sandbox, env: childEnv, windowsHide: true, windowsVerbatimArguments: true, stdio: ["ignore", "pipe", "pipe"] });
    } else if (windows && executable.endsWith(".ps1")) {
      child = spawn("powershell.exe", ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", executable, ...args],
        { cwd: sandbox, env: childEnv, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
    } else {
      child = spawn(executable, args, { cwd: sandbox, env: childEnv, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
    }
    const finish = (code, error = null) => {
      if (settled) return;
      settled = true; clearTimeout(timer);
      resolveResult({ args, code, timed_out: timedOut, ms: Date.now() - started,
        output: safeText(stdout + "\n" + stderr).slice(0, 2200), error });
    };
    child.stdout.on("data", (chunk) => { if (stdout.length < 100_000) stdout += chunk; });
    child.stderr.on("data", (chunk) => { if (stderr.length < 100_000) stderr += chunk; });
    child.once("error", (e) => finish(null, safeText(e.message)));
    child.once("close", (code) => finish(code));
    const timer = setTimeout(() => {
      timedOut = true;
      if (windows && child.pid) {
        const killer = spawn(path.join(process.env.SystemRoot ?? "C:\\Windows", "System32/taskkill.exe"),
          ["/PID", String(child.pid), "/T", "/F"], { windowsHide: true, stdio: "ignore" });
        killer.once("error", () => child.kill());
      } else child.kill("SIGKILL");
      setTimeout(() => finish(null, "startup timeout"), 2000).unref();
    }, 20_000);
  });
}

const sandboxParent = option("--sandbox-root") ? path.resolve(option("--sandbox-root")) : tmpdir();
await mkdir(sandboxParent, { recursive: true });
const auditRoot = await mkdtemp(path.join(sandboxParent, "uking-agent-audit-"));
const reports = [];
for (const cmd of commands.filter((cmd) => !option("--only") || option("--only").split(",").includes(cmd))) {
  const executable = resolve(cmd);
  if (!executable) { reports.push({ command: cmd, registered: registered.has(cmd), installed: false, state: "not_installed" }); continue; }
  const sandbox = path.join(auditRoot, cmd);
  for (const dir of ["", "tmp", "AppData", "LocalAppData", "config", "data", "cache", "state"]) await mkdir(path.join(sandbox, dir), { recursive: true });
  // Test the cold database before --help, which itself can initialize MiMo's DB.
  const coldDefault = cmd === "mimo" && flags.includes("--mimo-db") && flags.includes("--compare-default-temp")
    ? await run(executable, ["db", "SELECT 1"], sandbox, tmpdir()) : undefined;
  const version = await run(executable, ["--version"], sandbox);
  const help = await run(executable, ["--help"], sandbox);
  const smoke = cmd === "mimo" && flags.includes("--mimo-db") ? {
    ...(coldDefault ? { cold_default_temp: coldDefault } : {}),
    isolated_temp: await run(executable, ["db", "SELECT 1"], sandbox),
    ...(flags.includes("--compare-default-temp") ? { default_temp: await run(executable, ["db", "SELECT 1"], sandbox, tmpdir()) } : {}),
  } : undefined;
  const bunShell = /bun is a fast javascript runtime/i.test(help.output);
  reports.push({ command: cmd, registered: registered.has(cmd), installed: true, path: safeText(executable),
    state: version.code === 0 && help.code === 0 && !bunShell ? "startup_ok" : "startup_failed",
    version, help, ...(smoke ? { smoke } : {}), model_access: "not_tested" });
  process.stderr.write(`${cmd}: ${reports.at(-1).state}\n`);
}
const report = { generated_at: new Date().toISOString(), scope: "isolated version/help only; no model request or login", reports };
if (option("--out")) await writeFile(path.resolve(option("--out")), JSON.stringify(report, null, 2) + "\n");
console.log(JSON.stringify(report, null, 2));
