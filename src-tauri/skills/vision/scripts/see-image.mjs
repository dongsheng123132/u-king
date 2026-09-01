#!/usr/bin/env node
// U-King 看图 · OCR / 图像理解 / 定位 —— 给「只会文字」的 DeepSeek 当眼睛。
// 直连虾盘云默认 **国产 qwen3.7-flash**，不通/拒答时自动换替补 qwen3.7-plus（跑道实测，见下）。
// 零 npm 依赖（node 内置 + 系统 curl）。
// 用法见同目录 ../SKILL.md。stdout 出结果（--json 出 {ok,text,...}，否则纯文字）；stderr 出错误；退出码 0/1/2。
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdtempSync, rmSync, statSync, existsSync, readdirSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import https from "node:https";

const BASE = "https://api.u-claw.org.cn"; // 国内可达域名（.org 子域被 GFW SNI 阻断，见 CLAUDE.md）
// 默认识图模型：**国产 qwen3.7-flash**。
// 依据 = `node scripts/bench.mjs`（三类合成夹具、每格跑 3 遍的客观命中率）：
//   证照字段 / 大图小字 / 长截图 三项合计 —— qwen3.7-flash 100%，qwen3-vl-flash 97%，
//   qwen-vl-max 91%，qwen-vl-plus 86%，qwen3.5-ocr 69%，MiniMax-M3 66%。
//   长截图（2400×2908）一项尤其分化：qwen3.7-flash 4/4，qwen3-vl-flash 44%，qwen-vl-max 13%。
// 旧默认 MiniMax-M3 在宽截图上会**整页编造**（泛问三遍全 0/7，还编出不存在的按钮和账号），
// 比漏读更危险 —— 换默认就是为了这条。想要更快可 `--model qwen3-vl-flash`（97%，快约 30%）。
const DEFAULT_MODEL = "qwen3.7-flash";
// 替补链：主力那条路不通（报错/超时/拒答）时依次顶上，**只在没显式 --model 时才启用**。
// 两棒各挡一类故障，别把它们看成「第二好」和「第三好」：
//
//   ① qwen3.7-plus —— 挡**单个模型**抽风。同一套夹具 2026-08-16 实测 95%
//      （证照 8/8、大图小字 7/7、长截图 3/4），是主力之后最准的一档，且快（4~6s）。
//      ⚠️ 跟主力同属阿里百炼那条路由，整条腿断时它一起断。
//   ② kimi-k3 —— 挡**整条路由断**。在虾盘云上是**裸名**（=直连渠道，月之暗面自己的
//      账号和端点；带 `moonshotai/` 前缀的那个才是中转），所以厂商故障 / 欠费 / 限流
//      跟阿里那条互不影响。这是它排在这里的唯一理由，**不是因为它更准**。
//
// 🔴 kimi-k3 的真实数字，别被单跑一遍骗了（我就差点）：单跑合计 100% 很漂亮，
//    长截图复跑 3 遍是 **泛问 3.0(1~4)/4、耗时中位 61.2s、峰值 129.1s**。
//    带意图（`--ask`，SKILL.md 推荐的主用法）倒是稳定 4/4。
//    所以它只配当最后一棒，且逼出了下面这条总预算——否则三棒串起来能假死 9 分钟。
const DEFAULT_MODEL_FALLBACKS = ["qwen3.7-plus", "kimi-k3"];
const TIMEOUT_MS = 180000; // 单棒上限。长截图实测中位 29s，给足余量
// 整条链的总预算。没有它的话最坏情况 = 3×180s = 9 分钟，用户端早就当死了
// （「卡住」是我们最常收到的 bug 描述之一，pc-***）。剩不下 15s 就不再起新的一棒。
const CHAIN_BUDGET_MS = 240000;

const BOOL = new Set(["json", "quiet", "ocr"]);
function parseArgs(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith("--")) {
      const k = a.slice(2);
      if (BOOL.has(k)) { out[k] = true; continue; }
      out[k] = i + 1 < argv.length && !argv[i + 1].startsWith("--") ? argv[++i] : true;
    } else out._.push(a);
  }
  return out;
}

