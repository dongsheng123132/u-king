/**
 * 终端里的文件路径 = 可点的东西。
 *
 * 客户原话：「终端里的文件，无法点击预览，右侧选择打开方式，连复制都没」。
 * AI 干完活最后一句往往就是「已生成 D:\xx\报告.docx」—— 那行字在终端里是**死的**：
 * 想看得自己去资源管理器翻，想复制路径得先跟 ConPTY 的折行搏斗。
 *
 * 这里只做两件事，都不碰 PTY 数据流：
 *  1. 给 xterm 注册一个 link provider，把每一行里**像文件路径**的片段变成可点的链接；
 *  2. 记住鼠标最后悬停的那个链接，让右键菜单知道「你点的是哪个文件」。
 *
 * 判定故意保守 —— 宁可漏，不可把满屏普通英文单词都画上下划线：
 *  - 绝对路径（`D:\a\b.txt`、`/c/Users/...`、`/usr/local/bin`）一律算；
 *  - 带分隔符的相对路径（`src/main.rs`、`.\dist\app.exe`）算；
 *  - 光秃秃一个词，只有带**已知扩展名**（`报告.docx`）才算。
 * 存不存在不在这儿判：终端里出现的路径可能还没建出来，点了再去问后端，比事先猜稳。
 */

/** 只认这些扩展名的「裸文件名」（不带目录）。列表宁短勿长 —— 每多一个就多一片误画下划线。 */
const BARE_NAME_EXTS =
  "txt|md|json|jsonl|yaml|yml|toml|ini|csv|tsv|log|html|htm|css|js|mjs|cjs|ts|tsx|jsx|rs|py|go|java|c|h|cpp|sh|bat|ps1|pdf|docx|doc|xlsx|xls|pptx|ppt|png|jpg|jpeg|gif|webp|bmp|svg|mp4|mov|webm|mp3|wav|zip|7z|rar|tar|gz|exe|dll|dxf|psd";

/**
 * 全角标点：中文 CLI 输出里它们是**句子的**，不可能是路径的一部分。
 *
 * 🔴 不排掉会连累到「必须带扩展名」那道闸：`已生成 D:\工作\报告.docx，已上传 …` 里，
 * 贪婪的 SEG 会一路吃到 `报告.docx，已上传`，结尾不是扩展名 → 整条被丢掉 →
 * 再由「裸文件名」那条捡漏，捡出来的是**截断的 `告.docx`**（`\w` 不含中文，前缀类把「报」吃了）。
 * 客户看到的就是「下划线画在半个文件名上，点了打不开」。
 * 半角标点交给 TRAIL 收尾即可 —— 它们在路径里偶尔合法，全角的不会。
 */
const CJK_PUNCT = "，。；：！？、（）【】《》「」『』〈〉“”‘’…";

/** 路径里允许出现的字符：中英文、数字、常见符号。**不含空格** —— 带空格的路径靠引号那条路。 */
const SEG = `[^\\s"'<>|:*?${CJK_PUNCT}]+`;

/** 网址里允许出现的字符。**不含**空格、引号、尖括号、反引号、竖线、反斜杠。 */
const URL_BODY = "[^\\s\"'<>`|\\\\]+";

/**
 * 🔴 **这里一个 lookbehind 都不许有。**
 *
 * `(?<!…)` 在老 WebKit（macOS < 13.3 的 WKWebView，即 Safari < 16.4）上是**语法错误**， webkit-compat-ok
 * 当场抛 `SyntaxError: Invalid regular expression: invalid group specifier name`。
 * 而这几条正则是**模块顶层求值**的、这个模块又被打进主入口 chunk ——
 * 于是 Mac 用户看到的不是「路径点不动」，是**整个 U-King 白屏起不来**（0.9.99 / 0.9.100 中招）。
 *
 * 跑道照不到：`check-term-file-links.mjs` 两层跑的是 Node 22 + Playwright Chromium，
 * 两个引擎都支持 lookbehind，所以它一路全绿。守这条的是 `check-webkit-compat.mjs`（静态禁用）。
 *
 * 替代写法：**把「前面那个字符」吃进来当第 1 组**，路径永远是第 2 组，
 * 起始下标 = `m.index + m[1].length`。没有前缀约束的那两条用空组 `()` 占位，保持形状统一。
 */
