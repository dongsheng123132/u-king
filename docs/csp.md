# WebView 的基线 CSP（2026-11-02 起）

`src-tauri/tauri.conf.json` 的 `app.security.csp` 从 `null`（关闭）改成一条显式策略。
本文说明：**这条策略覆盖什么、为什么是这些来源、哪些地方是妥协、以后新增域名该改哪里。**

## 1. 现状与目标

改造前 `csp: null`：WebView 里跑什么、往哪发请求都没有限制。U-King 会渲染 AI 输出和用户导入的
文档（docx/xlsx/pdf/psd/zip/html 预览），一旦有注入点，关闭 CSP 等于把「能加载谁、能连谁」交给
页面自己决定。

具体到本仓，最直接的注入面是 `src/vendor/redline-core/viewers/DocxViewer.tsx`：它把 mammoth 产出的
HTML 用 `dangerouslySetInnerHTML` 直接塞进 DOM，**没有 DOMPurify 之类的前置净化**（代码注释的理由是
「mammoth 自己生成标签、不透传原始标记」）。这类「信任上游转换器」的判断一旦被某个畸形 docx 推翻，
有 CSP 和没 CSP 是两种后果：没 CSP 时注入的 `<img onerror>` 直接执行；有本策略时内联事件处理器
被 `script-src`（无 `'unsafe-inline'`）拦下。

改造后只做一件事：**给 WebView 立一条尽量紧的白名单**。不动 capabilities（`shell:allow-open`、
`core:webview:allow-create-webview-window` 是另一个 PR 的事），不动业务代码。

## 2. 这条 CSP 全文

```
default-src 'self';
script-src 'self';
style-src 'self' 'unsafe-inline';
img-src 'self' data: blob: asset: http://asset.localhost;
media-src 'self' asset: http://asset.localhost;
font-src 'self';
connect-src 'self' ipc: http://ipc.localhost asset: http://asset.localhost data: http://127.0.0.1:4318;
frame-src 'self' asset: http://asset.localhost blob: http://127.0.0.1:3080;
worker-src 'self';
object-src 'none';
base-uri 'self';
form-action 'none';
frame-ancestors 'none'
```

> 配置里是**一行**（JSON 字符串），换行只是给人看的，语义等价。

> **2026-11-03 更新**：真机实测发现白屏兜底页有两处被这条策略误伤（见 §12.1），
> 已改成 `addEventListener` + `__TAURI_INTERNALS__.invoke("report_bug", …)`。
> 结论不变 —— 而且比原来更紧：`connect-src` 不需要为兜底页放行任何 https。

## 3. 调研：WebView 到底需要哪些来源

先按「谁在发请求」分两桶，这一步决定了 CSP 能不能收紧——分错了就会写出一个看起来能用、
实际上要么把功能拦死、要么被迫开 `https:` 通配的策略。

### 3.1 桶 A：WebView 自己会连的来源（只有 4 个）

| 来源 | 用途 | 证据 |
|---|---|---|
| `ipc:` / `http://ipc.localhost` | Tauri v2 的前后端 IPC（`invoke`）。macOS/Linux 走 `ipc://`，Windows 走 `http://ipc.localhost` | 47 个前端文件 `import { invoke … } from "@tauri-apps/api/core"` |
| `asset:` / `http://asset.localhost` | 本地文件预览：`convertFileSrc()`（视频、redline 文档预览），并按需 `fetch(convertFileSrc(path))` 读字节 | `src/Video.tsx`、`src/opencodex/redline-host-tauri.tsx:21`、`src/opencodex/panels/*`；`Cargo.toml` 开 `protocol-asset` |
| `data:` | 作图功能把 data URL 再读回字节：`fetch(dataUrl)`；图片预览、二维码、作图历史都是 `data:image/png;base64,…` | `src/Draw.tsx:147`；`src-tauri/src/draw.rs::list_draw_history` 明确注释「图片读回 data URL 给前端」 |
| `http://127.0.0.1:4318` | 可选 OTLP trace 上报（默认关，`OTEL_TRACES_EXPORTER=none`；显式开启才发） | `src/lib/otel/tracer.ts:35` |

