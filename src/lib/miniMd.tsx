/**
 * 极简 markdown 渲染 —— 只认大模型最常吐的那几种：`## 标题` / `**粗体**` / `- 列表` /
 * `1. 有序列表` / `> 引用` / `---` 分隔线 / ``` 代码块 / **GFM 表格**。
 *
 * ## 为什么自己写几十行，不引 react-markdown
 * 体积红线（CLAUDE.md「设计取舍」：exe 要小，前端也一样别乱加依赖）。这里要的只是「让人能读」。
 *
 * ## 为什么必须渲染
 * 无人值守跑出来的结果是给**人**看的。一屏 `**` 和 `##` 在演示里就等于「这功能没做完」。
 *
 * ## 表格（2026-08-11 加，Issue #379 / #380）
 * 原来「刻意不做」，理由是这个渲染器只服务无人值守的结果页。但 U-Workspace 成了主工作台之后，
 * 模型张口就是表格，而不支持的后果不是「朴素」，是**满屏裸竖线** —— 客户原话「排版很糟糕」。
 *
 * 🔴 那两条 issue 里的诊断是错的（说「渲染器生成了 `<table>` 但没样式」，实际是压根不生成；
 * 又说「标题不渲染」，标题一直是渲染的）。**症状真，归因假** —— 照着归因去加 CSS 会一无所获。
 *
 * 判据严格：**必须有分隔行**（`|---|---|`）才当表格。否则一句「A | B 二选一」会被吃成表格，
 * 那是拿正常句子换表格，得不偿失。
 *
 * ## 代码块的操作条（2026-08-17 加）
 * 传了 `onRunInTerminal` 就给代码块配「复制 / 贴到终端」。见 `CodeBlock` 的注释。
 *
 * ## 刻意不做（仍然不做）
 * 图片、链接跳转、**HTML 透传**。尤其不透传 HTML —— 那等于把模型输出当代码执行。
 * 全程只产出 React 元素，不碰 `dangerouslySetInnerHTML`。表格也一样：单元格走 `inline()`，
 * React 自己转义，不存在「模型吐一段 `<script>` 就被执行」这条路。
 */
import { useState, type ReactNode } from "react";
import { Copy, Check, Terminal as TermIcon } from "lucide-react";
import { copyToClipboard } from "./clipboard";
import { useI18n } from "../i18n";

type Align = "left" | "center" | "right";

/**
 * GFM 表格的分隔行：`|---|:--:|---:|`。首尾竖线可省。
 *
 * 要求每一格都是纯 `-`（两端可带 `:`）。松一点就会把 `|--- 备注 ---|` 这种误判成分隔行，
 * 于是它上面那句正常的话被当成表头吃掉。
 */
function isTableSep(line: string): boolean {
  const body = line.trim().replace(/^\|/, "").replace(/\|$/, "");
  if (!body.includes("-")) return false;
  return body.split("|").every((c) => /^\s*:?-+:?\s*$/.test(c));
}

/** 切一行的单元格。`\|` 是转义竖线，不当分隔符（表格里写 `a \| b` 是合法的）。 */
function cells(line: string): string[] {
  const t = line.trim().replace(/^\|/, "").replace(/\|$/, "");
  const out: string[] = [];
  let cur = "";
  for (let i = 0; i < t.length; i++) {
    if (t[i] === "\\" && t[i + 1] === "|") {
      cur += "|";
      i++;
    } else if (t[i] === "|") {
      out.push(cur.trim());
      cur = "";
    } else {
      cur += t[i];
    }
  }
  out.push(cur.trim());
  return out;
}

/** 从分隔行读每列对齐。`:--`=左 `:-:`=中 `--:`=右，都没有就左。 */
function alignsOf(sep: string): Align[] {
  return cells(sep).map((c) => {
    const l = c.startsWith(":");
    const r = c.endsWith(":");
    return l && r ? "center" : r ? "right" : "left";
  });
}