// Key 优先级：--key > 环境变量 XIAPAN_API_KEY > ~/.uking/device.json（脚本内不含任何 Key）
function resolveKey(args) {
  if (args.key && args.key !== true) return String(args.key);
  if (process.env.XIAPAN_API_KEY) return process.env.XIAPAN_API_KEY;
  try {
    const j = JSON.parse(readFileSync(join(homedir(), ".uking", "device.json"), "utf8"));
    if (j && j.key) return j.key;
  } catch {}
  return "";
}

let QUIET = false, JSONMODE = false;
function logE(...m) { if (!QUIET) process.stderr.write(m.join(" ") + "\n"); }
function done(obj, code = 0) {
  if (JSONMODE) process.stdout.write(JSON.stringify(obj) + "\n");
  else if (obj.ok) process.stdout.write((obj.text || "") + "\n");
  else process.stderr.write("错误：" + (obj.error || "未知") + "\n");
  process.exit(code);
}
function fail(msg, code = 1) { done({ ok: false, error: String((msg && msg.message) || msg) }, code); }

// 从文件头魔数认 MIME（只认视觉 API 收的 png/jpg/webp/gif；认不出按 png 兜底）。
function sniffMime(buf) {
  if (buf.length >= 3 && buf[0] === 0xff && buf[1] === 0xd8 && buf[2] === 0xff) return "image/jpeg";
  if (buf.length >= 8 && buf[0] === 0x89 && buf[1] === 0x50 && buf[2] === 0x4e && buf[3] === 0x47) return "image/png";
  if (buf.length >= 12 && buf.toString("ascii", 0, 4) === "RIFF" && buf.toString("ascii", 8, 12) === "WEBP") return "image/webp";
  if (buf.length >= 4 && buf.toString("ascii", 0, 3) === "GIF") return "image/gif";
  return "image/png";
}

// 读图片原始尺寸（纯 std，不解码像素）。定位/裁剪都要它——
// 模型返回的坐标是归一化的，没有原图宽高就换算不回像素。
function imageSize(buf) {
  // PNG: IHDR 紧跟 8 字节签名
  if (buf.length >= 24 && buf.toString("ascii", 12, 16) === "IHDR")
    return { w: buf.readUInt32BE(16), h: buf.readUInt32BE(20) };
  // GIF: 逻辑屏幕宽高，小端
  if (buf.length >= 10 && buf.toString("ascii", 0, 3) === "GIF")
    return { w: buf.readUInt16LE(6), h: buf.readUInt16LE(8) };
  // WEBP: VP8X / VP8L / VP8
  if (buf.length >= 30 && buf.toString("ascii", 8, 12) === "WEBP") {
    const fourcc = buf.toString("ascii", 12, 16);
    if (fourcc === "VP8X") return { w: (buf.readUIntLE(24, 3) & 0xffffff) + 1, h: (buf.readUIntLE(27, 3) & 0xffffff) + 1 };
    if (fourcc === "VP8 ") return { w: buf.readUInt16LE(26) & 0x3fff, h: buf.readUInt16LE(28) & 0x3fff };
    if (fourcc === "VP8L") {
      const b = buf.readUInt32LE(21);
      return { w: (b & 0x3fff) + 1, h: ((b >> 14) & 0x3fff) + 1 };
    }
  }
  // JPEG: 扫 SOF 段（跳过 SOF4/SOF8/SOF12 这些非帧标记）
  if (buf.length >= 4 && buf[0] === 0xff && buf[1] === 0xd8) {
    let i = 2;
    while (i + 9 < buf.length) {
      if (buf[i] !== 0xff) { i++; continue; }
      const m = buf[i + 1];
      if (m >= 0xc0 && m <= 0xcf && m !== 0xc4 && m !== 0xc8 && m !== 0xcc)
        return { w: buf.readUInt16BE(i + 7), h: buf.readUInt16BE(i + 5) };
      i += 2 + (i + 3 < buf.length ? buf.readUInt16BE(i + 2) : 0);
    }
  }
  return null;
}

