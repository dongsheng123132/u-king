/**
 * 「启动工具前选文件夹」的最近文件夹记忆 —— `src/toolhub/recentFolders.ts` 的纯逻辑断言，无 DOM。
 *
 *     node scripts/check-recent-folders.mts
 *
 * 本仓没有前端测试框架（见 check-yield-chain.mts 头注释），所以这是一条可被 node 直接跑的断言
 * 脚本：Node >= 22.18 / 24 自带类型剥离，能直接 import `.ts`（recentFolders.ts 只用可擦除语法）；
 * 更老的 Node 用 `npx --yes tsx scripts/check-recent-folders.mts`。
 * 退出码：0 = 全过；1 = 有失败。手动跑，没挂进 `pnpm build`（理由同 check-yield-chain.mts）。
 */
import {
  MAX_RECENT,
  RECENT_FOLDERS_VERSION,
  defaultFor,
  dropMissing,
  emptyState,
  folderName,
  listRecent,
  parseState,
  pathKey,
  recordUse,
  samePath,
} from "../src/toolhub/recentFolders.ts";

let fail = 0;
let total = 0;
const eq = (name: string, got: unknown, want: unknown) => {
  total++;
  const g = JSON.stringify(got);
  const w = JSON.stringify(want);
  const ok = g === w;
  if (!ok) fail++;
  console.log(`${ok ? "  ok  " : "  FAIL"}  ${name}${ok ? "" : `\n         got =${g}\n         want=${w}`}`);
};

console.log("\n1. 路径比较 / 显示名");
eq("同一路径 斜杠方向 + 末尾斜杠 + 盘符大小写 视为相同", samePath("D:\\Work\\Proj\\", "d:/work/proj"), true);
eq("POSIX 路径区分大小写", samePath("/home/me/Proj", "/home/me/proj"), false);
eq("盘符根不丢斜杠", pathKey("D:\\"), "d:/");
eq("盘符根 D: 与 D:\\ 相同", samePath("D:", "D:\\"), true);
eq("UNC 路径大小写不敏感", samePath("\\\\srv\\Share\\A", "\\\\SRV\\share\\a\\"), true);
eq("folderName 取最后一段", folderName("C:\\Users\\me\\项目 测试"), "项目 测试");
eq("folderName 末尾斜杠", folderName("/Users/me/code/"), "code");
eq("folderName 盘符根原样显示", folderName("D:\\"), "D:\\");

console.log("\n2. recordUse：最新在前、去重、封顶");
{
  let s = emptyState();
  s = recordUse(s, "claude-code", "D:\\a", 1);
  s = recordUse(s, "codex", "D:\\b", 2);
  s = recordUse(s, "claude-code", "d:/A/", 3); // 同一个文件夹换了写法 → 去重并提到最前
  eq("去重后只剩两项", s.items.length, 2);
  eq("最新使用的在最前（保留最近一次的写法）", s.items.map((x) => x.path), ["d:/A/", "D:\\b"]);
  eq("空路径被忽略", recordUse(s, "pi", "   ", 9), s);
  let big = emptyState();
  for (let i = 0; i < MAX_RECENT + 5; i++) big = recordUse(big, `t${i}`, `D:\\p${i}`, i);
  eq(`封顶 ${MAX_RECENT} 项`, big.items.length, MAX_RECENT);
  eq("被挤出列表的项，指向它的 lastByTool 一并清掉", big.lastByTool["t0"], undefined);
  eq("最新的工具默认项保留", big.lastByTool[`t${MAX_RECENT + 4}`], `D:\\p${MAX_RECENT + 4}`);
  eq("listRecent 按最近降序", listRecent(big)[0].path, `D:\\p${MAX_RECENT + 4}`);
}

console.log("\n3. defaultFor：该工具上次用的 > 全局最近 > null");
{
  let s = emptyState();
  eq("没有记录 → null", defaultFor(s, "claude-code"), null);
  s = recordUse(s, "claude-code", "D:\\proj-a", 1);
  s = recordUse(s, "codex", "D:\\proj-b", 2);
  eq("claude 上次用 proj-a（不是全局最近的 proj-b）", defaultFor(s, "claude-code"), "D:\\proj-a");
  eq("codex 上次用 proj-b", defaultFor(s, "codex"), "D:\\proj-b");
  eq("没用过的工具 → 全局最近一个", defaultFor(s, "pi"), "D:\\proj-b");
}

console.log("\n4. dropMissing：不存在的路径自动剔除");
{
  let s = emptyState();
  s = recordUse(s, "claude-code", "D:\\gone", 1);
  s = recordUse(s, "codex", "D:\\here", 2);
  const d = dropMissing(s, ["d:/GONE/"]);
  eq("列表里剔掉", d.items.map((x) => x.path), ["D:\\here"]);
  eq("该工具的默认项一并剔掉", d.lastByTool["claude-code"], undefined);
  eq("claude 退回全局最近", defaultFor(d, "claude-code"), "D:\\here");
  eq("没有要剔的 → 原对象", dropMissing(s, []), s);
}

console.log("\n5. parseState：宽松、不抛错");
eq("null → 空", parseState(null), emptyState());
eq("坏 JSON → 空", parseState("{not json"), emptyState());
eq("版本不认识 → 空（以后迁移从这里接）", parseState(JSON.stringify({ v: 99, items: [{ path: "D:\\a", usedAt: 1 }] })), emptyState());
{
  const raw = JSON.stringify({
    v: RECENT_FOLDERS_VERSION,
    items: [
      { path: "D:\\old", usedAt: 1 },
      { path: 123, usedAt: 5 },
      { path: "D:\\new", usedAt: 9 },
      { path: "d:/NEW/", usedAt: 3 },
      { path: "", usedAt: 4 },
    ],
    lastByTool: { claude: "D:\\new", bad: 5 },
  });
  const s = parseState(raw);
  eq("坏项被丢、重复项只留一个、按时间降序", s.items.map((x) => x.path), ["D:\\new", "D:\\old"]);
  eq("lastByTool 只留字符串", s.lastByTool, { claude: "D:\\new" });
}

console.log(`\n判决 ${total - fail}/${total}`);
process.exit(fail ? 1 : 0);
