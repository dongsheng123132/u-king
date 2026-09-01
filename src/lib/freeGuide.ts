/**
 * 「免费额度怎么领」教程页的数据（AI 设置 · 免费额度分区）。
 *
 * **为什么是一个教程页，而不是一个内置供应商**：免费羊毛的寿命以周计 —— stealth 预览模型
 * 随时撤、免费额度随时收、注册门槛随时加。2026-08-24 我们试过把某一家（OpenCode Zen 的
 * Ox Alpha）写成 Rust 内置预设 + 前端硬编码角标，当天就退了：那样每换一家就要改代码、
 * 发一次版。用户原话「不要每次都改模型供应商，以不变应万变」。
 *
 * 所以这里只有两样东西，都不绑死任何一家：
 *  1. **每条只是一段说明 + 一个模板名**。点「一键导入」= 按 `template` 去模板画廊里找同名那条，
 *     带着 baseUrl / 模型 id 预填开「添加供应商」弹窗 —— 复用的是现成机制（`openAddTemplate`），
 *     这个文件不认识任何端点，也不该认识。
 *  2. **Key 一律客户自己领**。我们不替客户带 Key、不带免 Key 的魔法值、不做代申请。
 *     「没有 Key」不是我们的故障，教程的职责到「告诉他去哪领、怎么填」为止。
 *
 * 🔴 **本文件是断网兜底，不是真相源**。真相源是 skill 清单里的 `free_guide` 字段
 * （`installer.rs::RemoteFreeGuide`，同 `provider_templates` 那条热下发通道）——
 * 某家下线了、条件变了，改线上 JSON 即可，不发版。这里的内容会过期，UI 上要明说。
 *
 * 加/改条目时的规矩（跟 `providerTemplates.ts` 同一套）：
 *  - `template` 必须**逐字**等于模板画廊里的 `name`，否则「一键导入」按钮会自己隐藏
 *    （不是报错——找不到就不给点，别让用户点了没反应）；
 *  - `note` 只写**当天实际核过**的条件（免不免信用卡、限不限量、要不要手机号）。
 *    抄官网营销话术会让客户按着教程走到一半撞墙，比没有教程更坏。
 */

import registry from "../../website/free-registry.json";

export type FreeGuideEntry = {
  id?: string;
  /** 展示名，一般跟模板同名 */
  name: string;
  /** 模板画廊里的 `name`，用于「一键导入」；留空 = 只给说明不给按钮 */
  template?: string;
  /** 一句话：这家免费的是什么 */
  summary: string;
  /** 领 Key / 注册的地址（模板里也有一份，这里冗余是为了教程能独立读）。
   *  官方注册流程支持推荐码时，直接把推荐参数放在这里；按钮仍然只有一个「去领 Key」。 */
  key_url?: string;
  /** ⚠️ 客户会撞墙的条件：要不要卡、限不限量、有没有地域/手机号门槛 */
  note?: string;
  region?: string;
  targets?: string[];
  review?: "reviewed" | "pending_review" | "expired";
};

export type FreeGuide = {
  /** 单调递增。线上比本地大 = 有更新，用来提示客户 */
  version: number;
  /** 内容最后核实的日期，直接显示给客户看 —— 免费条件会烂，让他知道这份有多旧 */
  checked: string;
  entries: FreeGuideEntry[];
};

/** 与官网同源的内嵌断网兜底。线上版本会在进入页面时覆盖它。 */
export const FREE_GUIDE: FreeGuide = registry as FreeGuide;