// 按像素框裁一刀 —— 「二次取证」的执行端：先 --locate 拿到框，再回来看这一块。
// 用 ffmpeg（全格式通吃、U-King「厨具工具箱」里就有），没装就明说，不静默降级。
function cropToFile(src, box) {
  const [x1, y1, x2, y2] = box;
  const w = Math.max(1, Math.round(x2 - x1)), h = Math.max(1, Math.round(y2 - y1));
  const probe = spawnSync("ffmpeg", ["-version"], { stdio: "ignore" });
  if (probe.error || probe.status !== 0)
    fail("--region 需要 ffmpeg（U-King「厨具工具箱」里可一键装；或 winget install ffmpeg）。", 2);
  const dir = mkdtempSync(join(tmpdir(), "uking-crop-"));
  const out = join(dir, "region.png");
  const r = spawnSync("ffmpeg", ["-y", "-loglevel", "error", "-i", src,
    "-vf", `crop=${w}:${h}:${Math.max(0, Math.round(x1))}:${Math.max(0, Math.round(y1))}`, out],
    { encoding: "utf8", timeout: 60000 });
  if (r.status !== 0) { rmSync(dir, { recursive: true, force: true }); fail("裁剪失败：" + String(r.stderr || "").slice(0, 200)); }
  return { file: out, dir };
}

