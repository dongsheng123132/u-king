/**
 * 闸门：「某个 AI 工具是否存在/怎么探测/怎么显示」的单一真相源在 Rust 侧
 * （`src-tauri/src/tools.rs` 的 `TOOL_SPECS`），但**终端应用注册表**
 * （`src/opencodex/apps.ts` 的 `TUI_APPS`）是前端独立维护的另一份数据 —— 侧栏图标、
 * 启动提示词、切驱动 target 这些跟 `TOOL_SPECS` 描述的是同一批事实，抄了第二遍。
 *
 * ## 为什么不把 TOOL_SPECS 直接吐给前端 import（Phase C，2026-09-04）
 * `TOOL_SPECS` 是 Rust `const`，没有天然的「导出成 JSON 给前端读」路径；就算加一个
 * Tauri 命令把它序列化下发，也只能在运行时用，`apps.ts` 里那些字段（`prompts`/
 * `launchLabel`/`external`/`hidden` 这些纯 UI 决策）本来就不该、也不可能从后端表
 * 派生——它们是产品/体验判断，不是「这个工具存不存在」。所以继续两边各自维护，
 * 只用这条构建期比对脚本钉住**两边都有、且理应一致**的那几个字段：
 * 一边加了新工具或改了 target 忘了同步另一边，构建就当场报错，不用等到运行时
 * 才发现某个工具切驱动切不动。
 *
 * ## 判据
 *  ① `TUI_APPS[].toolId` 必须能在 `TOOL_SPECS[].id` 里找到（`toolId` 字段本身在
 *     `apps.ts` 里就注明是「tools.rs 里的 ToolInfo.id；注意 ≠ id」——两边的连接键
 *     是 `toolId`，不是 `TuiAppId` 那个路由用的 `id`，因为好几个工具路由 id 和
 *     `tools.rs` 的 id 对不上（`codex-cli`↔`codex`、`qwen`↔`qwen-code`）。
 *  ② 找到对应项后，`TOOL_SPECS.cmd` 必须等于 `TUI_APPS.prompts[0].cmd` 的**第一个词**
 *     （不是整句相等——像 `openclaw` 那样第一条提示词是
 *     `"openclaw gateway run --allow-unconfigured --port 18789"`，只有第一个 token
 *     才是可执行文件名，后面是参数）。
 *  ③ `TOOL_SPECS.config_target` 必须等于 `TUI_APPS.configTargets[0]`（有 configTargets
 *     才比；`TOOL_SPECS.config_target` 是 `None` 但前端仍声明了 configTargets，或反过来，
 *     都算不一致）。
 *  ④ `TOOL_SPECS` 里 `in_list_tools`/`cmd` 都非空、看起来像「应该有个 TUI 入口」的项，
 *     如果谁都没有 `toolId` 指过来，报「只在 TOOL_SPECS 里，apps.ts 没有对应 TUI 入口」——
 *     除了下面 `ALLOWED_SPEC_WITHOUT_TUI_APP` 里显式豁免的（GUI 复用同一份 cmd/target
 *     的重复项、纯下载类/体检类工具，本来就不该有终端应用）。
 *
 * 用法：node scripts/check-tool-registry-sync.mjs
 */
import { readFileSync } from "node:fs";

const RS_FILE = "src-tauri/src/tools.rs";
const TS_FILE = "src/opencodex/apps.ts";

/**
 * `TOOL_SPECS` 里没有 `toolId` 对应到 `TUI_APPS` 也完全正常的项，逐条写明理由：
 * - `clawx`：桌面 GUI，复用的是 `openclaw`（TUI_APPS 里那条）同一份 cmd("openclaw")/
 *   config_target("clawx")，只是多一个「桌面装没装」的判据，不该也没有第二个 TUI 入口。
 * - `harness-doctor`/`obsidian`/`uu-remote`/`codex-app`/`open365`/`hermes-app`/`uu-switch`：
 *   要么是纯体检工具，要么是纯 GUI/下载类应用，压根不是「终端里跑」的东西，
 *   `apps.ts` 只登记 TUI（终端）应用，它们从设计上就不会出现在那张表里。
 */