iframe 另算（`frame-src`）：`http://127.0.0.1:3080` 是 DSH 网页工作台的内嵌页
（`src/opencodex/ToolAppView.tsx:37` `DSH_WEB_URL`，`:690` 的 `<iframe src={DSH_WEB_URL}>`）。
`blob:` / `asset:` 是 HTML 预览用 `<iframe>`（`src/vendor/redline-core/viewers/HtmlViewer.tsx`，
bytes 走 blob、有 `srcUrl` 时走 asset）。

### 3.2 桶 B：源码里出现、但**不需要**写进 CSP 的域名

这是本次调研最容易被误判的地方。`grep -rhoE "https?://…" src src-tauri` 去重后有 **139 个 host**（含大量测试夹具），
但它们**没有一个是 WebView 发的**：

| 域名（举例） | 为什么不需要 CSP |
|---|---|
| `api.u-claw.org.cn`（43 处）、`api.deepseek.com`、`openrouter.ai`、`api.moonshot.cn`、`dashscope.aliyuncs.com`、`ark.cn-beijing.volces.com`、`tokenhub.tencentmaas.com`、`open.bigmodel.cn`、`api.xiaomimimo.com`、`qianfan.baidubce.com`、`api.z.ai`、`api.siliconflow.cn`、`opencode.ai` 等 | **模型 API 全部由 Rust 侧调系统 `curl`** 发出，不经过 WebView。`src-tauri/Cargo.toml` 没有 `reqwest`/`ureq`，网络代码是 `installer::curl(...)`、`agent/chat.rs::curl_command()`。前端只做展示与 `invoke` |
| `github.com`、`u-claw.org.cn`、`tu.u-claw.org.cn`、`code.claude.com`、`apps.microsoft.com`、各厂商 `platform.*` / `console.*` 取 Key 页 | 这些是 `openUrl()`（`@tauri-apps/plugin-opener`）**交给系统浏览器**打开的链接，不是 WebView 发请求 |
| `mirrors.aliyun.com`、`download.deepseek.com`、`registry.npmmirror.com`、`u-claw-updates.oss-cn-shenzhen.aliyuncs.com`、`ghproxy.net`、`modelscope.cn` | 下载/安装链路，同样在 Rust 侧 `curl` |
| `x.com`、`example.com`、`evil.com`、`*.test`、`u-claw.org.evil.com` | 测试夹具 / 钓鱼用例（校验「不要被相似域名骗过」的断言），永远不会被真正请求 |

结论：**能枚举完，所以不需要 `https:` 通配。** 这也是本 PR 与「先上 `https:` 兜底」做法的分界：
后者会把「页面能往任意 https 目标发请求」重新打开，等于把 `connect-src` 的收益让掉大半。

### 3.3 其它调研项

- `withGlobalTauri`：**未开启**（配置里没有这个键，默认 false）。所以 `window.__TAURI__` 不存在，
  `index.html` 的兜底脚本也刻意不碰它。
- `devUrl`：`http://localhost:1430`，`pnpm dev` 走 Vite。**开发期不套 CSP**（未设 `devCsp`）：
  Vite HMR 要 `ws://localhost:1431`、要注入内联样式，套上来只会让开发时满屏 violation。
  发布产物才走 `tauri://localhost`，CSP 在这里生效。
- `eval` / `new Function`：`src` 里只有一处 `Function("specifier","return import(specifier)")`
  （`src/lib/otel/tracer.ts:30` 的动态 import 垫片）。它**只在 `OTEL_TRACES_EXPORTER=jsonl` 且
  运行在 Node 时**才被调用，而 WebView 里 `isNodeRuntime()` 恒为 false → 走不到。
  所以不需要 `'unsafe-eval'`。构建产物里也没有 `eval(`/`new Function(`。
