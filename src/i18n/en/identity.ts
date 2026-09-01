/**
 * 「我的 U-King」页（身份 + 给 AI 的说明书）的英文覆盖。
 *
 * key = 组件里的中文原文，缺失自动回退中文。补新文案时用
 * `node scripts/extract-i18n-keys.mjs src/Identity.tsx` 对一遍，
 * 别靠肉眼找 —— 漏翻是静默的（回退中文），没人会收到报错。
 *
 * 术语约定（和 en/misc.ts、en/settings.ts 保持一致）：
 * 说明书 = manual · 指针 = pointer · 凭据 = credential · 挂上 = link · 明文 = plain text
 */
export const identity: Record<string, string> = {
  // —— 页头 ——
  "让 AI 认识 U-King": "Let AIs discover U-King",
  "给这台机器上的 U-King 起个名字、定个职责，并生成一份「给 AI 看的说明书」—— 让客户自己装的 Claude Code、Codex 或任何别家 AI，都能发现并调用它。":
    "Give the U-King on this machine a name and a job, then publish a manual written for AIs — so Claude Code, Codex or any other AI installed here can discover and call it.",
  "读取中…": "Loading…",

  // —— 健康横幅 ——
  "说明书已就位，别的 AI 能发现这台机器上的 U-King":
    "Manual is live — other AIs can discover the U-King on this machine",
  "说明书还没生成 —— 别的 AI 现在发现不了我们":
    "Manual not generated yet — no other AI can discover us right now",
  "重新生成说明书": "Regenerate manual",
  "生成说明书": "Generate manual",
  "看看 AI 会读到什么": "See what an AI would read",
  "全量版": "Full version",
  "打开目录": "Open folder",
  "说明书是从动作表现场编译出来的，不是手写文档 —— 升级 U-King 后点一下「重新生成」就跟上，永远不会和实际能力对不上。":
    "The manual is compiled from the live action table, not hand-written — after upgrading U-King just hit Regenerate and it stays in sync with what U-King can actually do.",

  // —— 让 AI 发现我 ——
  "让 AI 发现我": "Let other AIs discover me",
  "在这些工具的全局记忆文件里加一行指针，指向 ~/.uking/llms.txt —— 否则它们不会自己想到去读。我们只加带标记的一小块，你原有的内容一个字都不动，随时可撤销（首次改动会自动留一份 .uking-bak）。":
    "Add a one-line pointer to ~/.uking/llms.txt inside these tools' global memory files — otherwise they will never think to read it. We only insert a small marked block; your own content is left untouched and it can be undone at any time (a .uking-bak backup is kept on first change).",
  "已挂 {n}": "{n} linked",
  "已挂": "Linked",
  "撤销": "Unlink",
  "挂上": "Link",
  "全部挂上": "Link all",
  "指针只有 3 行 —— 它会进每个会话的上下文，所以刻意写得很短，详细内容都在 llms.txt 里按需读。":
    "The pointer is only 3 lines — it lands in every session's context, so it is deliberately tiny; the details live in llms.txt and are read on demand.",

  // —— 身份 ——
  "身份": "Identity",
  "明文保存在 identity.json，会原样写进说明书 —— 这是你对所有 AI 说话的地方。":
    "Stored as plain text in identity.json and copied verbatim into the manual — this is where you speak to every AI.",
  "明文": "Plain text",
  "它叫什么": "What it is called",
  "怎么称呼你": "How to address you",
  "比如：李工": "e.g. Alex",
  "职责（一句话）": "Job (one line)",
  "比如：负责海事业务文档和数据整理": "e.g. maritime paperwork and data tidying",
  "对所有 AI 的补充说明": "Extra notes for every AI",
  "比如：我的项目都在 D:\\\\work，别动 C 盘；文档一律用中文。":
    "e.g. my projects live on D:\\\\work, stay off drive C; always write docs in Chinese.",
  "保存": "Save",
  "有未保存的修改": "Unsaved changes",

  // —— 凭据 ——
  "凭据": "Credentials",
  "值只存在本机的 secrets.json。说明书里只写「配了哪些 Key」，永远不写值 —— 你可以点上面的「看看 AI 会读到什么」自己搜一遍验证。":
    "Values stay in secrets.json on this machine only. The manual lists which keys exist, never their values — hit \"See what an AI would read\" above and search it yourself to verify.",
  "私密": "Private",
  "已配": "Set",
  "空": "Empty",
  "删除": "Delete",
  "名称": "Name",
  "值": "Value",
  "添加": "Add",

  // —— 预览 ——
  "llms-full.txt（全量版）": "llms-full.txt (full version)",
  "llms.txt（AI 会读到的内容）": "llms.txt (what an AI reads)",
  "收起": "Collapse",

  // —— toast ——
  "读取身份失败: {e}": "Failed to read identity: {e}",
  "已保存，说明书同步更新了": "Saved — the manual was refreshed too",
  "保存失败: {e}": "Save failed: {e}",
  "说明书已重新生成": "Manual regenerated",
  "生成失败: {e}": "Generation failed: {e}",
  "凭据已保存（值只存本机，不进说明书）":
    "Credential saved (the value stays on this machine and never enters the manual)",
  "凭据已删除": "Credential deleted",
  "操作失败: {e}": "Failed: {e}",
  "挂上了 —— 那些 AI 下次开会话就知道有个 U-King 能用":
    "Linked — those AIs will know a U-King is available from their next session",
  "已撤销，你自己的内容原样没动": "Unlinked — your own content was left exactly as it was",
  "读不到说明书，先点「生成说明书」: {e}": "Cannot read the manual — hit Generate manual first: {e}",
};
