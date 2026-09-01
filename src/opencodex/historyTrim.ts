/**
 * u-chat 跨轮历史裁剪（纯函数，从 Chat.tsx 的 toApiMessages 抽出以便直跑边界测试）。
 *
 * 为什么必须有：每轮都把全部历史打包发后端，长会话成本随轮数单调上涨
 * （2026-08-25 客户案例：Claude Code 侧一个 8.5h 会话滚到 102 万 token、¥92/天）。
 *
 * 🔴 硬上界契约（sol 三轮实抓后立）：输出总字符**永不超过** CHAT_HISTORY_CHAR_BUDGET，
 * 从首位 system 就开始记账；system 自己超预算就截 system。
 *
 * 🧮 空间分配采用「分池预留制」（三轮实抓「跨阶段总额重算必错」教训后定）：
 *   正文额度 = 预算 − len(system)；说明池 = min(160, 正文额度的10%)；
 *   尾部只准花「正文额度 − 说明池」。各池独立花销，任何阶段都不回头重算总量，
 *   上界在结构上成立而不是靠每次小心。优先级：最新消息必在场（完整或前缀），
 *   折叠说明是锦上添花，池子被 system 挤没了就不说。
 */

export const CHAT_HISTORY_CHAR_BUDGET = 120_000;

/** 折叠说明最长这么多字符（全文约 85，留余量）。 */
export const FOLD_NOTE_CAP = 160;

/** 说明池小于这个数就干脆不说——半句话没信息量。 */
const NOTE_MIN_POOL = 48;

export type HistoryMsg = { role: string; content: string };

/**
 * 发送前的历史裁剪。输入「正文消息列表」（不含 system）与 system 提示词，
 * 输出裁剪后的完整 payload 消息列表（未超预算时与输入等价）。
 */
export function trimHistoryForPayload(
    msgs: HistoryMsg[],
    systemText: string,
): { role: string; content: string }[] {
    const out: { role: string; content: string }[] = [];
    // 硬上界从 system 开始记账。
    if (systemText.length > CHAT_HISTORY_CHAR_BUDGET) {
        out.push({ role: "system", content: systemText.slice(0, CHAT_HISTORY_CHAR_BUDGET) });
        return out;
    }
    out.push({ role: "system", content: systemText });

    const bodyRoom = CHAT_HISTORY_CHAR_BUDGET - systemText.length;

    const total = msgs.reduce((n, m) => n + m.content.length, 0);
    if (total <= bodyRoom) {
        for (const m of msgs) out.push({ role: m.role, content: m.content });
        return out;
    }

    // 分池：说明池上限 160 且最多占正文额度的一成；剩下全是头尾的活动空间。
    const notePoolCap = Math.min(FOLD_NOTE_CAP, Math.floor(bodyRoom * 0.1));
    const activeRoom = bodyRoom - notePoolCap;

    // 头部：最早的消息按顺序装满活动空间的 20%（开局的设定/结论最重要）
    const headBudget = Math.floor(activeRoom * 0.2);
    let used = 0;
    let splitAt = 0;
    for (let i = 0; i < msgs.length; i++) {
        if (used + msgs[i].content.length > headBudget) break;
        used += msgs[i].content.length;
        splitAt = i + 1;
    }
    for (let i = 0; i < splitAt; i++) out.push({ role: msgs[i].role, content: msgs[i].content });

    // 尾部：从最新往回吃满「活动空间 − 头部」；整条放得下放整条，
    // 放不下截断吃满剩余并停止——最新消息保证以完整或前缀形式在场。
    let tailBudget = activeRoom - used;
    const tail: HistoryMsg[] = [];
    for (let i = msgs.length - 1; i >= splitAt && tailBudget > 0; i--) {
        const c = msgs[i].content;
        if (c.length <= tailBudget) {
            tail.unshift({ role: msgs[i].role, content: c });
            tailBudget -= c.length;
        } else {
            tail.unshift({ role: msgs[i].role, content: c.slice(0, tailBudget) });
            break;
        }
    }
    // 衔接自然：开头不是 user 的整条丢回已折池（它的字符留在说明池里无关紧要——
    // 说明池是独立的，不参与这里的花销；直接放弃这些余量，上界只会更稳）。
    while (tail.length > 1 && tail[0].role !== "user") tail.shift();

    // 折叠说明：只花自己的池子。正文没有被折的消息、或池子太小放不下一段完整话，就不说。
    const folded = Math.max(msgs.length - splitAt - tail.length, 0);
    if (folded > 0 && notePoolCap >= NOTE_MIN_POOL) {
        const full = `（自动省额度：本会话中间较旧的 ${folded} 条消息已被折叠，仅保留最早与最近的部分。若需要早前细节请用户重发。）`;
        const note =
            full.length <= notePoolCap ? full : full.slice(0, Math.min(FOLD_NOTE_CAP, notePoolCap) - 1) + "…";
        out.push({ role: "system", content: note });
    }
    for (const m of tail) out.push(m);
    return out;
}