const PATTERNS: { re: RegExp; kind: "url" | "path" }[] = [
  // ⓪ 🔴 **网址必须排在最前面**，而且必须存在。
  //
  // 不做这条会同时错两次（客户 2026-08-16 的截图，两种错法都在同一屏上）：
  //   a) `U="https://2origin-site.pages.dev"` 被下面②认成 **`s://2origin-site.pages.dev`**
  //      —— `https` 的**最后一个字母 `s` 当了盘符**（`[A-Za-z]:[\/]`），于是走文件预览，
  //      报「系统找不到指定的路径 (os error 3)」。网址长得像盘符路径，这不是巧合，是
  //      `scheme:` 和 `X:` 本来就同形；
  //   b) `https://a.b/docs/` / `http://localhost:1430/` 这类**结尾是 `/` 或没扩展名**的，
  //      过不了下面 HAS_EXT 那道闸，于是**一条都认不出来**，点也点不着。
  //
  // 认出来只是一半：网址不能交给「预览文件」那条路（见 registerFileLinks 的 kind 分流）。
  { re: new RegExp(`(^|[^A-Za-z0-9_-])((?:https?|file)://${URL_BODY})`, "gi"), kind: "url" },
  // ① 引号包起来的（带空格的路径唯一能被可靠切出来的形式）；前缀就是那个左引号
  { re: new RegExp(`(")([A-Za-z]:[\\\\/][^"]{1,300}|/[^"]{1,300})"`, "g"), kind: "path" },
  // ② Windows 绝对路径：D:\x\y 或 D:/x/y
  //    前面不许贴着字母/数字/下划线 —— 否则 `https:` 里的 `s:` 就成了盘符（见 ⓪ 的 a）。
  //    ⓪ 已经把 http(s)/file 的网址先占走了，这条是**第二道**：换个 scheme（`ftp://`、
  //    `ws://`）我们不认，但也绝不会再把它错当成 S 盘。
  { re: new RegExp(`(^|[^A-Za-z0-9_])([A-Za-z]:[\\\\/]${SEG})`, "g"), kind: "path" },
  // ③ Unix / Git Bash 绝对路径：/c/Users/... 、/usr/local/bin/x（前面不许贴着字母或点）
  { re: new RegExp(`(^|[^\\w.])(/(?:${SEG})(?:/${SEG})*)`, "g"), kind: "path" },
  // ④ 带分隔符的相对路径：src/main.rs 、.\dist\app.exe 、..\out\a.txt
  { re: new RegExp(`()((?:\\.{1,2}[\\\\/])?(?:${SEG})(?:[\\\\/]${SEG})+)`, "g"), kind: "path" },
  // ⑤ 裸文件名 + 已知扩展名：报告.docx
  { re: new RegExp(`(^|[^\\w\\\\/.])([^\\s"'<>|:*?\\\\/]+\\.(?:${BARE_NAME_EXTS})\\b)`, "gi"), kind: "path" },
];

/**
 * 🔴 **必须带扩展名才算数**（最后一段得有 `.xxx`）。
 *
 * 不是洁癖，是被跑道当场逼出来的：不加这条，PowerShell 提示符 `PS C:\Users\<user>>` 里的
 * 那截目录**每一行都会被画成链接** —— 客户看到的是满屏下划线，比不做还糟。
 * 代价是**目录点不了**（`D:\工作` 这种不给链接）。认了这个代价：客户抱怨的是「文件」打不开，
 * 目录那条路文件面板里有；而误报是每一行都在犯。
 */
const HAS_EXT = /[^\\/]\.[A-Za-z0-9]{1,8}$/;

/**
 * 「长得像本机路径，但一定不是」的两种。认了 = 用户点开必然 404，跟网址被当文件读同一类错。
 * 都只做**减法**（只会少画链接，不会多画），所以误伤上限很低。
 */