// 本地文件 → data URL；http(s) 链接原样透传给 image_url。
function toImageUrl(src) {
  if (/^https?:\/\//i.test(src) || /^data:image\//i.test(src)) return src;
  let buf;
  try { buf = readFileSync(src); }
  catch { fail(`读不到图片文件：${src}`, 2); }
  const mb = buf.length / 1024 / 1024;
  if (mb > 10) logE(`⚠️ 图片 ${mb.toFixed(1)}MB 偏大（建议 ≤8MB、长宽 ≤4096），可能被上游拒或变慢。`);
  return `data:${sniffMime(buf)};base64,${buf.toString("base64")}`;
}

function hasCurl() {
  try { return spawnSync("curl", ["--version"], { stdio: "ignore" }).status === 0; }
  catch { return false; }
}
const CURL = hasCurl();

function httpsPost(path, key, bodyObj, timeoutMs) {
  return new Promise((res, rej) => {
    const u = new URL(BASE + path);
    const data = Buffer.from(JSON.stringify(bodyObj));
    const req = https.request(
      { method: "POST", hostname: u.hostname, port: 443, path: u.pathname,
        headers: { Authorization: `Bearer ${key}`, "Content-Type": "application/json", "Content-Length": data.length },
        timeout: timeoutMs },
      (r) => {
        const chunks = [];
        r.on("data", (c) => chunks.push(c));
        r.on("end", () => { const t = Buffer.concat(chunks).toString("utf8"); try { res(JSON.parse(t)); } catch { rej(new Error("响应不是 JSON：" + t.slice(0, 200))); } });
      }
    );
    req.on("error", rej);
    req.on("timeout", () => req.destroy(new Error("请求超时")));
    req.write(data); req.end();
  });
}
async function postChat(key, bodyObj, timeoutMs) {
  if (CURL) {
    const dir = mkdtempSync(join(tmpdir(), "uking-see-"));
    const bf = join(dir, "body.json");
    writeFileSync(bf, JSON.stringify(bodyObj)); // 大 base64 落文件 + --data @file，绕命令行长度/引号
    try {
      const r = spawnSync("curl", ["-sS", "-m", String(Math.ceil(timeoutMs / 1000)), "-X", "POST", BASE + "/v1/chat/completions",
        "-H", `Authorization: Bearer ${key}`, "-H", "Content-Type: application/json", "--data", `@${bf}`],
        { timeout: timeoutMs + 5000, maxBuffer: 32 * 1024 * 1024, encoding: "utf8" });
      if (r.error) throw r.error;
      if (r.status !== 0) throw new Error(`curl 退出码 ${r.status}：${String(r.stderr || "").slice(0, 200)}`);
      try { return JSON.parse(r.stdout || ""); } catch { throw new Error("接口响应不是 JSON：" + String(r.stdout).slice(0, 200)); }
    } finally { rmSync(dir, { recursive: true, force: true }); }
  }
  return httpsPost("/v1/chat/completions", key, bodyObj, timeoutMs);
}
function errOf(v) {
  if (v && v.error != null) return v.error.message || JSON.stringify(v.error);
  if (v && typeof v.code === "string" && v.code !== "success" && v.code !== "") return v.message || v.code;
  return null;
}

// ══ 纯文本模型闸门 ══
// 发图给纯文本模型**不会报错**，会拿到一个编出来的答案。2026-08-16 实测，同一张合成营业执照
// （`bench/fixtures/license.png`，法定代表人正解「张示例」），问「法定代表人姓名是什么」：
//   qwen-turbo  → 「张三」。凭空编一个名字，HTTP 200、无任何异常信号。
//   qwen-plus   → 泛问时还老实说「我无法查看图片」；**一旦带上具体问题就当常识题答 600 字**，
//                 全程不提自己看不见（还举了腾讯的例子）。所以正文特征只能当第二道网，
//                 真正的闸门必须在**发出去之前**按模型能力拦。
// 名单两个来源，缺一不可：
//   ① 我们自己实测（上面这两个）；
//   ② dsh 插件 `@earendil-works/pi-ai` 的 catalog —— 每个模型条目带 `input:["text"]` /
//      `["text","image"]`，是目前唯一一份机器可读的模态真相源。核对办法见 ../SKILL.md。
//   ②**覆盖不到 qwen-plus / qwen-turbo**（catalog 里压根没这两个 id），所以 ① 不能省。
// 拦错的代价是一条看得见的报错（说清该换哪个），放过的代价是一个查不出的假答案 —— 故从严。
const TEXT_ONLY = [
  /^qwen-(?:plus|turbo)$/i,              // 实测：一个装懂、一个编名字
  /^qwen3\.7-max$/i,                     // catalog text-only；同族 qwen3.7-plus 才收图
  /^qwen3-coder/i,
  /^(?:[\w.-]+\/)?deepseek(?!.*ocr)/i,   // 全系纯文本（deepseek-ocr 例外，那是识图的）
  /^(?:z-ai\/)?glm-5(?:\.\d+)?$/i,       // glm-5/5.1/5.2 纯文本；带 v 的 glm-5v-turbo 才收图
  /^(?:[\w.-]+\/)?minimax-m[12]/i,       // M3 才收图
];
const textOnlyRegex = (id) => TEXT_ONLY.some((re) => re.test(String(id || "").trim()));

// ══ 第二源：运行时查 catalog（补黑名单的 fail-open 缺口）══
//
// 上面那份手写清单是**黑名单**：名单外的模型一律放行。而这个脚本要防的失败模式
// （纯文本模型收下图、HTTP 200、返回一个编出来的答案）在**名单外的**纯文本模型上
// 一模一样会发生，第二道网（正文拒答句式）又只在泛问时管用 —— 上面那张表自己证明的。
// 所以「未知模型 + 具体问题」这条路以前是完全没人守的。
//
// dsh 的 `@earendil-works/pi-ai` catalog 是本机唯一一份机器可读的模态真相源
// （2026-08-16 实测：38 个文件 / 1,109 条 / 691 带 image、418 纯文本）。以前它只在
// **写代码时**被人工查一次、用来生成上面那份静态清单；现在运行时也查，缺口从
// 「名单外全放行」收窄到「名单外且 catalog 也不认识才放行」。实测能多拦 192 个。
//
// 🔴 三条铁律，改这段前先读：
//  1. **只用来加拦，绝不用来放行。**「不在 catalog 里」不等于纯文本 —— 我们自己的默认
//     `qwen3.7-flash`、替补链上的 `qwen3-vl-flash` / `qwen-vl-max` 都不在里面（实测）。
//     拿「不在里面」当证据会把主力路径当场拦死。
//  2. **同一裸名有多条时，必须条条都说纯文本才拦。** 559 个裸名里有 5 个自相矛盾
//     （如 `qwen3.6-plus` img6/txt1）—— 只要有一条说收图就放行，宁可漏不可误杀。
//  3. **任何异常一律当「不知道」放行。** catalog 是别人家的文件，dsh 没装、结构变了、
//     JSON 坏了都可能 —— 让一个第三方文件的意外把看图功能整个掐死，比它要防的问题更坏。
let catalogIdx = null;
function loadCatalog() {
  if (catalogIdx) return catalogIdx;
  catalogIdx = new Map(); // 裸名 → {img, txt} 计数
  try {
    const dir = join(homedir(), ".uking/runtime/node/node_modules/@deepseek-ai/dsh",
      "node_modules/@earendil-works/pi-ai/dist/providers/data");
    if (!existsSync(dir)) return catalogIdx;
    for (const f of readdirSync(dir)) {
      if (!f.endsWith(".json") || f === ".manifest.json") continue;
      const file = f.slice(0, -5).toLowerCase();
      let j;
      try { j = JSON.parse(readFileSync(join(dir, f), "utf8")); } catch { continue; }
      for (const outer of Object.keys(j || {})) {
        for (const id of Object.keys(j[outer] || {})) {
          const m = j[outer][id];
          if (!m || !Array.isArray(m.input)) continue;
          const canImage = m.input.includes("image");
          // 同一条按多种写法各记一次：catalog 里既有 `qwen/qwen-plus` 也有 `minimax.minimax-m2`，
          // 而我们这边传进来的是裸名。把厂商前缀（文件名 / 外层 key / 斜杠段）都剥一遍。
          const forms = new Set();
          const low = id.toLowerCase();
          forms.add(low);
          forms.add(low.slice(low.lastIndexOf("/") + 1));
          for (const pfx of [file + ".", String(outer).toLowerCase() + "."]) {
            for (const v of [...forms]) if (v.startsWith(pfx)) forms.add(v.slice(pfx.length));
          }
          for (const v of forms) {
            if (!v) continue;
            const cur = catalogIdx.get(v) || { img: 0, txt: 0 };
            if (canImage) cur.img++; else cur.txt++;
            catalogIdx.set(v, cur);
          }
        }
      }
    }
  } catch {
    /* 铁律 3：查不动就当不知道 */
  }
  return catalogIdx;
}

/** catalog 明确说「这个 id 的每一条都不收图」才返回 true；不认识 / 有矛盾 / 出错一律 false。 */
function catalogSaysTextOnly(id) {
  const raw = String(id || "").trim().toLowerCase();
  if (!raw) return false;
  const idx = loadCatalog();
  for (const key of [raw, raw.slice(raw.lastIndexOf("/") + 1)]) {
    const v = idx.get(key);
    if (v && v.txt > 0 && v.img === 0) return true; // 铁律 2
  }
  return false;
}

const textOnlyModel = (id) => textOnlyRegex(id) || catalogSaysTextOnly(id);

// 「收下了图但没看」—— 纯文本模型拿到 image_url **不报错**：HTTP 200、choices 齐全，
// 正文是一句人话「我看不了图」。只认 HTTP 状态或 ok 位的调用方会把这句当成识图结果
// 交给下游 DeepSeek，比报错难查得多。2026-08-16 实测（虾盘云同一张营业执照夹具）：
//   qwen-plus     → 200 +「我无法查看或分析图片内容。请您提供图片中的文字内容…」，命中 0/8
//   qwen3.7-plus  → 200 + 正确读出「测试样例贸易（示例市）有限公司 / 张示例」
// 名字很像但一个收图一个不收（qwen3.7-max 同样不收图），客户和 AI 都会选错，所以这里拦。
// 判据故意收窄：整段短（≤400 字）**且**拒答句式出现在开头 120 字内才算 —— 免得把
// 「图里本来就印着这句话」的正常转录误判成拒答（OCR 长文根本进不了这个分支）。
const REFUSAL_RE = [
  /(?:无法|不能|没法|不支持)[^。！\n]{0,8}(?:查看|看到|识别|读取|解析|处理|分析|浏览)[^。！\n]{0,6}(?:图片|图像|图|附件)/,
  /(?:看不到|看不了|读不了|无法接收)[^。！\n]{0,6}(?:图片|图像|图)/,
  /(?:不具备|没有)[^。！\n]{0,8}(?:视觉|识图|图像|多模态)[^。！\n]{0,6}(?:能力|功能)/,
  /\b(?:can(?:no|')?t|cannot|unable to|not able to)\b[^.\n]{0,40}\b(?:see|view|read|process|analy[sz]e)\b[^.\n]{0,30}\bimages?\b/i,
];
function refusalOf(text) {
  const t = String(text || "").trim();
  if (!t || t.length > 400) return null;
  const head = t.slice(0, 120);
  return REFUSAL_RE.some((re) => re.test(head)) ? t.slice(0, 100) : null;
}

const LOCATE_PROMPT = (what) =>
  `请在图中定位以下目标：${what}\n` +
  `只输出一个 JSON 数组，不要任何解释、不要 markdown 代码块。每项形如：\n` +
  `{"what":"目标的简短名称","bbox":[x1,y1,x2,y2],"text":"该区域里的文字（没有就空串）"}\n` +
  `找不到的目标就不要放进数组。`;

// 模型给的框用哪套坐标：Qwen-VL 系列返回 0~1000 归一化，别的可能给 0~1 或真实像素。
// 不猜——按图的实际宽高反推，并把判定结果作为 `space` 字段发出去，让调用方能核对。
function detectSpace(boxes, size) {
  const all = boxes.flat().filter((n) => Number.isFinite(n));
  if (!all.length) return "unknown";
  const max = Math.max(...all);
  if (max <= 1.001) return "0-1";
  if (!size) return max <= 1000 ? "0-1000" : "pixel";
  const overflowsPixels = boxes.some(([x1, y1, x2, y2]) =>
    x1 > size.w || x2 > size.w || y1 > size.h || y2 > size.h);
  if (overflowsPixels) return "0-1000";
  // 图本身比 1000 大、而所有坐标都挤在 0~1000 内 —— 归一化的可能性远大于「目标恰好都在左上角」
  if (max <= 1000 && (size.w > 1000 || size.h > 1000)) return "0-1000";
  return "pixel";
}
function toPixels(box, space, size) {
  if (!size || space === "pixel" || space === "unknown") return box.map((n) => Math.round(n));
  const sx = space === "0-1" ? size.w : size.w / 1000;
  const sy = space === "0-1" ? size.h : size.h / 1000;
  return [box[0] * sx, box[1] * sy, box[2] * sx, box[3] * sy].map((n) => Math.round(n));
}
function extractJson(text) {
  const fenced = text.match(/```(?:json)?\s*([\s\S]*?)```/i);
  const body = fenced ? fenced[1] : text;
  const s = body.indexOf("["), e = body.lastIndexOf("]");
  if (s === -1 || e <= s) return null;
  try { return JSON.parse(body.slice(s, e + 1)); } catch { return null; }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  QUIET = !!args.quiet; JSONMODE = !!args.json;
  const src = args._[0] || (typeof args.image === "string" ? args.image : "");
  if (!src) fail("用法：node see-image.mjs <图片路径或URL> [--ocr | --ask \"问题\" | --locate \"目标\"] [--region x1,y1,x2,y2] [--json]", 2);

  const isRemote = /^https?:\/\//i.test(src) || /^data:image\//i.test(src);
  let size = null;
  if (!isRemote) { try { size = imageSize(readFileSync(src)); } catch {} }

  // --region：先裁再看。给「上一轮定位到了、但字太小没读准」留的回头路。
  let shown = src, cropDir = null, region = null;
  if (typeof args.region === "string" && args.region) {
    if (isRemote) fail("--region 只支持本地图片文件（网络图请先下载）。", 2);
    region = args.region.split(",").map((s) => Number(s.trim()));
    if (region.length !== 4 || region.some((n) => !Number.isFinite(n)))
      fail("--region 格式：x1,y1,x2,y2（原图像素，左上角为原点）", 2);
    const c = cropToFile(src, region);
    shown = c.file; cropDir = c.dir;
    logE(`已裁出 ${Math.round(region[2] - region[0])}×${Math.round(region[3] - region[1])} 区域再看。`);
  }

  // 四种意图：--locate 出坐标；--ocr 逐字转录；--ask 回答具体问题；默认 描述+读字
  let prompt, mode;
  if (typeof args.locate === "string" && args.locate) { mode = "locate"; prompt = LOCATE_PROMPT(args.locate); }
  else if (args.ocr) { mode = "ocr"; prompt = "请把这张图里的所有文字**一字不差**地转录出来（OCR），保持原有换行与顺序，只输出文字本身，不要解释、不要加序号。"; }
  else if (typeof args.ask === "string" && args.ask) { mode = "ask"; prompt = args.ask; }
  else {
    mode = "describe";
    prompt = "请描述这张图片的内容；如果图里有文字（如截图、表格、发票、题目），把文字也一并准确读出来。";
    // 实测：泛泛地问，弱模型会整页编造；带着目的问，同一张图同一个模型能从 0/7 提到 5.7/7。
    logE("提示：知道自己要找什么时用 --ask \"…\"，命中率明显更高（见 bench）。");
  }

  // 显式 --model 一律照办、**不替他换**（选错了要能看见，不静默降级）；只有走默认时才挂替补。
  const explicitModel = (typeof args.model === "string" && args.model) ? args.model : "";
  const chain = explicitModel ? [explicitModel] : [DEFAULT_MODEL, ...DEFAULT_MODEL_FALLBACKS];
  // 闸门盖**整条链**，不只是 --model：这样以后谁把 DEFAULT/FALLBACK 改成纯文本模型也是当场炸，
  // 而不是无声无息开始编答案（实测 qwen-plus 带具体问题时把「张示例」答成「张三」，全程 ok:true）。
  const banned = chain.find(textOnlyModel);
  if (banned)
    fail(`${banned} 是纯文本模型，看不了图 —— 但它收下图片后**不会报错**，会给你一个编出来的答案。`
       + `请改用带视觉的：${DEFAULT_MODEL}（默认，最准）/ ${DEFAULT_MODEL_FALLBACKS.join(" / ")}（替补）/ `
       + `qwen3-vl-flash（更快）/ qwen-vl-max。`
       // 说清是哪一源拦的 —— 万一拦错了，才知道该去改哪份名单（手写清单在本文件；catalog 由 dsh 装）
       + `\n（判据来源：${textOnlyRegex(banned) ? "本脚本内置清单" : "dsh 的 pi-ai catalog——该模型 input 里没有 image"}）`
       + (banned === explicitModel ? "" : `\n（${banned} 来自脚本内置的默认/替补，这是配置错误，请修 see-image.mjs。）`), 2);
  const key = resolveKey(args);
  if (!key) fail("找不到 API Key（--key / 环境变量 XIAPAN_API_KEY / ~/.uking/device.json）。请先在 U-King 里领取或配置。", 2);

  const imageUrl = toImageUrl(shown);
  const bodyFor = (m) => ({
    model: m,
    messages: [{ role: "user", content: [
      { type: "text", text: prompt },
      { type: "image_url", image_url: { url: imageUrl } },
    ] }],
    max_tokens: mode === "locate" ? 800 : 1500,
    temperature: 0,
  });

  const t0 = Date.now();
  let model = null, text = null, fallbackFrom = null, lastErr = "";
  try {
    for (const m of chain) {
      // 总预算：剩不下 15s 就别再起新的一棒了 —— 与其让用户对着一个「还在跑」的界面
      // 再等三分钟，不如现在就把上一棒的真实错误告诉他。单棒仍受 TIMEOUT_MS 上限约束。
      const left = CHAIN_BUDGET_MS - (Date.now() - t0);
      if (m !== chain[0] && left < 15000) {
        logE(`  ⏱ 总预算 ${CHAIN_BUDGET_MS / 1000}s 用尽，不再试 ${chain.slice(chain.indexOf(m)).join(" / ")}。`);
        break;
      }
      logE(`看图中（模型 ${m}，${mode} 模式）…`);
      let resp;
      try { resp = await postChat(key, bodyFor(m), Math.min(TIMEOUT_MS, Math.max(15000, left))); }
      catch (err) { lastErr = `${m}：${String((err && err.message) || err)}`; logE("  ✖ " + lastErr); continue; }
      const e = errOf(resp);
      if (e) { lastErr = `${m}：${e}`; logE("  ✖ " + lastErr); continue; }
      const c = resp && resp.choices && resp.choices[0] && resp.choices[0].message && resp.choices[0].message.content;
      if (typeof c !== "string" || !c.trim()) {
        lastErr = `${m}：模型没返回文字（${JSON.stringify(resp).slice(0, 160)}）`;
        logE("  ✖ " + lastErr); continue;
      }
      const refused = refusalOf(c);
      if (refused) {
        lastErr = `${m} 收下了图但没看：「${refused}」—— 它多半是纯文本模型（qwen-plus / qwen3.7-max / deepseek-* 都是），换带视觉的（qwen3.7-flash / qwen3.7-plus / qwen3-vl-flash）再试。`;
        logE(`  ✖ ${m}：拒答（收图不报错，已判失败）`); continue;
      }
      model = m; text = c.trim();
      if (m !== chain[0]) { fallbackFrom = chain[0]; logE(`  ↳ 主力 ${chain[0]} 不可用，已换替补 ${m}。`); }
      break;
    }
  } finally { if (cropDir) rmSync(cropDir, { recursive: true, force: true }); }
  if (!model) fail(lastErr || "识图失败：没有可用的模型。");

  const out = { ok: true, text, model, mode, elapsed: ((Date.now() - t0) / 1000).toFixed(1) + "s" };
  if (fallbackFrom) out.fallback_from = fallbackFrom; // 报告里说清用的不是主力，别让降级隐身
  if (region) out.region = region.map(Number);
  if (size) out.size = size;

  if (mode === "locate") {
    const arr = extractJson(text);
    if (!Array.isArray(arr)) fail("定位失败：模型没返回可解析的 JSON 数组。原文：" + text.slice(0, 200));
    const boxes = arr.map((o) => (Array.isArray(o && o.bbox) ? o.bbox.map(Number) : [])).filter((b) => b.length === 4);
    const space = detectSpace(boxes, size);
    out.space = space; // 判定给出去，坐标对不对调用方能自己核
    out.items = arr.map((o) => {
      const bb = Array.isArray(o && o.bbox) && o.bbox.length === 4 ? o.bbox.map(Number) : null;
      return {
        what: (o && o.what) || "",
        text: (o && o.text) || "",
        bbox_raw: bb,
        // 换算到原图像素 —— 可直接喂回 `--region`
        bbox: bb ? toPixels(bb, space, size) : null,
      };
    });
    if (!JSONMODE) {
      process.stdout.write(out.items.map((i) =>
        `${i.what}\t[${(i.bbox || []).join(",")}]${i.text ? "\t" + i.text : ""}`).join("\n") + "\n");
      process.exit(0);
    }
  }

  done(out);
}
main().catch((err) => fail(err));
