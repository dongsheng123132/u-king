/**
 * 跑道：打中文时 U-CLI 会不会把**拼音原文**也发给 PTY（= 客户看到的「重复打字」）。
 *
 * ## 症状（客户截图，2026-08-19）
 * 在 U-CLI 里跑 Claude Code 打中文，输入行里出现 `chong'zhi 充值` ——
 * **拼音原文和转换后的中文同时进去了**，还夹着「键 值」「+ 备 包」这类碎片。
 *
 * ★ 这不是「每个键发两遍」（那会变成「我我们们」）。是**组字过程中的预览文本也被当数据发了出去**，
 *   末尾提交时又发一遍。两者拼在一起，看起来就像整句重复。
 *
 * ## 判据
 * 真实中文输入法打一个词，PTY **只该收到最终那个词**。组字期间的 `input` 事件
 * （`isComposing: true`）一个字节都不该出去。这里把 `term.onData` 全收下来对比。
 *
 * ## 为什么必须用真 xterm + 真事件
 * 这条链路全在浏览器的 composition 事件语义里，抄一份实现来测等于测了个寂寞
 * （同 `check-term-ime.mjs` 的理由）。所以吃真的 `@xterm/xterm`，用真的
 * `CompositionEvent` / `InputEvent` 走真的监听。
 *
 * ## 同时回答「是不是我们的补丁造成的」
 * patched=false / true 各跑一遍。两边都漏 → 上游 xterm 的事；只有 patched 漏 →
 * `imeAnchor` 的 `scrollToBottom` 打断了 composition 状态机，得我们自己修。
 *
 * 用法：node scripts/check-term-ime-dup.mjs
 */
import { chromium } from "playwright";
import { transformWithEsbuild } from "vite";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");

const XTERM_JS = read("node_modules/@xterm/xterm/lib/xterm.js");
const XTERM_CSS = read("node_modules/@xterm/xterm/css/xterm.css");
const FIT_JS = read("node_modules/@xterm/addon-fit/lib/addon-fit.js");

const ANCHOR_JS = (
  await transformWithEsbuild(read("src/opencodex/term/imeAnchor.ts"), "imeAnchor.ts", {
    loader: "ts",
    format: "iife",
    globalName: "ImeAnchor",
  })
).code;

/** 要打的词，和它的拼音串（含隔音符 —— 客户截图里就是 `chong'zhi`）。 */
const WORD = "充值";
const PINYIN = "chong'zhi";

async function run(page, { patched, scrollUp, keyboardPick = true, aiBusy = false }) {
  return page.evaluate(
    async ([patched, scrollUp, word, pinyin, keyboardPick, aiBusy]) => {
      document.body.innerHTML = '<div id="t" style="width:820px;height:420px"></div>';
      const term = new window.Terminal({
        fontFamily: '"JetBrains Mono", ui-monospace, monospace',
        fontSize: 13,
        lineHeight: 1.2,
        scrollback: 5000,
        rows: 24,
        cols: 80,
      });
      const fit = new window.FitAddon.FitAddon();
      term.loadAddon(fit);
      term.open(document.getElementById("t"));
      if (patched) window.ImeAnchor.anchorImeToCursor(term);
      fit.fit();
      await new Promise((r) => setTimeout(r, 200));

      // 灌到有回滚 —— Claude Code 跑一会儿就是这个状态（也是 imeAnchor 要处理的处境）
      for (let i = 0; i < 96; i++) term.write(`line ${i} ................\r\n`);
      await new Promise((r) => setTimeout(r, 200));
      if (scrollUp) term.scrollLines(-scrollUp);
      await new Promise((r) => setTimeout(r, 120));

      // ★ 从这里开始收 —— 上面灌的输出不算
      const out = [];
      term.onData((d) => out.push(d));

      const ta = document.querySelector(".xterm-helper-textarea");
      ta.focus();

      // 真实 Windows 中文输入法的事件序列：
      //   compositionstart → (每敲一个字母：value 变长 + input(isComposing:true) + compositionupdate)
      //   → 选词 → compositionend(data=中文) → input(isComposing:false, value=中文)
      // 🔴 三个字段一个都不能省，否则跑道会骗你。xterm 的真实判据是：
      //    `e.data && "insertText" === e.inputType && (!e.composed || !this._keyDownSeen)`
      //    · 缺 `inputType` → 事件被整个忽略 → **假绿**（第一版栽在这，
      //      连 compositionstart 都删掉都照样绿）
      //    · 缺 `composed: true` → 守卫前半段恒真 → **假红**（第二版栽在这，
      //      「复现」出一个真实浏览器里不存在的双发）
      //    · 不发 keydown → `_keyDownSeen` 为假 → 同样假红
      //    真实用户输入的 input 事件一律 composed:true；组字期间的 keydown 是 keyCode 229。
      const fireInput = (value, composing) => {
        ta.value = value;
        ta.dispatchEvent(
          new InputEvent("input", {
            bubbles: true,
            composed: true,
            data: value,
            isComposing: composing,
            inputType: composing ? "insertCompositionText" : "insertText",
          }),
        );
      };
      /** 组字期间浏览器发的 keydown（keyCode 229 = 「交给输入法处理」）。 */
      const fireImeKeyDown = () =>
        ta.dispatchEvent(
          new KeyboardEvent("keydown", { bubbles: true, composed: true, keyCode: 229, key: "Process" }),
        );

      // 🔴 **组字期间上游还在刷输出** —— 客户原话：「不是每次都有，是在上面 AI 在干活的时候」。
      //    这是最关键的一个变量，第一版跑道在**安静的终端**里打字，所以复现不了。
      //    带输出时会出现两件安静时没有的事：① xterm 不停 refresh / _syncTextArea
      //    ② 我们的 imeAnchor 在每个 compositionupdate 都 scrollToBottom（= 又一次重绘）。
      let busy = null;
      if (aiBusy) {
        let n = 0;
        busy = setInterval(() => term.write(`\x1b[36m[ai]\x1b[0m 正在处理第 ${n++} 步…\r\n`), 12);
      }

      ta.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
      for (let i = 1; i <= pinyin.length; i++) {
        const buf = pinyin.slice(0, i);
        fireImeKeyDown(); // 每敲一个字母，浏览器先发 keydown(229)
        fireInput(buf, true);
        ta.dispatchEvent(new CompositionEvent("compositionupdate", { data: buf, bubbles: true }));
        await new Promise((r) => setTimeout(r, 30));
      }
      // 选词落地。keyboardPick=true 走「按数字键/空格选词」（有 keydown）；
      // false 走「**鼠标点候选条**选词」—— 那一下不产生 keydown，是真实存在的另一条路。
      if (keyboardPick) fireImeKeyDown();
      ta.dispatchEvent(new CompositionEvent("compositionend", { data: word, bubbles: true }));
      fireInput(word, false);
      await new Promise((r) => setTimeout(r, 250));
      if (busy) clearInterval(busy);

      const sent = out.join("");
      return {
        sent,
        chunks: out.length,
        // 拼音里独有的字母序列漏出去了没（用不会出现在中文里的片段判，别用单字母）
        leakedPinyin: sent.includes("chong") || sent.includes("zhi"),
        // 最终那个词到底送到没
        gotWord: sent.includes(word),
        // 送了几遍
        wordCount: sent.split(word).length - 1,
      };
    },
    [patched, scrollUp, WORD, PINYIN, keyboardPick, aiBusy],
  );
}

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1100, height: 700 } });
await page.addStyleTag({ content: XTERM_CSS });
await page.addScriptTag({ content: XTERM_JS });
await page.addScriptTag({ content: FIT_JS });
await page.addScriptTag({ content: ANCHOR_JS });

