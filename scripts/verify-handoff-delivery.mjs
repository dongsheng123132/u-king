#!/usr/bin/env node
/**
 * 护照交接的**投递语义**跑道 —— 不用起界面、不用点按钮。
 *
 * ## 为什么单开这一条
 * 「护照交接」这条链路上真正会出错的地方，多数不需要眼睛：
 * 一封信取走后还在不在（重挂载会不会重发）、回执是谁签的、
 * 拿不到正文时那段话敢不敢承认自己没带上下文。这些都是纯函数的事。
 * 剩下真要人看的只有「卡片上那行字长得对不对」——把它收缩到最小，
 * 才值得为它起一台干净机。
 *
 * ## 🔴 调真模块，不照抄
 * 直接 `import` 仓库里的 `src/opencodex/handoff.ts`（node 22 的
 * `--experimental-strip-types` 直接吃 TS）。跑道里自己重写一份 Map
 * 语义等于复制了第二份实现 —— 改坏 handoff.ts 它照样绿，那就是个摆设。
 *
 * 用法：node --experimental-strip-types scripts/verify-handoff-delivery.mjs
 * 退出码 0 = 全过；1 = 有断言没过（stdout 只出结论，细节走 stderr）。
 */
import {
  buildHandoffPrompt,
  deliver,
  deliveredPassport,
  onHandoffChange,
  queueHandoff,
  takeHandoff,
} from "../src/opencodex/handoff.ts";

const fails = [];
const ok = (name, cond, detail = "") => {
  if (cond) {
    process.stderr.write(`  ✓ ${name}\n`);
  } else {
    fails.push(`${name}${detail ? ` —— ${detail}` : ""}`);
    process.stderr.write(`  ✗ ${name} ${detail}\n`);
  }
};

const S = "sess-1";
const P = "TP-QA98-FIX1";

// ① 投递 → 收件人自取，拿到的就是投进去的那封。
queueHandoff(S, { passportId: P, engine: "claude", prompt: "hello" });
const got = takeHandoff(S);
ok("投进去的信能被收件人取到", got?.passportId === P && got?.engine === "claude");

// ② **取走即删**。会话切走再切回来会重挂载，第二次取必须为空 ——
//    否则用户会看见 AI 反复接手同一个任务（比不投递更糟）。
ok("取走即删：重挂载不会再发一遍", takeHandoff(S) === null);

// ③ 回执是收件人签的字。发出去之前，护照页不许显示「已送达」。
ok("没签字之前 = 没送达", deliveredPassport(S) === null);
deliver(S, P);
ok("签字之后才算送达", deliveredPassport(S) === P);

// ④ 同一个会话再接一张护照：旧回执必须清掉，否则新任务一投进去
//    界面就挂着上一次的「已送达」——把「还没发」画成了「已完成」。
queueHandoff(S, { passportId: "TP-OTHER", engine: "uking", prompt: "x" });
ok("重新交接会清掉上一次的回执", deliveredPassport(S) === null);
takeHandoff(S);

// ⑤ 订阅：护照页靠它把「投递中」翻成「已送达」。投递和签字都要通知，退订后不再收。
let hits = 0;
const off = onHandoffChange(() => hits++);
queueHandoff("sess-2", { passportId: "TP-X", engine: "codex", prompt: "y" });
deliver("sess-2", "TP-X");
const during = hits;
off();
deliver("sess-2", "TP-X");
ok("投递与签字都会通知订阅方", during >= 2, `收到 ${during} 次`);
ok("退订后不再收到通知", hits === during);

// ⑥ 正文来自后端。拿到 compiled 就**原样带上**，不另拼一份摘要
//    （拼第二份的那一刻它就会跟真状态漂开，而漂开的那次正好是出事那次）。
const compiled = "## 这个任务的当前状态\n**目标**：修完六项发布阻断";
const withCtx = buildHandoffPrompt(P, compiled);
ok("带上下文时护照号在", withCtx.includes(P));
ok("带上下文时后端正文被原样带上", withCtx.includes("修完六项发布阻断"));

// ⑦ **读不到正文时不许装作读到了**。只发护照号，并明说上下文没带上 ——
//    接手方据此知道要自己去读，而不是拿着半份状态开干。
for (const empty of [null, "", "   "]) {
  const bare = buildHandoffPrompt(P, empty);
  ok(
    `compiled=${JSON.stringify(empty)} 时明说没带上下文`,
    bare.includes(P) && bare.includes("没能读出护照正文"),
  );
}

if (fails.length) {
  console.log(JSON.stringify({ ok: false, failed: fails }, null, 1));
  process.exit(1);
}
console.log(JSON.stringify({ ok: true, checks: 12 }));