- `blob:`：`src/vendor/redline-core/viewers/util.ts` 在宿主没给 `srcUrl` 时用
  `URL.createObjectURL` 造预览 URL（图片 → `img-src`，HTML → `frame-src`）。
- 内联样式：`style={{…}}` 共 60 处（React 动态宽度/颜色/进度），必须允许 `'unsafe-inline'`（见 §4.1）。
- 内联事件处理器：**没有任何业务代码依赖**；只有白屏兜底页原先在 `innerHTML` 里写了
  `onclick=`，已被 `script-src-attr` 拦掉（真机实测），现改为 `addEventListener`（见 §12.1）。
- 外部字体 / 远程图片 / 远程脚本：**没有**。构建后的 CSS 里 `url()` 为 0 条，`index.html` 只有
  1 个内联脚本 + 1 个同源 module 脚本，`miniMd` 也不渲染 `![]()` 图片。

## 4. 逐条说明

| 指令 | 值 | 理由 |
|---|---|---|
| `default-src` | `'self'` | 兜底同源；没写到的指令都退回这里，不会「忘了写 = 放开」 |
| `script-src` | `'self'` | 只有 `dist/assets/*.js`。内联脚本由 Tauri 编译期自动加 `sha256`（见 §5），**所以不需要 `'unsafe-inline'`** |
| `style-src` | `'self' 'unsafe-inline'` | §4.1 |
| `img-src` | `'self' data: blob: asset: http://asset.localhost` | 打包图、`data:` 二维码/作图历史、`blob:` 预览、`asset:` 本地文件 |
| `media-src` | `'self'` + asset | `Video.tsx` 的 `<video src={convertFileSrc(path)}>` |
| `font-src` | `'self'` | 无 web font（系统字体栈），显式写明而不靠兜底 |
| `connect-src` | `'self'` + `ipc:` + `http://ipc.localhost` + asset + `data:` + `http://127.0.0.1:4318` | §3.1 的四个来源 |
| `frame-src` | `'self'` + asset + `blob:` + `http://127.0.0.1:3080` | DSH 工作台 iframe、HTML 预览 iframe |
| `worker-src` | `'self'` | pdf.js 的 worker URL 是同源的（`PdfViewer.tsx` 用 `new URL(..., import.meta.url)`）；即便它退回「主线程假 worker」，走的也是同源动态 `import`，同样被 `script-src 'self'` 覆盖 |
| `object-src` | `'none'` | 全仓没有 `<object>`/`<embed>`，直接关死插件类内容 |
| `base-uri` | `'self'` | 防 `<base>` 劫持把相对 URL 指到别处；页面本身没有 `<base>` |
| `form-action` | `'none'` | 全仓没有 `<form>`，提交无处可去 |
| `frame-ancestors` | `'none'` | 自己的页面不该被任何人框住（Tauri 以 HTTP 头下发，这条在头里有效） |

### 4.1 `style-src 'unsafe-inline'`：确实是妥协

React 的 `style={{…}}` 会变成元素上的 `style="…"` 属性，属 CSP2 的 `style-src` 管辖，不给
`'unsafe-inline'` 就直接丢样式（进度条宽度、作图取色、图表配色全歪）。60 处动态样式改成 class
或 CSS 变量是大工程，不属「最小可用 CSP」，所以先放开，并在 §6 记为待收口项。

实际风险比字面小：注入出来的 CSS 想外带数据得靠 `url(…)`，而 `url()` 在 CSP 里归 `img-src`/`font-src`
管——本策略这两项都没有任意 `https:`，所以「CSS 注入 → 外发」这条链是断的。

## 5. 关键机制：内联脚本的 `sha256` 是 Tauri 自动加的