const cases = [
  { name: "原版 xterm（无我们的补丁）· 不回滚", patched: false, scrollUp: 0 },
  { name: "原版 xterm（无我们的补丁）· 回滚 8 行", patched: false, scrollUp: 8 },
  { name: "带 imeAnchor 补丁 · 不回滚", patched: true, scrollUp: 0 },
  { name: "带 imeAnchor 补丁 · 回滚 8 行", patched: true, scrollUp: 8 },
  // 🔴 鼠标点候选条选词 —— 这一下不产生 keydown，于是 xterm 那道 `!this._keyDownSeen`
  //    守卫失效。是真实用户行为，不是造出来的场景。
  { name: "★ 鼠标点候选条选词（无 keydown）· 带补丁", patched: true, scrollUp: 0, keyboardPick: false },
  // 🔴 客户的真实处境：AI 正在刷输出的同时打中文。「不是每次都有」= 有并发才有。
  { name: "★★ AI 正在刷输出时打字 · 原版 xterm", patched: false, scrollUp: 0, aiBusy: true },
  { name: "★★ AI 正在刷输出时打字 · 带 imeAnchor 补丁", patched: true, scrollUp: 0, aiBusy: true },
  { name: "★★ AI 正在刷输出 + 回滚 8 行 · 带补丁", patched: true, scrollUp: 8, aiBusy: true },
];

let bad = 0;
console.log(`打「${WORD}」（拼音 ${PINYIN}）—— PTY 只该收到「${WORD}」一次\n`);
for (const c of cases) {
  const r = await run(page, c);
  const ok = !r.leakedPinyin && r.gotWord && r.wordCount === 1;
  if (!ok) bad++;
  console.log(`${ok ? "✅" : "❌"} ${c.name}`);
  console.log(`     实收 ${JSON.stringify(r.sent)}（${r.chunks} 段）`);
  if (r.leakedPinyin) console.log(`     🔴 拼音原文漏进 PTY —— 这就是客户看到的「重复打字」`);
  if (!r.gotWord) console.log(`     🔴 最终的词没送出去`);
  else if (r.wordCount !== 1) console.log(`     🔴 最终的词送了 ${r.wordCount} 遍`);
}
await browser.close();

if (bad) {
  console.error(`\n❌ ${bad}/${cases.length} 种情形下组字内容会漏给 PTY。`);
  console.error(`   若「原版 xterm」那两条也红 → 是上游 xterm 的事，我们得在自己这层拦；`);
  console.error(`   若只有「带补丁」红 → imeAnchor 打断了 composition 状态机，改我们自己的。`);
  process.exit(1);
}
console.log("\n✅ 四种情形都只把最终的词发出去，没有组字内容泄漏。");
