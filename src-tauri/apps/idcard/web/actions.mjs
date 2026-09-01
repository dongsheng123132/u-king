// 抠正 —— 把照片里歪着的卡片/证件/牌子抠出来摆正。
//
// 为什么四角是用户拖的，不是自动检测：
// 角点检测（边缘 → 直线 → 交点）在干净扫描件上很准，在真实照片里会被
// 反光、阴影、背景直线（桌沿、瓷砖缝）带跑，而且失败是静默的 —— 你拿到
// 一张歪得莫名其妙的图，不知道是它没检测到还是检测错了。
// 让用户拖四个点：三秒钟的事，100% 可控，失败不可能发生。
// 自动检测可以以后作为「猜一下」的辅助加上去，但不该是唯一路径。

const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));

/** 证件长宽比。ID-1 是身份证/银行卡的国际标准。 */
const RATIOS = {
  "id-card": 85.6 / 54,   // 第二代身份证、银行卡
  a4: 210 / 297,
  square: 1,
  auto: null,             // 按拖出来的四边形自己算
};

/** 两点距离 */
const dist = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1]);

/**
 * 从四边形推断输出尺寸。
 * 取对边的较长者当基准 —— 透视让近端比远端长，取长的那条才不会把细节压掉。
 */
function outSize(q, ratio, maxSide) {
  const wTop = dist(q[0], q[1]), wBottom = dist(q[3], q[2]);
  const hLeft = dist(q[0], q[3]), hRight = dist(q[1], q[2]);
  let w = Math.max(wTop, wBottom);
  let h = Math.max(hLeft, hRight);
  if (ratio) {
    // 有目标比例时，以较长边为准反推另一边，避免把证件拉扁
    if (w / h > ratio) h = w / ratio;
    else w = h * ratio;
  }
  const k = Math.min(1, maxSide / Math.max(w, h));
  return { w: Math.max(1, Math.round(w * k)), h: Math.max(1, Math.round(h * k)) };
}

/**
 * 把四个点排成 左上/右上/右下/左下。
 * 用户是随手点的，顺序不能指望。按「和 + 差」排序是这个问题的经典解法：
 * x+y 最小的是左上、最大的是右下；x−y 最小的是左下、最大的是右上。
 */
function orderQuad(pts) {
  const sum = pts.map((p) => p[0] + p[1]);
  const diff = pts.map((p) => p[0] - p[1]);
  const at = (arr, pick) => pts[arr.indexOf(pick(...arr))];
  return [
    at(sum, Math.min),   // 左上
    at(diff, Math.max),  // 右上
    at(sum, Math.max),   // 右下
    at(diff, Math.min),  // 左下
  ];
}

export async function cardExtract(input, ctx) {
  const { uking } = ctx;
  const src = await uking.image.decode(input.image);

  const raw = input.corners.map((p) => [
    clamp(Number(Array.isArray(p) ? p[0] : p.x), 0, src.w),
    clamp(Number(Array.isArray(p) ? p[1] : p.y), 0, src.h),
  ]);
  const q = input.ordered ? raw : orderQuad(raw);

  // 退化四边形（四点几乎共线或重合）直接拒，好过输出一张乱码
  const area = Math.abs(
    (q[1][0] - q[0][0]) * (q[3][1] - q[0][1]) - (q[3][0] - q[0][0]) * (q[1][1] - q[0][1])
  );
  if (area < 100) throw new Error("invalid_input: 这四个点圈不出一个有效的四边形，重新拖一下");

  const ratio = RATIOS[input.preset ?? "auto"] ?? null;
  const { w, h } = outSize(q, ratio, input.max_side ?? 2400);

  await uking.ui?.progress?.(40, "透视校正");
  const out = await uking.image.warpPerspective(src.id, q, w, h);

  await uking.ui?.progress?.(90, "输出");
  const message = `已抠正 · ${w}×${h}${ratio ? ` · 按${input.preset}比例` : ""}`;
  const artifact = await uking.artifact.emit({
    kind: "image",
    data: await uking.image.encode(out.id, "png"),
    action: "app.idcard.card.extract",
    message,
  });
  await uking.ui?.progress?.(100, "完成");

  return { ok: true, artifact, message, corners_used: q, to: { w, h } };
}

export default { "app.idcard.card.extract": cardExtract };