`index.html` 里有一段**内联**兜底脚本（首屏白屏时显示可读提示并上报，`transformIndexHtml` 会把
`%APP_VERSION%` 换成版本号）。按常规认知，内联脚本要么 `'unsafe-inline'`，要么手写 hash——
但版本号每次发版都变，手写 hash 一定会过期，过期后兜底脚本被静默拦掉，正好回到它要解决的白屏。

Tauri 2 在**编译期**把这件事做了：`tauri-codegen` 对每个 HTML 资产执行
`inject_script_hashes()`，把 `script:not(:empty)` 的内容（CRLF 归一化后）算 `sha256`，
运行时由 `set_csp()` 把 `'sha256-…'` 追加进 `script-src`（顺带保证有 `'self'`）。
也就是说：

- 配置里写 `script-src 'self'` 就够了，内联脚本不会挂；
- hash 随每次构建重算，发版不用改配置；
- **但它依赖 `dangerousDisableAssetCspModification` 保持默认（false）**。哪天有人为了「手动控制」
  把它设成 `true`，这段兜底脚本就会被静默拦掉——这是本策略唯一「不写在配置里」的隐含前提。

自查这句话对不对，一条命令即可（构建后）：

```bash
python3 - <<'PY'
import re,hashlib,base64
h=open('dist/index.html',encoding='utf-8').read()
for s in re.findall(r'<script(?![^>]*\bsrc=)[^>]*>(.*?)</script>',h,re.S):
    n=s.replace('\r\n','\n').replace('\r','\n')
    print("'sha256-"+base64.b64encode(hashlib.sha256(n.encode()).digest()).decode()+"'")
PY
```

1.4.10 这次构建算出来是 `'sha256-cMUQ+ixr/j3DefAjq/kazL874K+cXXHsPmTgul3waI0='`（版本号一变就变）。

## 6. 妥协与已知风险

| # | 项 | 性质 | 以后怎么收口 |
|---|---|---|---|
| 1 | `style-src 'unsafe-inline'` | 中：允许样式注入（但外发链被 `img-src`/`font-src` 掐断） | 60 处 `style={{…}}` 换 class/CSS 变量后删掉 |
| 2 | `connect-src data:` | 低：只影响 `fetch('data:…')`，不产生网络出口 | `Draw.tsx` 的 dataURL→Blob 改成直接解码可去掉 |
| 3 | `connect-src http://127.0.0.1:4318` | 低：本机 OTLP 采集器；本机任意进程占这个端口就能收到 trace | 只在显式开 OTLP 时有意义，可考虑改成「按需从配置拼」 |
| 4 | `frame-src http://127.0.0.1:3080` | 低～中：DSH 网页工作台是**本机第三方服务**，框里的页面按自己的 CSP 跑 | 端口收紧到常量即可（已是常量）；后续可考虑改成独立窗口 |
| 5 | 没有 `report-uri`/`report-to` | 低：违规只在 DevTools console 里，收集不到 | 需要的话走 Tauri 自有日志通道，别引第三方收集端点 |
| 6 | `asset:` 来源 | 低：`asset:` 是另一个 origin，用户文件**不能**被当脚本执行（不在 `script-src`） | — |
| 7 | 未设 `devCsp` | 设计如此：开发期不套 CSP，避免 HMR/内联样式假报警 | 真要开，得加 `ws://localhost:1431` 与 `'unsafe-inline'` |
| 8 | `script-src` 无 `'wasm-unsafe-eval'` | 现状无影响：构建产物里 0 处 `wasm`。pdf.js 的 jbig2/openjpeg wasm 没被 Vite 收进 dist（本来就没被引用） | 若以后要真正启用 pdf.js wasm（JPX/JBIG2 图片），需**同时**补 wasm 资产与 `'wasm-unsafe-eval'` |

> capabilities 侧（`shell:allow-open` 可开任意 URL、`core:webview:allow-create-webview-window`）
> 不属本文范围：那是权限面，CSP 拦不住，另开 PR。

## 7. 自查：csp_evaluator 1.1.8 实测

