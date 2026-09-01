// 图片修补 —— 动作实现（动作模块，不碰 DOM）。
//
// 这个文件被两处 import：web/index.html（GUI 面）和宿主的自带 Node（无头面）。
// 所以这里没有 canvas、没有 fetch、没有 fs —— 图像原语全走 ctx.uking.image.*，
// AI 调用全走 ctx.uking.ai.*（密钥在宿主进程里，这里永远看不到）。
//
// ────────────────────────────────────────────────────────────────
// 为什么是「裁剪 → 编辑 → 羽化回贴」，而不是把整张图丢给模型
//
// 上游 /v1/images/edits 没有 mask 参数。实测过：带 -F mask=@ 会 HTTP 200，
// 但掩码不被当成内绘掩码，反而把结果搞坏（目标区被涂黑、无关区域被删）。
// 所以几何本身就是掩码 —— 把目标裁出来居中送进去。
//
// 更硬的理由是分辨率：模型只吐 1024 见方。把用户 4000×3000 的照片整张送进去，
// 拿回来的是 1024 的糊图。裁剪回贴则原图分辨率一分不少，
// 而且框外每个像素逐字节不变 —— 这是算法保证，不是经验观察。
// ────────────────────────────────────────────────────────────────

const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));

/** 模型稳定接受的编辑尺寸。1024 是「所有模型都吃」的那一档。 */
const EDIT_SIZE = 1024;

/** 上下文环占选区最长边的比例。太小则模型没有纹理可参考，太大则无谓地把更多像素交给模型。 */
const PAD_RATIO = 0.35;

function contextBox(sel, W, H) {
  const pad = clamp(Math.round(PAD_RATIO * Math.max(sel.w, sel.h)), 24, 256);
  // 方形吸附：模型只吐正方形，送非方形进去会被拉伸。越界时平移而不是缩小。
  const S = clamp(Math.max(sel.w, sel.h) + 2 * pad, 256, Math.min(W, H));
  const cx = sel.x + sel.w / 2, cy = sel.y + sel.h / 2;
  return {
    x: clamp(Math.round(cx - S / 2), 0, W - S),
    y: clamp(Math.round(cy - S / 2), 0, H - S),
    w: S, h: S, pad,
  };
}

function normRegion(region, W, H) {
  const x = clamp(Math.round(region.x), 0, W - 1);
  const y = clamp(Math.round(region.y), 0, H - 1);
  const w = clamp(Math.round(region.w), 1, W - x);
  const h = clamp(Math.round(region.h), 1, H - y);
  if (w < 4 || h < 4) throw new Error("invalid_input: 选区太小了，拖大一点");
  return { x, y, w, h };
}

/**
 * 环带统计（选区外一圈的逐通道中位数与标准差）交给宿主算。
 * 中位数抗字形边缘的抗锯齿，均值不抗 —— 这点很要紧，所以不能图省事用均值。
 * 之所以不在这里自己算：一个 528×528 的环带是 110 万像素，塞进 JSON 送过来
 * 又慢又占内存。宿主那边是原生数组，顺手就算完了。
 */
const ringStats = (uking, id, rect, inner) => uking.image.ringStats(id, rect, inner);

/** 宿主的 pixels 返回 base64（比 JSON 数字数组小 4 倍以上），这里解回字节。 */
function bytesOf(res) {
  const b = atob(res.rgba_b64);
  const u = new Uint8Array(b.length);
  for (let i = 0; i < b.length; i++) u[i] = b.charCodeAt(i);
  return u;
}

// ═══════════════════════════ 去水印 ═══════════════════════════