const ALLOWED_SPEC_WITHOUT_TUI_APP = new Set([
  "clawx",
  "harness-doctor",
  "obsidian",
  "uu-remote",
  "codex-app",
  "open365",
  "hermes-app",
  "uu-switch",
]);

/** 从 `start` 位置的 `[` 开始，返回与之配对的 `]` 的下标（简单方括号计数）。 */
function matchBracket(src, start, open, close) {
  let depth = 0;
  for (let i = start; i < src.length; i++) {
    if (src[i] === open) depth++;
    else if (src[i] === close) {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/** 把 `body` 里用花括号包起来的顶层对象/结构体字面量逐个切出来（不管前缀是什么关键字）。 */
function splitTopLevelBraceBlocks(body) {
  const blocks = [];
  let depth = 0;
  let start = -1;
  for (let i = 0; i < body.length; i++) {
    if (body[i] === "{") {
      if (depth === 0) start = i;
      depth++;
    } else if (body[i] === "}") {
      depth--;
      if (depth === 0 && start >= 0) {
        blocks.push(body.slice(start, i + 1));
        start = -1;
      }
    }
  }
  return blocks;
}

function parseToolSpecs(src) {
  const marker = "pub const TOOL_SPECS: &[ToolSpec] = &[";
  const start = src.indexOf(marker);
  if (start < 0) {
    console.error(`❌ ${RS_FILE} 里找不到 \`${marker}\``);
    process.exit(1);
  }
  const arrayStart = start + marker.length - 1; // 指向那个 `[`
  const arrayEnd = matchBracket(src, arrayStart, "[", "]");
  if (arrayEnd < 0) {
    console.error(`❌ ${RS_FILE} 里 TOOL_SPECS 数组没找到匹配的结尾`);
    process.exit(1);
  }
  const body = src.slice(arrayStart + 1, arrayEnd);
  const blocks = splitTopLevelBraceBlocks(body).filter((b) => /\bid\s*:/.test(b));

  return blocks.map((block) => {
    const id = block.match(/\bid:\s*"([^"]*)"/)?.[1] ?? null;
    const cmd = block.match(/\bcmd:\s*"([^"]*)"/)?.[1] ?? null;
    const configTarget = block.match(/config_target:\s*Some\("([^"]*)"\)/)?.[1] ?? null;
    const inListTools = /in_list_tools:\s*true/.test(block);
    return { id, cmd, configTarget, inListTools };
  });
}

function parseTuiApps(src) {
  const marker = "export const TUI_APPS: TuiApp[] = [";
  const start = src.indexOf(marker);
  if (start < 0) {
    console.error(`❌ ${TS_FILE} 里找不到 \`${marker}\``);
    process.exit(1);
  }
  const arrayStart = start + marker.length - 1; // 指向那个 `[`
  const arrayEnd = matchBracket(src, arrayStart, "[", "]");
  if (arrayEnd < 0) {
    console.error(`❌ ${TS_FILE} 里 TUI_APPS 数组没找到匹配的结尾`);
    process.exit(1);
  }
  const body = src.slice(arrayStart + 1, arrayEnd);
  const blocks = splitTopLevelBraceBlocks(body).filter((b) => /\btoolId\s*:/.test(b));

  return blocks.map((block) => {
    const id = block.match(/\bid:\s*"([^"]*)"/)?.[1] ?? null;
    const toolId = block.match(/\btoolId:\s*"([^"]*)"/)?.[1] ?? null;

    // prompts 数组的第一个 { ... } 对象里的 cmd 字段。
    let firstPromptCmd = null;
    const promptsIdx = block.indexOf("prompts:");
    if (promptsIdx >= 0) {
      const bracketStart = block.indexOf("[", promptsIdx);
      const bracketEnd = matchBracket(block, bracketStart, "[", "]");
      if (bracketStart >= 0 && bracketEnd > bracketStart) {
        const promptsBody = block.slice(bracketStart + 1, bracketEnd);
        const firstObj = splitTopLevelBraceBlocks(promptsBody)[0];
        if (firstObj) {
          firstPromptCmd = firstObj.match(/cmd:\s*"((?:[^"\\]|\\.)*)"/)?.[1] ?? null;
        }
      }
    }

    // configTargets 数组的第一个字符串。
    let firstConfigTarget = null;
    const ctIdx = block.indexOf("configTargets:");
    if (ctIdx >= 0) {
      const m = block.slice(ctIdx).match(/configTargets:\s*\[\s*"([^"]*)"/);
      if (m) firstConfigTarget = m[1];
    }

    return { id, toolId, firstPromptCmd, firstConfigTarget };
  });
}

