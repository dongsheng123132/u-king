/**
 * 这条报错是「用户/环境自己能处理」的，还是真的软件 bug？
 *
 * 为什么要有这个判断：判错了两头都坏 ——
 *   · 判成 bug：客户看到「出图失败：user quota is not enough」这种半英文原文，
 *     不知道该干什么；同时一条非 bug 被自动上报，把 bug 仓库淹掉。
 *   · 判成用户问题：真崩溃被吞掉，我们永远不知道。
 *
 * 2026-07-27 从 Draw.tsx / Video.tsx 各自的一条正则下沉到这里。
 * 那两份是复制出来的、各自演化，于是**漏的是同一批**：
 * 线上未关闭的 30 条 issue 里有 10 条是它们误报的 ——
 *   · 「用户额度不足, 剩余额度: -0.99」×6  ← 正则里只有「余额不足」，差一个字
 *   · 「这个 Key 无效、已过期，或还没有在服务器建档」  ← 只匹配了英文 invalid token
 *   · 「system cpu overloaded」            ← 服务端过载，客户端没 bug
 *   · 「curl: (6) getaddrinfo」            ← 客户机 DNS/断网
 *
 * 加规则时的分寸：**宁可漏判成 bug，也别过度宽泛**。
 * 吞掉一条真崩溃的代价，远大于多收一条噪声 issue。
 */

/** 判定结果。`hint` 是给用户看的人话；它同时是 i18n 的 key（「中文即 key」）。 */
export type ErrorVerdict = {
  /** true = 用户/环境问题，直接给人话、**不上报** bug 仓库 */
  actionable: boolean;
  /** actionable 时显示这句；空串表示直接显示原文即可（原文本身已经是中文人话） */
  hint: string;
};

/** 每条规则 = [匹配, 给用户看的人话]。人话为空串 = 原文已经够清楚，直接显示。 */
const RULES: Array<[RegExp, string]> = [
  // ── 余额 / 额度 ────────────────────────────────────────────
  // 「余额不足」和「用户额度不足」是服务端两种不同措辞，都要认。
  [/余额不足|额度不足|剩余额度|quota is not enough|insufficient|out of credit/i,
    "额度不够了，去「虾盘云 · 充值」充一点就能继续。"],
  // 账户被冻结 / 额度耗尽但 Key 本身有效（pc-*** 类：/v1/models 认证通过但
  // 模型列表全空、chat_count=0）—— 跟「Key 无效」区分开，别让客户去换 Key。
  [/frozen|freeze|account suspended|quota exhausted|no available model|无可用模型|模型列表为空|chat_count\s*[:=]\s*0/i,
    "账户额度已耗尽或已被冻结（Key 本身有效）。去「虾盘云 · 充值」页充值即可恢复。"],

  // ── Key / 鉴权 ────────────────────────────────────────────
  [/invalid token|Key 无效|已过期|没有在服务器建档|Key 暂时不可用|unauthorized|\b401\b/i,
    "这个 Key 用不了（无效或还没建档）。去「虾盘云 · 充值」页确认一下 Key。"],

  // ── 服务端过载 / 上游故障 ──────────────────────────────────
  // 不是客户端 bug，服务端自己有监控；上报到客户端 bug 仓库只会淹掉真问题。
  [/cpu overloaded|overloaded|too many requests|rate limit|\b(429|502|503|504)\b|service unavailable|bad gateway/i,
    "服务器这会儿忙不过来，等一两分钟再试。不是你电脑的问题。"],

  // ── 网络（客户机侧）────────────────────────────────────────
  // curl 6=DNS 解析不了，7=连不上，35=TLS 握手被断（国内 SNI 阻断常见），28=超时。
  [/getaddrinfo|could not resolve|无法解析|curl.*\(\s*(6|7|35)\s*\)|curl 退出码 (6|7|35)|connection refused|network is unreachable/i,
    "网络连不上服务器。检查一下网络，或换个网络（有的公司网/校园网会挡）。"],

  // ── 超时 ──────────────────────────────────────────────────
  [/超时|timeout|timed out|curl.*\(\s*28\s*\)|curl 退出码 28/i,
    "这次等太久超时了。图越复杂越慢，重试一次通常就好。"],

  // ── 内容安全审核 ──────────────────────────────────────────
  [/安全系统拒绝|safety system|content filter|content_policy|policy violation/i, ""],

  // ── 入参不合法（用户选的尺寸/模型不被支持）────────────────
  [/尺寸格式不对|尺寸不支持|must be one of|invalid size|unsupported model/i, ""],
  [/参考图格式|参考图.*色彩模式|invalid file or mode for image|invalid image format/i, ""],
];

/**
 * 分类一条报错。
 *
 * @param msg 后端抛上来的原始错误串（通常是 `String(e)`）
 */
export function classifyError(msg: string): ErrorVerdict {
  for (const [re, hint] of RULES) {
    if (re.test(msg)) return { actionable: true, hint };
  }
  return { actionable: false, hint: "" };
}