export async function watermarkRemove(input, ctx) {
  const { uking } = ctx;
  const src = await uking.image.decode(input.image);
  const sel = normRegion(input.region, src.w, src.h);
  const box = contextBox(sel, src.w, src.h);

  await uking.ui?.progress?.(10, "裁剪选区");
  const crop = await uking.image.crop(src.id, box);
  const sent = await uking.image.resize(crop.id, EDIT_SIZE, EDIT_SIZE);

  const what = input.hint ? `a ${input.hint}` : "a watermark, logo or stamp";
  const prompt =
    `The center of this image contains ${what}. Remove it completely, including every character ` +
    `and any box or frame around it, and reconstruct the underlying surface texture so that no trace remains. ` +
    `Everything else in the image must stay exactly as it is: do not repaint, do not change the composition, ` +
    `colors, lighting or sharpness, and do not add any new elements or text. ` +
    `Output the same framing and the same size as the input.`;

  await uking.ui?.progress?.(25, "AI 补全中（可能要一两分钟）");
  const edited = await uking.ai.imageEdit({
    image: await uking.image.encode(sent.id, "png"),
    prompt,
    size: `${EDIT_SIZE}x${EDIT_SIZE}`,
    model: input.model,
  });

  await uking.ui?.progress?.(80, "回贴");
  const back = await uking.image.resize((await uking.image.decode(edited.image)).id, box.w, box.h);

  // 色彩守卫：比对环带均值。模型偶尔会整体调色，那样贴回去会有一圈色差。
  const feather = clamp(Math.round(box.pad * 0.6), 8, Math.max(8, box.pad - 4));
  const selInBox = { x: sel.x - box.x, y: sel.y - box.y, w: sel.w, h: sel.h };
  const grown = {
    x: selInBox.x - feather, y: selInBox.y - feather,
    w: selInBox.w + 2 * feather, h: selInBox.h + 2 * feather,
  };
  const full = { x: 0, y: 0, w: box.w, h: box.h };
  const a = await ringStats(uking, crop.id, full, grown);
  const b = await ringStats(uking, back.id, full, grown);
  const ringDelta = a.median.map((v, i) => v - b.median[i]);
  const dmax = Math.max(...ringDelta.map(Math.abs));

  const notes = [];
  if (dmax > 40) notes.push(`环带色差 ${dmax.toFixed(0)} 偏大，模型很可能重画了整幅，建议重试或缩小选区`);
  const offset = dmax > 4 && dmax <= 40 ? ringDelta : null;
  if (offset) notes.push("已做色彩补偿");

  const out = await uking.image.compositeFeather(src.id, back.id, { x: box.x, y: box.y }, sel, feather, offset);
  await uking.ui?.progress?.(100, "完成");

  // 交产物，不交像素。同一个动作会被终端里的 agent 调用，那边只能消化文本；
  // 往终端吐几 MB base64 既看不见图，又白白撑爆它的上下文。
  const artifact = await uking.artifact.emit({
    kind: "image",
    data: await uking.image.encode(out.id, "png"),
    action: "app.imagefix.watermark.remove",
    message: `已去除水印 · ${src.w}×${src.h}`,
  });

  return {
    ok: true,
    artifact,
    message: `已去除水印 · ${src.w}×${src.h}${notes.length ? " · " + notes.join("；") : ""}`,
    engine_used: "ai",
    context_box: { x: box.x, y: box.y, w: box.w, h: box.h },
    ring_delta: ringDelta.map((v) => Math.round(v * 10) / 10),
    notes: notes.join("；") || undefined,
  };
}

// ═══════════════════════════ 改文字 ═══════════════════════════
//
// 默认走本地，不是走 AI —— 这不是保守，是实测：扩散模型稳定糊中文字形。
// 蓝底白字的标牌、纯色背景的价签这类最常见的场景，本地重绘是像素级精确、免费、瞬时的。
// 只有背景确实有纹理时，平色填充才会露馅，那时候才值得花额度让 AI 去画。

const STDDEV_FLAT = 12;      // ≤ 这个值：平背景，本地重绘完美
const STDDEV_TEXTURED = 25;  // > 这个值：有纹理，平色填充会露馅

