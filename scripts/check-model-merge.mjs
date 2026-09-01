/**
 * 跑道：`mergeModels` 的三种情形 + 一条「拉不到时不许显示空」。
 *
 * 为什么值得单开：这个函数决定**客户在「换模型」下拉里看得见什么**。
 * 它错的两个方向代价不对称 ——
 *   · 少显示 → 客户用不了已经充过钱的模型（2026-08-20 实测 13 个就是这么消失的）
 *   · 显示了服务端没有的 → 客户点下去 503，以为自己钱没到账
 * 而这两种都**只看界面查不出来**（下拉里少一项，谁也不会发现）。
 *
 * 用法：node scripts/check-model-merge.mjs
 */
import { transformWithEsbuild } from "vite";
import { readFileSync, writeFileSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
// 吃**真的**源码，不在这里抄一份实现（抄一份的话补丁被删了跑道照样绿）
const js = (
  await transformWithEsbuild(readFileSync(join(root, "src/lib/models.ts"), "utf8"), "models.ts", {
    loader: "ts",
    format: "esm",
  })
).code;
const dir = mkdtempSync(join(tmpdir(), "uking-models-"));
const file = join(dir, "models.mjs");
writeFileSync(file, js, "utf8");
const { mergeModels, XIAPAN_MODELS } = await import(pathToFileURL(file).href);

const ids = (gs) => gs.flatMap((g) => g.items.map((m) => m.id));
const fails = [];
const check = (name, cond, detail = "") => {
  if (cond) console.log(`  ✅ ${name}`);
  else {
    console.log(`  ❌ ${name}${detail ? "  —— " + detail : ""}`);
    fails.push(name);
  }
};

const catalogIds = ids(XIAPAN_MODELS);
const sample = catalogIds[0];
console.log(`本地清单收录 ${catalogIds.length} 个 id，拿 "${sample}" 当样本\n`);

// ① 拉不到 → 原样退回清单，绝不显示空
for (const empty of [null, undefined, []]) {
  const got = ids(mergeModels(empty));
  check(
    `拉不到（${JSON.stringify(empty)}）→ 退回完整清单，不显示空`,
    got.length === catalogIds.length,
    `实得 ${got.length} 个，应为 ${catalogIds.length}`,
  );
}

// ② 服务端有、清单没有 → 必须出现（这就是那 13 个消失的模型）
const brandNew = "deepseek-v5-imaginary";
const withNew = ids(mergeModels([sample, brandNew]));
check("服务端新上的模型（清单没收）→ 照样能选", withNew.includes(brandNew), `实得 ${JSON.stringify(withNew)}`);

// ③ 清单有、服务端没有 → 必须消失（别让客户点到 503）
const gone = catalogIds.find((x) => x !== sample);
const withoutGone = ids(mergeModels([sample]));
check("清单有、服务端下架了 → 不显示", !withoutGone.includes(gone), `${gone} 不该还在`);

// ④ 非对话类（作图/视频/语音/OCR）不该**自动**混进「换模型」下拉。
//    🔴 只对「清单没收的」生效 —— 清单里**明确收录过**的照旧显示。
//    第一版把 `qwen3-vl-plus` 也当噪声，红了；查下来它就在清单的视觉组里（给「发图识别」用的），
//    是**我们自己有意放进去的**。★ 是测试的期望错了，不是代码错了 ——
//    过滤器的职责是「别把没人审过的东西自动塞进来」，不是「否决我们审过的决定」。
const noise = ["qwen-image-2.0", "wanx2.1-t2v-turbo", "gpt-4o-mini-tts", "qwen3.5-ocr"];
const withNoise = ids(mergeModels([sample, ...noise]));
check("清单没收的作图/视频/语音/OCR 不自动混进对话下拉", noise.every((n) => !withNoise.includes(n)));
const curatedVision = catalogIds.find((x) => /-vl-/.test(x));
if (curatedVision) {
  const keep = ids(mergeModels([sample, curatedVision]));
  check(`清单**有意收录**的视觉模型（${curatedVision}）不被噪声过滤器误杀`, keep.includes(curatedVision));
}

// ⑤ 已收录的保留人话说明（合并不能把 label/desc 弄丢）
const merged = mergeModels([sample]);
const item = merged.flatMap((g) => g.items).find((m) => m.id === sample);
check("已收录的模型仍带 label（说明没被合丢）", !!item && item.label !== item.id, `label=${item?.label}`);

if (fails.length) {
  console.error(`\n❌ ${fails.length} 条不过：${fails.join("；")}`);
  process.exit(1);
}
console.log("\n✅ 全过。");