用 Google 的评估器（`npm i csp_evaluator`，与 csp-evaluator.withgoogle.com 同一套规则）跑本策略，
结果 **10 条 findings**，逐条判定如下。不追求「零 finding」的洁癖结论——该保留的保留，理由写清楚。

| 严重度 | 指令 | 评估器原话 | 本仓判定 |
|---|---|---|---|
| HIGH(50) | `script-src` | `'self'` can be problematic if you host JSONP, AngularJS or user uploaded files | **可接受**。该 origin（`tauri://localhost`）只服务打包产物：没有 JSONP 端点、没有 AngularJS；用户导入的文件走的是**另一个** origin `asset:`，且 `asset:` 不在 `script-src` 里——反过来把它加进来才是真危险 |
| MEDIUM(60) | `connect-src` | connect-src 允许 localhost 作为来源，生产环境应移除 | **保留**。`127.0.0.1:4318` 是本机 OTLP 采集器，仅显式开 `OTEL_TRACES_EXPORTER=otlp` 时用到；桌面应用的「生产环境」就是客户机本身，localhost 在这里是功能目标而非攻击面 |
| MEDIUM(60) | `frame-src` | 同上 | **保留**。`127.0.0.1:3080` 是 DSH 网页工作台，本机服务、端口是前端常量 |
| LOW(30) ×7 | `img-src` / `media-src` / `connect-src` / `frame-src` | Allow only resources downloaded over HTTPS | **无法遵守**。`http://asset.localhost` 与 `http://ipc.localhost` 是 Tauri 在 Windows 上的固定 host，不是可选项；本地文件也没有「HTTPS 传输」这一说 |

注意：评估器**没有**报 `style-src 'unsafe-inline'`——它只对 `script-src` 的 `unsafe-inline` 出 finding。
所以 §6 第 1 条不能因为「工具没报」就当没事：那条是靠 `img-src`/`font-src` 里没有任意 `https:`
把「CSS 注入 → 外发」的链路掐断来兜的，仍记为待收口项。

复现：

```bash
cd /tmp && npm i csp_evaluator
node -e 'const {CspParser}=require("csp_evaluator/dist/parser.js");
const {CspEvaluator}=require("csp_evaluator/dist/evaluator.js");
const fs=require("fs");
const csp=JSON.parse(fs.readFileSync("src-tauri/tauri.conf.json")).app.security.csp;
for (const f of new CspEvaluator(new CspParser(csp).csp).evaluate())
  console.log(f.severity, f.directive, f.description, f.value||"");'
```

（严重度数值：10=INFO / 30=LOW / 50=HIGH / 60=MEDIUM，与网页版一致。）

## 8. 以后新增 API 域名，改哪里

只有一处：`src-tauri/tauri.conf.json` → `app.security.csp` 里对应指令的来源列表。

判断该不该加，先问一句 **「这个请求是 WebView 发的，还是 Rust 发的？」**：

- **Rust 发的（绝大多数）**：`installer::curl` / `agent::chat.rs` / `tools.rs` / `dshdesk.rs` 之类，
  **不要加**任何 CSP。CSP 管不到进程外的 curl，加了只是无谓放宽。
- **浏览器打开的**：`openUrl()`（opener 插件）→ 也**不要加**，系统浏览器不受 WebView CSP 约束。
- **WebView 自己 `fetch`/`<img>`/`<iframe>` 的**：才需要加，并且优先精确到 `主机[:端口]`，
  不要用 `https:` 通配。

## 9. 真机验证（必须做）

CSP 在构建期不校验，本地 `pnpm build` 通过**不能**证明运行时不被拦。需要属主在
Windows / macOS 真机上启动一次，看 DevTools console：

1. 启动应用（画像同平常：装机流程、对话、文件预览都点一遍）。
2. Windows：WebView2 已带 `--remote-debugging-port=9222`（见 `tauri.conf.json`
   `additionalBrowserArgs`），浏览器开 `http://127.0.0.1:9222` 看 console；
   macOS：WebView 右键 →「检查元素」。