export async function textReplace(input, ctx) {
  const { uking } = ctx;
  const src = await uking.image.decode(input.image);
  const sel = normRegion(input.region, src.w, src.h);
  const newText = String(input.new_text ?? "");
  if (!newText) throw new Error("invalid_input: new_text 不能为空");

  // ── 测量：先拿证据，再选路 ──
  await uking.ui?.progress?.(10, "测量背景");
  const ringW = clamp(Math.round(Math.min(sel.w, sel.h) * 0.12), 2, 12);
  const probe = {
    x: clamp(sel.x - ringW, 0, src.w - 1), y: clamp(sel.y - ringW, 0, src.h - 1),
    w: 0, h: 0,
  };
  probe.w = clamp(sel.w + 2 * ringW, 1, src.w - probe.x);
  probe.h = clamp(sel.h + 2 * ringW, 1, src.h - probe.y);
  const inner = { x: sel.x - probe.x, y: sel.y - probe.y, w: sel.w, h: sel.h };
  const bg = await ringStats(uking, src.id, probe, inner);
  // 字形检测要逐像素看，但只在选区这一小块上做，量级完全可控
  const px = bytesOf(await uking.image.pixels(src.id, probe));

  // 字形色：选区内偏离背景中位数超过 3σ 的像素，取其中位色
  const thr = Math.max(24, bg.stddev * 3);
  const glyph = [[], [], []];
  let minX = 1e9, minY = 1e9, maxX = -1, maxY = -1;
  for (let y = inner.y; y < inner.y + inner.h; y++) {
    for (let x = inner.x; x < inner.x + inner.w; x++) {
      const i = (y * probe.w + x) * 4;
      const d = Math.max(
        Math.abs(px[i] - bg.median[0]), Math.abs(px[i + 1] - bg.median[1]), Math.abs(px[i + 2] - bg.median[2]));
      if (d <= thr) continue;
      glyph[0].push(px[i]); glyph[1].push(px[i + 1]); glyph[2].push(px[i + 2]);
      if (x < minX) minX = x; if (x > maxX) maxX = x;
      if (y < minY) minY = y; if (y > maxY) maxY = y;
    }
  }
  const med = (a) => { const s = [...a].sort((p, q) => p - q); return s.length ? s[s.length >> 1] : 0; };
  const textColor = glyph[0].length ? glyph.map(med) : [0, 0, 0];
  const coverage = glyph[0].length / (inner.w * inner.h);
  const glyphBox = maxX >= 0
    ? { x: probe.x + minX, y: probe.y + minY, w: maxX - minX + 1, h: maxY - minY + 1 }
    : { ...sel };

  const measured = {
    bg_median: bg.median,
    bg_stddev: Math.round(bg.stddev * 10) / 10,
    text_color: textColor,
    glyph_box: glyphBox,
    coverage: Math.round(coverage * 1000) / 1000,
  };

  // ── 选路 ──
  let engine = input.engine ?? "auto";
  if (engine === "auto") {
    const areaRatio = (sel.w * sel.h) / (src.w * src.h);
    if (coverage < 0.005) engine = "ai";                                   // 框里几乎没字，多半是图标/水印
    else if (bg.stddev <= STDDEV_FLAT && areaRatio < 0.08) engine = "local";
    else if (bg.stddev <= STDDEV_TEXTURED) engine = "local";
    else engine = "ai";
  }

  if (engine === "local") {
    await uking.ui?.progress?.(40, "本地重绘");
    const work = await uking.image.clone(src.id);
    await uking.image.fillRect(work.id, sel, measured.bg_median, {
      // 平背景往往还有一点拍摄颗粒。同方差噪声，免得那块看起来像贴纸。
      noise: bg.stddev > 3 ? bg.stddev : 0,
    });
    await uking.image.drawText(work.id, {
      text: newText,
      rect: sel,
      color: input.style?.color ?? rgbHex(textColor),
      font_family: input.style?.font_family,
      align: input.style?.align ?? "center",
      bold: input.style?.bold ?? false,
      // 字号按原字形高度定，而不是按选区高度 —— 选区是用户随手拖的，字形框才是真的
      fit_height: Math.min(glyphBox.h, sel.h),
    });
    await uking.ui?.progress?.(100, "完成");
    const artifact = await uking.artifact.emit({
      kind: "image",
      data: await uking.image.encode(work.id, "png"),
      action: "app.imagefix.text.replace",
      message: `已改为「${newText}」· 本地精确重绘 · 未消耗额度`,
    });
    return {
      ok: true, artifact, engine_used: "local", measured,
      message: `已改为「${newText}」· 本地精确重绘 · 未消耗额度`,
      notes: bg.stddev > STDDEV_FLAT ? "背景不是纯平色，若边缘有痕迹可改用 AI 重绘" : undefined,
    };
  }

  // ── AI 路：与去水印同一套裁剪回贴 ──
  const box = contextBox(sel, src.w, src.h);
  await uking.ui?.progress?.(25, "AI 重绘中（可能要一两分钟）");
  const crop = await uking.image.crop(src.id, box);
  const sent = await uking.image.resize(crop.id, EDIT_SIZE, EDIT_SIZE);
  const prompt =
    `The center of this image contains a short line of text. Replace that text with exactly: 「${newText}」. ` +
    `Keep the same font style, weight, size, color and baseline as the original text. ` +
    `Everything else in the image must remain exactly as it is. ` +
    `Output the same framing and the same size as the input.`;
  const edited = await uking.ai.imageEdit({
    image: await uking.image.encode(sent.id, "png"), prompt,
    size: `${EDIT_SIZE}x${EDIT_SIZE}`, model: input.model,
  });
  await uking.ui?.progress?.(80, "回贴");
  const back = await uking.image.resize((await uking.image.decode(edited.image)).id, box.w, box.h);
  const feather = clamp(Math.round(box.pad * 0.6), 8, Math.max(8, box.pad - 4));
  const out = await uking.image.compositeFeather(src.id, back.id, { x: box.x, y: box.y }, sel, feather, null);
  await uking.ui?.progress?.(100, "完成");
  const artifact = await uking.artifact.emit({
    kind: "image",
    data: await uking.image.encode(out.id, "png"),
    action: "app.imagefix.text.replace",
    message: `已改为「${newText}」· AI 重绘`,
  });
  return {
    ok: true, artifact, engine_used: "ai", measured,
    message: `已改为「${newText}」· AI 重绘`,
    notes: "AI 重绘中文长句可能出现错字，请核对；不满意可改用本地重绘",
  };
}

const rgbHex = ([r, g, b]) =>
  "#" + [r, g, b].map((v) => clamp(Math.round(v), 0, 255).toString(16).padStart(2, "0")).join("");

export default {
  "app.imagefix.watermark.remove": watermarkRemove,
  "app.imagefix.text.replace": textReplace,
};