/** 行内：只处理 `**粗体**` 和 `` `代码` ``，其余原样。奇数个标记时把剩下的当普通文本，不吞字。 */
function inline(text: string, key: string): ReactNode[] {
  const out: ReactNode[] = [];
  // 先按 `代码` 切，再在非代码段里处理 **粗体** —— 反过来会把代码里的星号也加粗
  text.split(/(`[^`]+`)/g).forEach((seg, i) => {
    if (seg.startsWith("`") && seg.endsWith("`") && seg.length > 2) {
      out.push(
        <code key={`${key}-c${i}`} className="px-1 py-px rounded bg-white/[0.08] font-mono text-[0.92em]">
          {seg.slice(1, -1)}
        </code>,
      );
      return;
    }
    // `**粗体**` 必须先切，否则 `*斜体*` 的规则会把它拆成两半，屏幕上留下半截星号
    seg.split(/(\*\*[^*]+\*\*|\*[^*\n]+\*)/g).forEach((part, j) => {
      if (!part) return;
      const kk = `${key}-b${i}-${j}`;
      if (part.startsWith("**") && part.endsWith("**") && part.length > 4) {
        out.push(
          <strong key={kk} className="font-semibold text-ink-0">
            {part.slice(2, -2)}
          </strong>,
        );
      } else if (part.startsWith("*") && part.endsWith("*") && part.length > 2) {
        out.push(
          <em key={kk} className="italic">
            {part.slice(1, -1)}
          </em>,
        );
      } else {
        out.push(<span key={kk}>{part}</span>);
      }
    });
  });
  return out;
}

/** 围栏语言标签里，算「终端里能敲的东西」的那些。 */
const SHELL_LANGS = new Set([
  "bash", "sh", "shell", "zsh", "fish", "console", "terminal",
  "powershell", "pwsh", "ps", "ps1", "cmd", "bat", "batch", "dos",
]);

/**
 * 这块代码「贴进终端」有没有意义。
 *
 * 🔴 **不是每个代码块都该出这个键。** 把 20 行 Python 贴进 PowerShell 不叫「运行」，
 * 叫把终端搞乱；而用户是**看见按钮才以为这事能做**的 —— 出一个按了会坏事的按钮，
 * 比不出按钮更坏。所以只认两种：标了 shell 系语言的，和没标语言的单行命令。
 * 标了 `python` / `json` / `ts` 的一律不出（那是代码，不是命令）。
 */
function runnableInTerm(lang: string, code: string): boolean {
  if (SHELL_LANGS.has(lang.toLowerCase())) return true;
  if (lang) return false;
  return code.trim().split(/\n/).filter((l) => l.trim()).length === 1;
}

/**
 * 代码块 + 悬停操作条（贴到终端 / 复制）。
 *
 * ## 为什么值得单独做一个
 * 模型给出一条命令，用户下一步 100% 是「拿去跑」。原来这里是个光秃秃的 `<pre>`，
 * 那条路是：手动划选 → Ctrl+C → 切到终端 → 粘贴。四步里三步能出错，
 * 而**划漏一个字符的命令报起错来跟环境坏了一模一样** —— 用户会去查错的地方。
 *
 * ## 🔴 只贴，不回车
 * 跟 `termInbox.ts` / `ChatPanel::CommandStrip` 同一条纪律：跑命令是**写**操作，
 * 最后那一下必须由人按。换个入口不等于换掉这条规矩（同影核「写动作必须 `--yes`」：
 * 确认权在核心、不在某个界面的礼貌）。所以这里没有「▷ 直接执行」。
 *
 * ## 为什么是命名 group
 * 外层消息自己是 `group`（`MsgActions` 的悬停复制靠它）。这里若也叫 `group`，
 * CSS 的 `.group:hover .group-hover\:x` 会由**任意**祖先 group 触发 ——
 * 鼠标停在消息任何地方，所有代码块的按钮会一起亮。
 */
function CodeBlock({
  code,
  lang,
  onRunInTerminal,
}: {
  code: string;
  lang: string;
  onRunInTerminal?: (cmd: string) => void;
}) {
  const { t } = useI18n();
  const [done, setDone] = useState(false);
  const canRun = !!onRunInTerminal && runnableInTerm(lang, code);
  return (
    <div className="group/code relative my-2">
      <pre className="px-3 py-2 rounded-lg bg-bg-0/70 border border-white/[0.06] overflow-x-auto text-[12px] font-mono text-ink-2 whitespace-pre">
        {code}
      </pre>
      {/* 悬停才出：常驻的话，一段带三个代码块的回答会被按钮糊满 */}
      <div className="absolute top-1 right-1 flex items-center gap-1 opacity-0 group-hover/code:opacity-100 transition-opacity">
        {canRun && (
          <button
            onClick={() => onRunInTerminal?.(code.trim())}
            title={t("贴进终端（不自动回车，你按回车才真跑）")}
            className="inline-flex items-center gap-1 h-6 px-2 rounded-md border border-accent/30 bg-accent/[0.14] text-[11px] text-accent hover:bg-accent/[0.24]"
          >
            <TermIcon size={10} /> {t("贴到终端")}
          </button>
        )}
        <button
          onClick={() =>
            void copyToClipboard(code).then((ok) => {
              if (ok) {
                setDone(true);
                window.setTimeout(() => setDone(false), 1500);
              }
            })
          }
          title={t("复制")}
          className="inline-flex items-center justify-center w-6 h-6 rounded-md bg-bg-2/85 border border-white/[0.08] text-ink-4 hover:text-ink-1"
        >
          {done ? <Check size={11} className="text-success-400" /> : <Copy size={11} />}
        </button>
      </div>
    </div>
  );
}

export function MiniMd({
  text,
  className = "",
  onRunInTerminal,
}: {
  text: string;
  className?: string;
  /** 传了才给代码块出「贴到终端」。没有终端的地方（AutomationPanel）不传 —— 按钮不该指向不存在的东西。 */
  onRunInTerminal?: (cmd: string) => void;
}) {
  const lines = (text ?? "").split(/\r?\n/);
  const blocks: ReactNode[] = [];
  // 用持有对象而不是裸 let：TS 不跟踪闭包里的赋值，裸 let 会在 forEach 之后被收窄成 never
  const buf: { code: string[] | null; lang: string } = { code: null, lang: "" };
  let list: { ordered: boolean; items: string[] } | null = null;

  const flushList = (k: string) => {
    if (!list) return;
    const Tag = list.ordered ? "ol" : "ul";
    blocks.push(
      <Tag
        key={`l${k}`}
        className={
          "my-1.5 pl-5 space-y-1 " + (list.ordered ? "list-decimal" : "list-disc") + " marker:text-ink-5"
        }
      >
        {list.items.map((it, i) => (
          <li key={i}>{inline(it, `${k}-${i}`)}</li>
        ))}
      </Tag>,
    );
    list = null;
  };

  // 表格要往后多吃几行，而 forEach 没法跳步 —— 用一个「吃到第几行」的游标，之后的行直接跳过。
  const eaten = { until: -1 };

  lines.forEach((raw, i) => {
    if (i <= eaten.until) return;
    const k = String(i);
    const line = raw.replace(/\s+$/, "");

    // 代码块：``` 开合之间原样保留（含缩进），别去解析里面的星号
    if (line.trim().startsWith("```")) {
      if (buf.code === null) {
        flushList(k);
        buf.code = [];
        // 围栏后面那截是语言标签（```bash / ```powershell）。原来直接丢掉，
        // 现在拿它判断「贴到终端」该不该出（见 runnableInTerm）。
        buf.lang = line.trim().slice(3).trim().split(/\s+/)[0] ?? "";
      } else {
        blocks.push(
          <CodeBlock key={`p${k}`} code={buf.code.join("\n")} lang={buf.lang} onRunInTerminal={onRunInTerminal} />,
        );
        buf.code = null;
        buf.lang = "";
      }
      return;
    }
    if (buf.code !== null) {
      buf.code.push(raw);
      return;
    }

    if (!line.trim()) {
      flushList(k);
      return;
    }
    if (/^\s*(---+|===+|\*\*\*+)\s*$/.test(line)) {
      flushList(k);
      blocks.push(<hr key={`h${k}`} className="my-3 border-white/[0.08]" />);
      return;
    }
    const head = /^(#{1,6})\s+(.*)$/.exec(line);
    if (head) {
      flushList(k);
      const lvl = head[1].length;
      blocks.push(
        <div
          key={`t${k}`}
          className={
            "font-semibold text-ink-0 " +
            // 用 em 而不是 px：外层字号可调（测试报告 #007），标题得跟着一起缩放，
            // 否则放大正文后标题反而比正文小，层级整个翻过来。
            (lvl <= 1 ? "text-[1.18em] mt-3 mb-1.5" : lvl === 2 ? "text-[1.1em] mt-3 mb-1" : "text-[1.02em] mt-2 mb-0.5")
          }
        >
          {inline(head[2], k)}
        </div>,
      );
      return;
    }
    const quote = /^\s*>\s?(.*)$/.exec(line);
    if (quote) {
      flushList(k);
      blocks.push(
        <div key={`q${k}`} className="my-1 pl-3 border-l-2 border-accent/40 text-ink-3">
          {inline(quote[1], k)}
        </div>,
      );
      return;
    }
    // GFM 表格。**必须下一行是分隔行**才认 —— 只看竖线的话，「A | B 二选一」这种正常句子
    // 会被吃成一张表。表头也要求 ≥2 格，单列「表格」没有意义、更可能是误判。
    const head2 = i + 1 < lines.length ? lines[i + 1] : "";
    if (line.includes("|") && isTableSep(head2) && cells(line).length >= 2) {
      const cols = cells(line);
      const aligns = alignsOf(head2);
      // GFM 要求分隔行列数和表头一致。对不上多半不是表格，别硬认。
      if (aligns.length === cols.length) {
        flushList(k);
        const body: string[][] = [];
        let j = i + 2;
        while (j < lines.length && lines[j].trim() && lines[j].includes("|")) {
          body.push(cells(lines[j]));
          j++;
        }
        eaten.until = j - 1;
        const at = (a: Align) => (a === "center" ? "text-center" : a === "right" ? "text-right" : "text-left");
        blocks.push(
          // 外层 overflow-x-auto：列多时表格自己横向滚，不把整个对话区撑破
          // （客户反馈的「排版很糟糕」有一半是宽内容顶破布局）。
          <div key={`tb${k}`} className="my-2 overflow-x-auto">
            <table className="min-w-full border-collapse text-[0.95em]">
              <thead>
                <tr className="border-b border-white/[0.14]">
                  {cols.map((c, ci) => (
                    <th
                      key={ci}
                      className={"px-2.5 py-1.5 font-semibold text-ink-0 whitespace-nowrap " + at(aligns[ci])}
                    >
                      {inline(c, `${k}-h${ci}`)}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {body.map((row, ri) => (
                  <tr key={ri} className="border-b border-white/[0.06] last:border-0">
                    {/* 按表头列数补齐/截断：模型经常少写一格，缺的画空格总比整行错位强 */}
                    {cols.map((_, ci) => (
                      <td key={ci} className={"px-2.5 py-1.5 align-top " + at(aligns[ci])}>
                        {inline(row[ci] ?? "", `${k}-r${ri}-${ci}`)}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>,
        );
        return;
      }
    }

    const ul = /^\s*[-*+]\s+(.*)$/.exec(line);
    const ol = /^\s*\d+[.)]\s+(.*)$/.exec(line);
    if (ul || ol) {
      const ordered = !!ol;
      if (!list || list.ordered !== ordered) {
        flushList(k);
        list = { ordered, items: [] };
      }
      list.items.push((ul ?? ol)![1]);
      return;
    }
    flushList(k);
    blocks.push(
      <p key={`b${k}`} className="my-1 leading-relaxed">
        {inline(line, k)}
      </p>,
    );
  });
  flushList("end");
  // 文件末尾没闭合的 ``` ：把已收的原样吐出来，别把内容吞了。
  // 🔴 这一支**不给操作条**：流式输出时围栏还没闭合，命令可能只写了半条 ——
  // 「贴到终端」贴过去半条比没有按钮更坏（用户按了回车才发现是残的）。
  if (buf.code !== null && buf.code.length) {
    blocks.push(
      <pre
        key="p-end"
        className="my-2 px-3 py-2 rounded-lg bg-bg-0/70 border border-white/[0.06] overflow-x-auto text-[12px] font-mono text-ink-2 whitespace-pre"
      >
        {buf.code.join("\n")}
      </pre>,
    );
  }

  return <div className={className}>{blocks}</div>;
}
