// historyTrim.ts 边界测试：node --experimental-strip-types 直跑 TS 纯函数，零依赖。
// 用例含 sol 终审/复审的全部指控：单条超长、systemText 计入预算、说明占位不破上界。
import { strict as assert } from "node:assert";
import { trimHistoryForPayload, CHAT_HISTORY_CHAR_BUDGET } from "../src/opencodex/historyTrim.ts";

let groups = 0;
const totalChars = (out) => out.reduce((n, m) => n + m.content.length, 0);

// ① 未超预算：原样直发
{
    const msgs = [{ role: "user", content: "a".repeat(100) }, { role: "assistant", content: "b".repeat(100) }];
    const out = trimHistoryForPayload(msgs, SYS());
    assert.equal(out.length, 3);
    assert.equal(out[0].content, SYS());
    assert.equal(out[1].content, msgs[0].content);
    groups++;
}
function SYS() { return "你是助手"; }

// ② 超预算常规三尺寸：掐头留尾 + 折叠说明 + 最新消息完整保留 + 硬上界
for (const step of [117, 401, 977]) {
    const sys = SYS();
    const msgs = [];
    for (let i = 0; i < 300; i++) msgs.push({ role: i % 2 ? "assistant" : "user", content: String(i).padStart(4, "0").repeat(step) });
    const out = trimHistoryForPayload(msgs, sys);
    assert.ok(totalChars(out) <= CHAT_HISTORY_CHAR_BUDGET, `step=${step} 硬上界被突破: ${totalChars(out)}`);
    assert.ok(out.some(m => m.role === "system" && m.content.includes("折叠")), `step=${step} 该有折叠说明`);
    assert.equal(out.at(-1).content, msgs[299].content, `step=${step} 最新消息应完整保留`);
    assert.ok(out[1].content.startsWith("0000"), `step=${step} 头部首条保留`);
}
groups++;

// ③ 🔴 sol P1 原始用例：单条超长消息（旧实现先入列后记账会无限突破）
{
    const monster = "M".repeat(5_000_000);
    const msgs = [
        { role: "user", content: "head" },
        { role: "assistant", content: "tail-prev" },
        { role: "user", content: monster },
    ];
    const out = trimHistoryForPayload(msgs, SYS());
    assert.ok(totalChars(out) <= CHAT_HISTORY_CHAR_BUDGET, `单条超长突破硬上界: ${totalChars(out)}`);
    assert.ok(totalChars(out) > CHAT_HISTORY_CHAR_BUDGET - 500, "截断条应吃满剩余额度而非缩水");
    assert.ok(out.some(m => m.content.includes("MMMMMM")), "怪物条应被截断保留而非整条丢弃");
}

// 同族混排：大消息+怪物+普通收尾
{
    const msgs = [
        { role: "user", content: "k".repeat(119_999) },
        { role: "user", content: "M".repeat(5_000_000) },
        { role: "assistant", content: "z".repeat(30_000) },
        { role: "user", content: "final question" },
    ];
    const out = trimHistoryForPayload(msgs, SYS());
    assert.ok(totalChars(out) <= CHAT_HISTORY_CHAR_BUDGET, `混排突破硬上界: ${totalChars(out)}`);
}
groups++;

// ④ 全是超长单条：头部一条装不下也要保住尾部与说明
{
    const msgs = Array.from({ length: 8 }, (_, i) => ({ role: i % 2 ? "assistant" : "user", content: "x".repeat(90_000) }));
    const out = trimHistoryForPayload(msgs, SYS());
    assert.ok(totalChars(out) <= CHAT_HISTORY_CHAR_BUDGET, `硬上界被突破: ${totalChars(out)}`);
    assert.ok(out.some(m => m.role === "system" && m.content.includes("折叠")), "该有折叠说明");
    groups++;
}

// ⑤ 🔴 sol 复审补充：超长 systemText 必须计入预算
for (const sysLen of [30_000, 119_900, 120_001]) {
    const sys = "S".repeat(sysLen);
    const msgs = [{ role: "user", content: "u".repeat(2_000) }, { role: "assistant", content: "a".repeat(2_000) }];
    const out = trimHistoryForPayload(msgs, sys);
    assert.ok(totalChars(out) <= CHAT_HISTORY_CHAR_BUDGET, `sysLen=${sysLen} 连 system 一起突破硬上界: ${totalChars(out)}`);
}
// system 就已超预算：正文一条都进不去，但绝不能崩、也不能为负
{
    const out = trimHistoryForPayload([{ role: "user", content: "q".repeat(500) }], "Z".repeat(200_000));
    assert.ok(totalChars(out) <= CHAT_HISTORY_CHAR_BUDGET);
    assert.ok(out.length >= 1 && out[0].role === "system");
}
groups++;

// ⑥ 随机化模糊：150 组随机长度/角色/system，硬上界恒成立 + 最新消息前缀保留
{
    let seed = 20260827;
    const rnd = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff;
    for (let t = 0; t < 150; t++) {
        const n = 2 + Math.floor(rnd() * 60);
        const sys = "S".repeat(Math.floor(rnd() * rnd() * 300_000));
        const msgs = Array.from({ length: n }, (_, i) => ({
            role: i % 2 ? "assistant" : "user",
            content: String.fromCharCode(97 + Math.floor(rnd() * 26)).repeat(1 + Math.floor(rnd() * rnd() * 400_000)),
        }));
        if (msgs.reduce((nn, m) => nn + m.content.length, 0) + sys.length <= CHAT_HISTORY_CHAR_BUDGET) continue;
        const out = trimHistoryForPayload(msgs, sys);
        assert.ok(totalChars(out) <= CHAT_HISTORY_CHAR_BUDGET, `t=${t} 模糊突破: ${totalChars(out)}`);
        // system 自己就超预算的退化场景：输出只剩截断的 system，没有正文可言
        if (sys.length > CHAT_HISTORY_CHAR_BUDGET) continue;
        assert.ok(out.length > 1, `t=${t} 正文不该全丢`);
        // 最新消息（或其截断前缀）必须在末尾——真断言，不许恒真子句
        const lastSrc = msgs.at(-1).content;
        const tailMsg = out.at(-1);
        const keep = Math.min(50, lastSrc.length, tailMsg.content.length);
        assert.ok(lastSrc.startsWith(tailMsg.content.slice(0, keep)), `t=${t} 末尾不是最新消息/其截断`);
    }
    groups++;
}

console.log(`historyTrim 边界测试: ${groups}/6 组全绿 (预算=${CHAT_HISTORY_CHAR_BUDGET})`);