3. 搜 `Content Security Policy` / `Refused to`。**0 条**才算过。
4. 重点回归：① 首屏正常（内联兜底脚本没被误拦，正常时它不显示）；② 文档/视频预览出图；
   ③ DSH 工作台 iframe 能开；④ 作图历史缩略图出图；⑤ 终端窗口（`uking-term-*`）正常。
5. 想确认策略真的生效：看 Network 里 `index.html` 的**响应头** `Content-Security-Policy`
   （Tauri 走 HTTP 头，不是 `<meta>`，所以 `document.querySelector('meta[http-equiv]')` 是 null，
   别被这个误导），应能看到 §2 的内容**外加** Tauri 注入的 `'sha256-…'`。

拦了什么、贴 console 原文到 PR / issue 即可定位到具体指令。

## 10. 回滚

改回 `null` 即可，无迁移、无数据影响：

```json
"security": { "csp": null }
```

（`assetProtocol` 保持不动——它是 `convertFileSrc` 用的，与 CSP 无关。）

## 11. 参考

- Tauri v2 CSP：<https://v2.tauri.app/security/csp/>
- Tauri 源码：`tauri-codegen/src/context.rs::inject_script_hashes`、
  `tauri/src/manager/mod.rs::set_csp` / `replace_csp_nonce`
- 本仓：`src-tauri/tauri.conf.json`（唯一改动点）、`src-tauri/capabilities/default.json`（未改）

## 12. 运行时验证结果（2026-11-03，macOS 真机）

用「探针版」构建（`tauri.conf.json` 与 dist 与发布版**完全一致**，只额外注入一段监听
`securitypolicyviolation` 并把事件写到系统临时目录的内联脚本；探针不进 main）在
macOS / WKWebView 上启动验证：

- **非预期违规 0 条**（启动 → 首屏渲染完成）。探针自己产生的 2 条反向对照不计入。
- 正向能力全部通过：IPC 往返、`data:` fetch、`blob:` 图片解码、同源 fetch、`asset:` fetch
  （`~/.uking/video` 夹具）、内联样式、`addEventListener`、PTY 分配。
- 反向对照 2 条都按预期被拦：`connect-src` 拦 https 外连、`script-src-attr` 拦内联 `onclick`。

### 12.1 这次实测发现并修掉的两个真问题（白屏兜底页）

CSP 之前，`index.html` 的兜底页靠两样东西自救/上报，两者在这条策略下都会失效：

1. `reportBootFailure()` 用 `XMLHttpRequest` 直连 `https://u-claw.org.cn/uking/bug`
   → 被 `connect-src` 拦掉，**白屏上报静默丢失**（而且白屏恰恰是最需要上报的路径）。
   修：改走宿主注入的 `__TAURI_INTERNALS__.invoke("report_bug", …)`；Rust 侧先落
   `~/.uking/logs/crash.log`、再用系统 curl 发网络，CSP 不必放行任何 https 来源。
2. 兜底页两个按钮写成 `innerHTML` 里的 `onclick=` → 被 `script-src-attr` 拦掉，
   **「重新加载 / 下载最新版」点了没反应**（真机实测：handler 一次都没触发）。
   修：改 `id` + `addEventListener`。内联**脚本**仍允许（Tauri 自动加 `sha256`），
   只是不允许内联**事件处理器**。

两处都保持 ES5、都不依赖前端包（`__TAURI_INTERNALS__` 由宿主注入，主包加载失败时仍在）。

### 12.2 验证方法（可复现）

探针构建启动后读取系统临时目录的 `uking-csp-probe.json`（macOS：`$(getconf
DARWIN_USER_TEMP_DIR)uking-csp-probe.json`；Windows：`%TEMP%\uking-csp-probe.json`），
`summary.unexpected` 应为 0，`checks` 全 `ok`。反向对照的 2 条违规带 `synthetic: true`。
