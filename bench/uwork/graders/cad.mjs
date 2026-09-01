/**
 * 判分：办公室平面图 .dxf。
 *
 * 判的是「这份图纸拿到 CAD 里打得开、且画的是要求的东西」，不是「文件存在」。
 * DXF 是成对的 (组码, 值) 文本流，这里自己解——判分器不能依赖客户机没有的 unzip/CAD。
 */
import fs from "node:fs";
import path from "node:path";

/** DXF → [{code, value}]，再切出 ENTITIES 段里的实体。 */
function parseDxf(text) {
  const lines = text.split(/\r?\n/);
  const pairs = [];
  for (let i = 0; i + 1 < lines.length; i += 2) pairs.push({ code: lines[i].trim(), value: lines[i + 1] });
  const ents = [];
  let inEnt = false, cur = null;
  for (const p of pairs) {
    if (p.code === "2" && p.value === "ENTITIES") { inEnt = true; continue; }
    if (!inEnt) continue;
    if (p.code === "0") {
      if (p.value === "ENDSEC") break;
      if (cur) ents.push(cur);
      cur = { type: p.value, props: {} };
    } else if (cur) {
      (cur.props[p.code] ||= []).push(p.value);
    }
  }
  if (cur) ents.push(cur);
  const layers = [];
  let inLayerTable = false;
  for (let i = 0; i < pairs.length; i++) {
    if (pairs[i].code === "2" && pairs[i].value === "LAYER") inLayerTable = true;
    else if (inLayerTable && pairs[i].code === "0" && pairs[i].value === "ENDTAB") inLayerTable = false;
    else if (inLayerTable && pairs[i].code === "2") layers.push(pairs[i].value);
  }
  return { pairs, ents, layers: [...new Set(layers)] };
}
/** `\U+4F1A` 还原成汉字，好跟中文断言比对。 */
const unesc = (s) => String(s).replace(/\\U\+([0-9A-Fa-f]{4})/g, (_, h) => String.fromCodePoint(parseInt(h, 16)));

export async function grade({ ws }) {
  const checks = [];
  const add = (n, ok, d) => checks.push({ name: n, ok: !!ok, detail: d || "" });

  const files = fs.readdirSync(ws);
  const dxf = files.find((f) => f.toLowerCase().endsWith(".dxf"));
  add("生成了 .dxf", !!dxf, dxf ? dxf : `目录里没有 .dxf（有: ${files.join(", ") || "空"}）`);
  if (!dxf) return { pass: false, checks };

  const text = fs.readFileSync(path.join(ws, dxf), "utf8");
  let d;
  try { d = parseDxf(text); } catch (e) { add("DXF 结构可解析", false, e.message); return { pass: false, checks }; }

  add("DXF 结构完整（有 ENTITIES 且以 EOF 收尾）", d.ents.length > 0 && /\bEOF\b/.test(text.slice(-40)),
      `实体 ${d.ents.length} 个`);
  add("画了足够的图元（≥8 个实体）", d.ents.length >= 8, `实际 ${d.ents.length} 个`);

  const geo = d.ents.filter((e) => ["LINE", "POLYLINE", "LWPOLYLINE", "CIRCLE", "ARC"].includes(e.type));
  add("有几何图元（墙/家具不是纯文字堆的）", geo.length >= 6, `几何实体 ${geo.length} 个`);

  const textEnts = d.ents.filter((e) => e.type === "TEXT" || e.type === "MTEXT");
  const labels = textEnts.map((e) => unesc((e.props["1"] || []).join(""))).join(" | ");
  add("标了「会议室」", /会议室/.test(labels), labels ? `图上文字: ${labels}` : "图上一个文字都没有");
  add("标了「办公区」", /办公区/.test(labels), "");
  add("标了总长尺寸 12000", /12000/.test(labels) || /12000/.test(text), "");

  const userLayers = d.layers.filter((l) => l !== "0");
  add("分了图层（≥2 个自定义层）", userLayers.length >= 2, `图层: ${d.layers.join(", ")}`);

  // 尺寸大致对：外墙 12000×8000。取所有坐标的极值。
  const xs = [], ys = [];
  for (const e of d.ents) {
    for (const c of ["10", "11"]) for (const v of e.props[c] || []) if (!isNaN(+v)) xs.push(+v);
    for (const c of ["20", "21"]) for (const v of e.props[c] || []) if (!isNaN(+v)) ys.push(+v);
  }
  const w = xs.length ? Math.max(...xs) - Math.min(...xs) : 0;
  const h = ys.length ? Math.max(...ys) - Math.min(...ys) : 0;
  add("尺寸按毫米画（外墙约 12000×8000）", w >= 11000 && w <= 16000 && h >= 7000 && h <= 12000,
      `实际范围 ${Math.round(w)} × ${Math.round(h)}${w < 100 ? "（按米画的？CAD 里会小得看不见）" : ""}`);

  // 🔴 字太小 = 等于没标。文件、图层、实体数可以全对，客户打开一看「尺寸呢？」——
  // 这正是「格式完全正常、客户看不出问题在哪」那一类，必须硬判。
  // 经验值：字高小于图幅的 1/300 在 A3 上就已经看不清了。
  const heights = textEnts.map((e) => +((e.props["40"] || [])[0] || 0)).filter((v) => v > 0);
  const span = Math.max(w, h);
  const tooSmall = heights.filter((v) => v < span / 300);
  add("图上文字大小可读（不是看不见的小点）", heights.length > 0 && tooSmall.length === 0,
      heights.length ? `字高 ${heights.map((v) => Math.round(v * 10) / 10).join("/")}，图幅 ${Math.round(span)}${tooSmall.length ? ` —— ${tooSmall.length} 处小于图幅 1/300，打开等于没标` : ""}` : "取不到字高");

  const preview = files.find((f) => /\.(svg|png)$/i.test(f));
  add("（加分项）出了预览图，客户没装 CAD 也看得见", !!preview, preview || "只给了 .dxf——客户机大多没装 CAD");

  const hard = checks.filter((c) => !c.name.startsWith("（加分项）"));
  return { pass: hard.every((c) => c.ok), checks };
}