const rsSrc = readFileSync(RS_FILE, "utf8");
const tsSrc = readFileSync(TS_FILE, "utf8");

const specs = parseToolSpecs(rsSrc);
const apps = parseTuiApps(tsSrc);

console.log(`${RS_FILE} 的 TOOL_SPECS: ${specs.length} 条`);
console.log(`${TS_FILE} 的 TUI_APPS: ${apps.length} 条`);

let bad = 0;
const specById = new Map(specs.map((s) => [s.id, s]));
const matchedSpecIds = new Set();

for (const app of apps) {
  const spec = specById.get(app.toolId);
  if (!spec) {
    console.error(
      `\n❌ TUI_APPS「${app.id}」的 toolId="${app.toolId}" 在 ${RS_FILE} 的 TOOL_SPECS 里找不到同名 id`,
    );
    bad++;
    continue;
  }
  matchedSpecIds.add(spec.id);

  if (spec.cmd) {
    const firstToken = (app.firstPromptCmd ?? "").trim().split(/\s+/)[0] ?? "";
    if (firstToken !== spec.cmd) {
      console.error(
        `\n❌ 「${app.id}」(toolId="${app.toolId}") 的 cmd 不一致：` +
          `TOOL_SPECS.cmd="${spec.cmd}" TUI_APPS.prompts[0].cmd 第一个词="${firstToken}"` +
          ` (完整 prompts[0].cmd="${app.firstPromptCmd ?? "(未设置)"}")`,
      );
      bad++;
    }
  }

  const specTarget = spec.configTarget; // string | null
  const appTarget = app.firstConfigTarget; // string | null
  if (specTarget !== appTarget) {
    console.error(
      `\n❌ 「${app.id}」(toolId="${app.toolId}") 的 configTarget 不一致：` +
        `TOOL_SPECS.config_target=${specTarget === null ? "None" : `"${specTarget}"`} ` +
        `TUI_APPS.configTargets[0]=${appTarget === null ? "(未设置)" : `"${appTarget}"`}`,
    );
    bad++;
  }
}

for (const spec of specs) {
  if (matchedSpecIds.has(spec.id)) continue;
  if (ALLOWED_SPEC_WITHOUT_TUI_APP.has(spec.id)) continue;
  if (!spec.cmd) continue; // 没有可执行文件名的，本来就不是终端应用候选
  console.error(
    `\n❌ TOOL_SPECS 里的「${spec.id}」(cmd="${spec.cmd}") 在 ${TS_FILE} 的 TUI_APPS 里没有 ` +
      `toolId="${spec.id}" 的对应项，也不在 ALLOWED_SPEC_WITHOUT_TUI_APP 豁免名单里 —— ` +
      `要么补一个 TUI_APPS 条目，要么把它加进脚本头部的豁免名单并写明理由。`,
  );
  bad++;
}

if (bad) {
  console.error(`\n共 ${bad} 处不一致。改工具注册表时 tools.rs 和 apps.ts 要一起看。`);
  process.exit(1);
}
console.log("\n✅ TOOL_SPECS 和 TUI_APPS 在 id/cmd/config_target 上一致。");
