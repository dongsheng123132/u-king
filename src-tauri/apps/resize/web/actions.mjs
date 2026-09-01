// 改尺寸 —— 动作实现。
//
// 教程成品。整个文件只有一个函数，但它把这套架构的四件事都用上了：
//   ① 不碰 DOM（所以 GUI 和无头 CLI 能 import 同一份）
//   ② 图像运算走宿主原语（Node 里没有 canvas）
//   ③ 成品经 artifact.emit 交给宿主，返回引用而不是像素
//   ④ 入参校验交给宿主按 input_schema 做，这里只管业务判断

/**
 * @param {{image:string, max_w:number, max_h:number, allow_upscale?:boolean}} input
 * @param {{uking:object}} ctx
 */
export async function imageFit(input, ctx) {
  const { uking } = ctx;
  const src = await uking.image.decode(input.image);

  // 等比缩放因子。取两个方向里更紧的那个，才不会有一边溢出。
  let k = Math.min(input.max_w / src.w, input.max_h / src.h);

  // 默认不放大：把一张小图拉大只会得到一个更大、更糊的文件。
  // 用户真要放大就显式说，别替他做这个决定。
  if (k > 1 && !input.allow_upscale) k = 1;

  const w = Math.max(1, Math.round(src.w * k));
  const h = Math.max(1, Math.round(src.h * k));

  const out = w === src.w && h === src.h ? src : await uking.image.resize(src.id, w, h);
  const png = await uking.image.encode(out.id, "png");

  const message =
    k === 1 && w === src.w
      ? `原图 ${src.w}×${src.h} 已经在范围内，未缩放`
      : `${src.w}×${src.h} → ${w}×${h}`;

  const artifact = await uking.artifact.emit({
    kind: "image",
    data: png,
    action: "app.resize.image.fit",
    message,
  });

  return {
    ok: true,
    artifact,
    message,
    from: { w: src.w, h: src.h },
    to: { w, h },
  };
}

// 动作 id → 实现。key 必须与 action-parity.json 里的 actions[].id 逐字一致，
// 宿主安装时会校验，对不上装不进去。
export default {
  "app.resize.image.fit": imageFit,
};