function looksNotLocal(line: string, start: number, text: string): boolean {
  // ① git diff 的合成前缀：`diff --git a/src/x.ts b/src/x.ts` —— `a/` `b/` 是 git 编的，
  //    磁盘上没有。AI CLI 天天刷 diff，这条是**出现频率最高**的一个误报。
  if (/^diff --git /.test(line) && /^[ab][\\/]/.test(text)) return true;
  // ② 远端地址的路径部分：`git@github.com:owner/repo.git`、`scp x user@host:/var/log/a.log`
  //    —— 冒号左边是 user@host，右边那截在**别人机器**上。
  if (/[\w.-]+@[\w.-]+:$/.test(line.slice(0, start))) return true;
  return false;
}

/** 结尾这些标点是句子的，不是路径的（`见 D:\a\b.txt。` / `(src/main.rs)`）。 */
const TRAIL = /[.,;:!?)\]}>，。；：！？）】、'"]+$/;
/** 开头这些同理。箭头/项目符号是**排版**，不是路径的一部分 —— AI 爱写
 *  `格子那件 →demo\SUBMIT-…\`，不削掉的话链接会从「→」开始，点了自然打不开。 */
const LEAD = /^[([{<【（'"`→←⇒⟹▶▸•·※]+/;

/** `kind` 决定**点了之后走哪条路**：`path` 去预览文件，`url` 交给浏览器。分错了就是 os error 3。 */
export type LineHit = {
  text: string;
  start: number;
  end: number;
  kind: "url" | "path";
  /** 明写着是目录（结尾带分隔符）。点它该**打开文件夹**，不是塞进文件预览。 */
  isDir?: boolean;
};

/**
 * 从一行终端文本里找出所有像路径的片段。
 * @returns start/end 是 0 基、左闭右开的字符下标（xterm 的 range 是 1 基，调用方 +1）。
 */
export function findPathsInLine(line: string): LineHit[] {
  const hits: LineHit[] = [];
  const taken: boolean[] = new Array(line.length).fill(false);

  for (const { re, kind } of PATTERNS) {
    re.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = re.exec(line)) !== null) {
      // 每条模式的形状都是「(前缀组)(路径组)」—— 前缀可能是空串，路径永远是第 2 组
      const raw = m[2];
      let start = m.index + m[1].length;
      let text = raw;

      const lead = LEAD.exec(text);
      if (lead) {
        start += lead[0].length;
        text = text.slice(lead[0].length);
      }
      const trail = TRAIL.exec(text);
      if (trail) text = text.slice(0, text.length - trail[0].length);
      if (text.length < 3) continue;
      // 「必须带扩展名」是给**文件路径**定的（防提示符满屏下划线）。网址没有这个概念：
      // `http://localhost:1430/`、`https://a.b/docs/` 一个扩展名都没有，照样是好网址。
      // 「必须带扩展名」是给**文件**定的（防提示符满屏下划线）。但**以分隔符结尾**的那种
      // （`demo\SUBMIT-格子-20260816\`、`cd /usr/local/`）是明写着的目录，放它过 ——
      // 客户原话：「比如打开对应的文件夹」。这条放宽很精确：PowerShell 提示符
      // `PS C:\Users\<user>>` 结尾是 `>`（还会被 TRAIL 削掉），够不着这条。
      const looksDir = /[\\/]$/.test(text);
      if (kind === "path" && !looksDir && !HAS_EXT.test(text)) continue;
      if (kind === "path" && looksNotLocal(line, start, text)) continue;

      const end = start + text.length;
      // 前面的模式优先（绝对路径 > 相对 > 裸文件名），重叠的直接丢
      let overlap = false;
      for (let i = start; i < end; i++) if (taken[i]) { overlap = true; break; }
      if (overlap) continue;
      for (let i = start; i < end; i++) taken[i] = true;
      hits.push({ text, start, end, kind, isDir: kind === "path" && looksDir });
    }
  }
  return hits.sort((a, b) => a.start - b.start);
}

/**
 * 一个字符在终端里占几格。
 *
 * 🔴 **别拿字符下标当列号。** 终端是按格排字的：中日韩全角字符一个占两格。
 * 「已生成 报告.docx」里 `报告.docx` 的字符下标是 4，列号却是 8 —— 差 4 格。
 * 拿下标当列号，下划线会画在路径左边，越靠后的字符越点不着（跑道实测到的就是这个）。
 */
function charCells(ch: string): number {
  const c = ch.codePointAt(0) ?? 0;
  // 常见全角区间（CJK、假名、谚文、全角标点/字母）—— 不引 wcwidth 依赖，够用即可
  return (c >= 0x1100 && c <= 0x115f) ||
    (c >= 0x2e80 && c <= 0xa4cf) ||
    (c >= 0xac00 && c <= 0xd7a3) ||
    (c >= 0xf900 && c <= 0xfaff) ||
    (c >= 0xfe30 && c <= 0xfe6f) ||
    (c >= 0xff00 && c <= 0xff60) ||
    (c >= 0xffe0 && c <= 0xffe6)
    ? 2
    : 1;
}

/** 一段文本占几格（字符下标 → 列号的换算靠它）。 */
export function displayWidth(s: string): number {
  let n = 0;
  for (const ch of s) n += charCells(ch);
  return n;
}

/**
 * 从一行里挖出「可能是某个目录」的绝对路径。给**裸文件名**找家用的。
 *
 * 🔴 为什么需要它：AI 的输出常常是「先报目录、再报文件名」，两件事**在不同的行**上 ——
 * ```
 * D:\工作项目\...\GOAI初赛-第3次提交-20260816\
 * 里面已经打包上传的 zip: xxx-v3-20260816.zip（357 KB，28 个文件）
 * ```
 * 那个 zip 是**裸文件名**，我们只会把它拼到「终端当前目录」上 —— 而它根本不在那儿。
 * 结果右键菜单六项全废：预览 404、默认程序打开报错、资源管理器定位不到，
 * 而**「复制路径」还会不声不响地复制一条错路径**（客户 2026-08-16 实锤）。
 *
 * 判定：结尾带分隔符的当目录本身；否则取它的上一级（`Wrote D:\a\b\x.txt` → `D:\a\b`）。
 * 只挖**绝对**路径 —— 相对目录当线索太容易猜歪，宁可少猜。
 */
export function dirHintsFromLine(line: string): string[] {
  const re = new RegExp(`(^|[^A-Za-z0-9_])([A-Za-z]:[\\\\/]${SEG}|/(?:${SEG})(?:/${SEG})*)`, "g");
  const hints: string[] = [];
  let m: RegExpExecArray | null;
  while ((m = re.exec(line)) !== null) {
    let p = m[2];
    const trail = TRAIL.exec(p);
    if (trail) p = p.slice(0, p.length - trail[0].length);
    if (p.length < 4) continue;
    const dir = /[\\/]$/.test(p) ? p.replace(/[\\/]+$/, "") : p.replace(/[\\/][^\\/]*$/, "");
    if (dir.length >= 3 && !hints.includes(dir)) hints.push(dir);
  }
  return hints;
}

/**
 * 一个链接文本可能对应的绝对路径，**按可信度排序**：先按终端当前目录算，
 * 再拿上文的目录线索猜。调用方逐个问后端「在不在」，取第一个存在的。
 *
 * 只有**裸文件名**（不含分隔符）才用线索猜 —— 文本自己带了路径就照它说的算，别自作聪明。
 */
export function candidatePaths(
  text: string,
  cwd: string | undefined,
  dirHints: string[],
  /** 这个链接**前面**那截原文（同一行）。用来救「文件名里有空格」—— 见下。 */
  linePrefix = "",
): string[] {
  const out: string[] = [];
  const push = (p: string) => {
    if (p && !out.includes(p)) out.push(p);
  };
  // 🔴 文件名里有空格时，正则只能切到最后一段：`- AI4R_OPEN 格子.zip` → `格子.zip`。
  // 空格在终端里是天然的分隔符，光看文本**没法判断**它是名字的一部分还是两个词 ——
  // 所以不猜，把两种都列成候选，让**磁盘**裁决（调用方逐个问 produced_file_info）。
  // 只往左吃**一个** ASCII 词（字母数字 _ - .）：中文叙述（`已生成 报告.docx`）不会被吃进来。
  const names: string[] = [text];
  const m = /([A-Za-z0-9_.-]+) $/.exec(linePrefix);
  if (m && !/[\\/]/.test(text)) names.push(`${m[1]} ${text}`);

  for (const n of names) {
    push(resolvePath(n, cwd));
    if (!/[\\/]/.test(n)) {
      for (const d of dirHints) push(d + (d.includes("\\") ? "\\" : "/") + n);
    }
  }
  return out;
}

/** 相对路径拼到 cwd 上；绝对路径原样返回。**不碰文件系统**，纯字符串。 */
export function resolvePath(raw: string, cwd?: string): string {
  const isAbs = /^([A-Za-z]:[\\/]|\\\\|\/)/.test(raw);
  if (isAbs || !cwd) return raw;
  const sep = cwd.includes("\\") ? "\\" : "/";
  return cwd.replace(/[\\/]$/, "") + sep + raw.replace(/^\.[\\/]/, "");
}

/** xterm 的最小面（只用到我们真的要的那几个），避免把整个 XTerm 类型拖进纯逻辑模块。 */
type LinkableTerm = {
  registerLinkProvider: (p: {
    provideLinks: (
      row: number,
      cb: (
        links:
          | undefined
          | {
              range: { start: { x: number; y: number }; end: { x: number; y: number } };
              text: string;
              activate: (e: MouseEvent, text: string) => void;
              hover?: (e: MouseEvent, text: string) => void;
              leave?: () => void;
            }[],
      ) => void,
    ) => void;
  }) => { dispose: () => void };
  buffer: {
    active: {
      getLine: (row: number) => { translateToString: (trim?: boolean) => string; isWrapped: boolean } | undefined;
    };
  };
};

export type FileLinkHooks = {
  /** 终端的工作目录，用于把相对路径拼成绝对路径。 */
  cwd?: () => string | undefined;
  /** 左键点了一个**文件路径**。`candidates` 按可信度排序，第一项就是 `absPath`；
   *  调用方应逐个问后端「在不在」，取第一个存在的（见 `candidatePaths`）。 */
  onOpen: (absPath: string, candidates: string[]) => void;
  /** 左键点了一个**网址**。没给就静默不响应 —— 但绝不会掉进 onOpen 去当文件读。 */
  onOpenUrl?: (url: string) => void;
  /** 在**文件路径**上按了右键（坐标给菜单定位）。网址不出这个菜单（「打开方式」对网址没意义）。 */
  onMenu: (info: { x: number; y: number; path: string; candidates: string[] }) => void;
};

/**
 * 把路径链接挂到一个 xterm 上。返回反注册函数。
 *
 * `el` 是该终端的 DOM 容器 —— 右键菜单靠它上面的 contextmenu 事件触发；
 * 「右键的是哪个路径」用**最后悬停的链接**判定（右键前必然先有 mousemove，xterm 会调 hover）。
 */
export function registerFileLinks(term: LinkableTerm, el: HTMLElement, hooks: FileLinkHooks): () => void {
  let hovered: { path: string; candidates: string[] } | null = null;

  /** 往上翻找目录线索。**只在这一行真有裸文件名时才翻** —— provideLinks 每次渲染都会被调，
   *  无条件扫 200 行正则会把滚动卡住。最多取 5 个，近的排前面。 */
  const dirHintsAbove = (row: number): string[] => {
    const hints: string[] = [];
    for (let r = row - 2; r >= 0 && row - r <= 200 && hints.length < 5; r--) {
      const l = term.buffer.active.getLine(r)?.translateToString(true) ?? "";
      if (!l) continue;
      for (const d of dirHintsFromLine(l)) if (!hints.includes(d)) hints.push(d);
    }
    return hints;
  };

  const provider = term.registerLinkProvider({
    provideLinks(row, cb) {
      // 🔴 **按「逻辑行」认，不是按屏幕行。** 终端一行满了会折到下一行（xterm 标记 isWrapped），
      // 而长中文路径在窄面板里必折。只看单行的后果实测过（客户 2026-08-16 那条 40MB 的 zip）：
      //   row0 "D:\项目资料\图书排版-…\报价-零"  → 认出 []      ← 真正的开头，没链接
      //   row1 "械零部件测绘及成图技术\发给客户-…"  → 认出 []
      //   row2 "-taskpack.zip（40 MB）"           → 认出 ["-taskpack.zip"]  ← **碎片假链接**
      // 头上没得点，尾巴上却挂了个点了必错的。所以先把折行拼回去再认。
      const buf = term.buffer.active;
      let first = row - 1; // 0 基
      while (first > 0 && buf.getLine(first)?.isWrapped) first--;
      const parts: string[] = [];
      for (let r = first; r - first < 32; r++) {
        const l = buf.getLine(r);
        if (!l) break;
        if (r > first && !l.isWrapped) break;
        // 不 trim：折行的每一段都是满宽的，中间不能被裁掉，否则下标对不回去
        parts.push(l.translateToString(false));
      }
      if (!parts.length) return cb(undefined);
      parts[parts.length - 1] = parts[parts.length - 1].replace(/\s+$/, ""); // 只裁最后一段的补白
      const line = parts.join("");
      if (!line) return cb(undefined);
      const hits = findPathsInLine(line);
      if (!hits.length) return cb(undefined);
      const needHints = hits.some((h) => h.kind === "path" && !/[\\/]/.test(h.text));
      const hints = needHints ? dirHintsAbove(first + 1) : [];
      const candsOf = (text: string, at: number) =>
        candidatePaths(text, hooks.cwd?.(), hints, line.slice(0, at));

      /** 逻辑行里的字符下标 → xterm 的 (列, 行)。列是**格**不是字符（全角占两格）。 */
      const posOf = (idx: number, inclusive: boolean) => {
        let acc = 0;
        for (let i = 0; i < parts.length; i++) {
          if (idx < acc + parts[i].length) {
            const w = idx - acc;
            const cells = displayWidth(parts[i].slice(0, inclusive ? w + 1 : w));
            return { x: inclusive ? cells : cells + 1, y: first + i + 1 };
          }
          acc += parts[i].length;
        }
        const last = parts.length - 1;
        return { x: displayWidth(parts[last]), y: first + last + 1 };
      };

      cb(
        hits
          .map((h) => ({ h, start: posOf(h.start, false), end: posOf(h.end - 1, true) }))
          // xterm 是**按行**来问的：只把盖住当前这一行的链接给它
          .filter(({ start, end }) => start.y <= row && row <= end.y)
          .map(({ h, start, end }) => ({
          // xterm 的 range 是**列号**（1 基、两端闭），不是字符下标 —— 全角字符占两格，
          // 差别在中文路径上是实打实的好几格（见 charCells 的注释）。
          range: { start, end },
          text: h.text,
          // 🔴 网址不许走 onOpen。以前没有这个分叉，`https://…` 被当绝对路径丢进文件预览，
          // 客户看到的是「读取失败：系统找不到指定的路径 (os error 3)」。
          activate: (_e, text) => {
            if (h.kind === "url") return hooks.onOpenUrl?.(text);
            const cands = candsOf(text, h.start);
            hooks.onOpen(cands[0], cands);
          },
          hover: (_e, text) => {
            // 只有文件路径进右键菜单；悬到网址上要把它清掉，否则右键会拿到**上一个**文件的路径
            if (h.kind === "url") {
              hovered = null;
              return;
            }
            const cands = candsOf(text, h.start);
            hovered = { path: cands[0], candidates: cands };
          },
          leave: () => {
            hovered = null;
          },
        })),
      );
    },
  });

  const onContext = (e: MouseEvent) => {
    if (!hovered) return; // 不在路径上：让已有的「有选区就复制」那条逻辑继续管
    e.preventDefault();
    e.stopPropagation();
    hooks.onMenu({ x: e.clientX, y: e.clientY, path: hovered.path, candidates: hovered.candidates });
  };
  // capture：要抢在 attachClipboard 那个 contextmenu 之前，否则「选中态 + 右键路径」会被它吃掉
  el.addEventListener("contextmenu", onContext, true);

  return () => {
    provider.dispose();
    el.removeEventListener("contextmenu", onContext, true);
  };
}
