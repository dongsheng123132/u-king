/**
 * cc-switch 式「添加 / 编辑自定义供应商」弹窗 —— 全 app 唯一一份 provider 编辑表单。
 *
 * 2026-09-06 从 Manager.tsx 原地搬出（纯搬家，零行为变化）+ 合并 ProviderManager 的
 * 盲存表单（补 codex_model 字段、放宽校验），从此全 app 只剩这一份实现。
 */
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  CheckCircle2,
  Loader2,
  Plus,
  RefreshCw,
  Trash2,
  X,
  XCircle,
  Zap,
} from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "../lib/cn";
import { useI18n } from "../i18n";
import { ToolIcon } from "./ToolIcon";
import type { ProviderPreset } from "../Wizard";
import { PROVIDER_TEMPLATES, type ProviderTemplate } from "../lib/providerTemplates";
import type { FreeGuide } from "../lib/freeGuide";

/** 自定义供应商表单的输入框统一样式（单一定义，宪法第 8 条；Manager 从这里 import）。
 *  🔴 定义放本文件而不是 Manager：Manager 已 import 本文件，若这两个常量留在 Manager
 *  就形成双向循环 import——tsc/构建能过，但谁在模块顶层求值对方的导出就会踩加载时序坑。 */
export const IPT =
  "w-full h-9 rounded-lg border border-white/[0.10] bg-bg-1 px-3 text-[12px] text-ink-1 outline-none focus:border-accent/50 placeholder:text-ink-4";

export const TOOL_LABELS: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex",
  clawx: "ClawX / OpenClaw",
  hermes: "Hermes",
  dsh: "DeepSeek Harness",
  pi: "pi",
  opencode: "OpenCode",
  cline: "Cline",
};

/** 试连 / 拉模型清单结果（Manager 也在用，单一定义搬到这里，Manager 改 import）。 */
export type TestResult = { ok: boolean; api: string; latency_ms: number; reply: string | null; error: string | null };

/** 免费路线正在接入的上下文。Key 只在 `editing` 的本机表单状态里，绝不进官网或 Registry。 */
export type FreeRouteContext = {
  entry: FreeGuide["entries"][number];
  target: string;
  stage: "draft" | "added";
  savedId?: string;
};

/** 表单字段包裹（标签 + 说明 + 输入区）。 */
function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="block">
      <div className="text-[12px] font-medium text-ink-0 mb-2">{label}</div>
      {children}
      {hint && <div className="mt-1.5 text-[10.5px] text-ink-4 leading-snug">{hint}</div>}
    </label>
  );
}

/**
 * cc-switch 式「添加 / 编辑自定义供应商」弹窗。
 * 字段对齐 cc-switch 的自定义表单：名称 + 接口地址(base) + 模型 + API Key。
 * id 为空 = 新增（后端按 name 生成）；非空 = 编辑既有自定义项。
 */
export function CustomProviderModal({
  value,
  onChange,
  onSave,
  onClose,
  addable = [],
  templates = PROVIDER_TEMPLATES,
  addingTo,
  onAddBuiltin,
  onPurge,
  variant = "modal",
  freeRoute,
  onFreeTargetChange,
  onFreeRouteDirty,
  onEnableFreeRoute,
  enablingFreeRoute = false,
}: {
  value: ProviderPreset;
  onChange: (p: ProviderPreset) => void;
  onSave: (p: ProviderPreset) => void;
  onClose: () => void;
  /** 当前这个 AI 的列表里没有、可一键加回的供应商（内置 + 被移出这个 AI 的自定义）。 */
  addable?: ProviderPreset[];
  /** 预设模板清单——调用方传的是「远程覆盖 ?? 静态兜底」（见 Manager 里的 `templates`）；
   *  不传就退回静态导入，保证这个组件单独测试/复用时不需要额外接线。 */
  templates?: ProviderTemplate[];
  /** 加到哪个 AI 的列表里（列表是 per-tool 的，得说清楚加的是谁的）。 */
  addingTo?: string;
  onAddBuiltin?: (p: ProviderPreset) => void;
  /** 彻底删除（全部 AI + 定义 + Key）。只在编辑既有自定义供应商时给。 */
  onPurge?: (p: ProviderPreset) => void;
  variant?: "modal" | "drawer";
  freeRoute?: FreeRouteContext | null;
  onFreeTargetChange?: (target: string) => void;
  /** 免费路线保存后又改了表单：旧 provider 不能再被直接启用，必须重新保存/试连。 */
  onFreeRouteDirty?: () => void;
  onEnableFreeRoute?: () => void;
  enablingFreeRoute?: boolean;
}) {
  const { t } = useI18n();
  const isEdit = !!value.id;
  /**
   * 存前试连 + 存前拉模型（2026-08-22，用户亲历「无法准确添加新的供应商」后重做）。
   *
   * 🔴 原来的流程是**盲存**：填完只能保存，Key 抄错一位 / base 少个 /v1 / 模型 id 打错，
   * 第一条报错出现在列表深处甚至切驱动失败时 —— 离「你填错的那一格」隔着三层。
   * 现在错误死在弹窗里：拉得到模型清单只证明端点可达；试连回话才证明 Key 和模型都对了。
   * 某些上游（如 OpenRouter）允许匿名读取 /models，不能把模型清单当作 Key 校验。
   */
  const [probing, setProbing] = useState(false);
  const [probe, setProbe] = useState<TestResult | null>(null);
  const [fetchingList, setFetchingList] = useState(false);
  const [modelList, setModelList] = useState<string[]>([]);
  const [listErr, setListErr] = useState<string | null>(null);
  // 任何一格变了，上一次的试连结果就不再作数 —— 留着一个绿勾伴着改坏的表单，比没有更糟。
  const set = (patch: Partial<ProviderPreset>) => {
    setProbe(null);
    if (freeRoute?.stage === "added") onFreeRouteDirty?.();
    onChange({ ...value, ...patch });
  };

  const fetchModels = async () => {
    if (fetchingList) return;
    setFetchingList(true);
    setListErr(null);
    try {
      const ids = await invoke<string[]>("list_models_at_endpoint", {
        baseUrl: value.openai_base.trim(),
        apiKey: value.api_key ?? "",
      });
      setModelList(ids);
      // 模型还空着就替他填上第一个 —— 拉都拉到了，别让人再抄一遍
      if (!value.model.trim() && ids.length) set({ model: ids[0] });
    } catch (e) {
      setModelList([]);
      setListErr(String(e));
    } finally {
      setFetchingList(false);
    }
  };

  const runProbe = async () => {
    if (probing) return;
    setProbing(true);
    setProbe(null);
    const r = await invoke<TestResult>("probe_endpoint", {
      baseUrl: value.openai_base.trim(),
      apiKey: value.api_key ?? "",
      model: value.model.trim(),
    }).catch((e) => ({ ok: false, api: "openai", latency_ms: 0, reply: null, error: String(e) }) as TestResult);
    setProbe(r);
    setProbing(false);
  };

  /** 点预设模板 = 把 baseUrl/官网/Key 提示/默认模型一次填好（只补 Key）。null = 自定义清空。 */
  const applyTemplate = (tpl: ProviderTemplate | null) => {
    if (!tpl) {
      set({ name: "", openai_base: "", anthropic_base: null, model: "", small_model: "", key_url: "", key_hint: "API Key" });
      return;
    }
    set({
      name: tpl.name,
      openai_base: tpl.openai_base,
      anthropic_base: tpl.anthropic_base ?? null,
      model: tpl.model ?? "",
      small_model: tpl.small_model ?? "",
      key_url: tpl.key_url ?? "",
      key_hint: tpl.key_hint ?? "API Key",
    });
  };
  /** 当前表单的 baseUrl 命中哪个模板（高亮用）；都不命中 = 自定义。 */
  const activeTpl = templates.find((tpl) => tpl.openai_base === value.openai_base.trim());

  // 🔴 2026-09-06 合并 ProviderManager 表单时放宽：原先必须有 openai_base，把「纯 Anthropic
  // 中转站」（B 表单原本允许的场景）挡在门外。现在两个端点填一个即可；试连/拉模型清单两个
  // 探测本身只打 openai 端点，openai_base 为空时照旧禁用（见下方按钮 disabled/title）。
  const canSave =
    value.name.trim().length > 0 &&
    (value.openai_base.trim().length > 0 || !!value.anthropic_base?.trim()) &&
    value.api_key !== undefined;

  const submit = () => {
    if (!canSave) return;
    // 新增：id 留空**交给后端生成**；编辑：保持原 id。
    // 🔴 这里以前自己算一份 slug（`name.replace(/[^a-z0-9]+/g,"-")`），中文名整串被替换成 "-"
    // → id 恒为 `custom--`，两个中文名供应商撞同一个 id、后加的静默覆盖先加的（issue #359
    // 客户机上就是 `custom--`）。判据只留后端一份（宪法第 8 条）。
    onSave({
      ...value,
      id: value.id,
      builtin: false,
      builtin_recharge: false,
      // anthropic_base 留空表示纯 OpenAI 兼容；填了则 Claude Code 走 Anthropic 格式
      anthropic_base: value.anthropic_base?.trim() ? value.anthropic_base.trim() : null,
      small_model: value.small_model?.trim() || value.model.trim(),
      codex_model: value.codex_model?.trim() || undefined,
    });
  };

  const isDrawer = variant === "drawer";
  const noOpenaiBase = !value.openai_base.trim();
  return (
    <div
      className={cn("fixed inset-0 z-[60]", isDrawer ? "pointer-events-none" : "grid place-items-center bg-black/60 backdrop-blur-sm p-4")}
      onClick={isDrawer ? undefined : onClose}
    >
      <div
        className={cn(
          "border border-white/[0.10] bg-bg-1 shadow-card",
          isDrawer ? "pointer-events-auto absolute right-0 top-0 h-full w-full max-w-[480px] rounded-l-card flex flex-col" : "w-full max-w-[440px] rounded-card",
        )}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-5 h-13 py-3.5 border-b border-white/[0.08] bg-bg-1/60">
          <div className="text-[14px] font-semibold text-ink-0">
            {isDrawer ? t("正在接入：{name}", { name: freeRoute?.entry.name ?? value.name }) : isEdit ? t("编辑供应商") : t("添加供应商")}
          </div>
          <button
            onClick={onClose}
            className="grid place-items-center w-7 h-7 rounded-md text-ink-3 hover:text-ink-1 hover:bg-white/[0.06]"
          >
            <X size={15} />
          </button>
        </div>

        {isDrawer && freeRoute && (
          <div className="px-5 py-3 border-b border-white/[0.08] bg-accent/[0.045] text-[11px] leading-relaxed text-ink-3">
            <div className="flex items-center gap-2 text-ink-1 font-medium">
              <span className="rounded-full bg-emerald-500/15 px-2 py-0.5 text-emerald-500">{t("免费档")}</span>
              <span>{freeRoute.entry.region ?? t("第三方")}</span>
              <select value={freeRoute.target} onChange={(e) => onFreeTargetChange?.(e.target.value)} className="ml-auto rounded border border-white/[0.12] bg-bg-2 px-1.5 py-1 text-ink-1">
                {(freeRoute.entry.targets ?? ["pi"]).map((target) => <option key={target} value={target}>{TOOL_LABELS[target] ?? target}</option>)}
              </select>
            </div>
            <div className="mt-1">{freeRoute.stage === "added" ? t("已添加：Key 和供应商已保存到本机，尚未启用给任何 AI。") : t("默认：仅此第三方来源；不使用虾盘钱包，不扣费。")}</div>
          </div>
        )}

        <div className={cn("px-5 py-4 space-y-3 overflow-y-auto", isDrawer ? "flex-1" : "max-h-[70vh]")}>
          {/* ★「U-King 内置 · 一键添加」——「添加」是用户主动伸手的时刻，摆在这里才不算抢。
              主列表默认只留虾盘云 + 官方直连，其余内置（DeepSeek/GLM/Kimi/Ollama）都在这一排；
              虾盘云被移除后，这里也是它**唯一**的常规回归入口（另一条是列表底部「已移除」那行，
              只在亲手删过之后才出现）。点一下即成 —— 内置的端点/Key 我们已经配好，不用填表。 */}
          {!isEdit && !isDrawer && addable.length > 0 && (
            <div>
              <div className="text-[12px] font-medium text-ink-1 mb-1.5 flex items-center gap-1.5">
                <Zap size={12} className="text-accent" />
                {addingTo ? t("一键加进 {tool} 的列表", { tool: addingTo }) : t("U-King 内置 · 一键添加")}
                <span className="text-[10.5px] font-normal text-ink-4">{t("免填表")}</span>
              </div>
              <div className="grid grid-cols-2 gap-1.5">
                {addable.map((p) => (
                  <button
                    key={p.id}
                    onClick={() => onAddBuiltin?.(p)}
                    title={p.summary}
                    className="flex items-center gap-2 px-2.5 h-11 rounded-lg border border-white/[0.10] bg-bg-2/60 text-left hover:border-accent/40 hover:bg-accent/[0.06] transition-colors"
                  >
                    <ToolIcon
                      tool={p.builtin_recharge ? "deepseek" : p.id}
                      size={17}
                      active
                      className="shrink-0 opacity-90"
                    />
                    <div className="min-w-0 flex-1">
                      <div className="text-[11.5px] font-medium text-ink-1 truncate">{p.name}</div>
                      <div className="text-[10px] text-ink-4 truncate">
                        {p.builtin_recharge
                          ? t("内置 Key，免注册")
                          : p.key_hint || t("需自备 API Key")}
                      </div>
                    </div>
                    <Plus size={13} className="shrink-0 text-ink-4" />
                  </button>
                ))}
              </div>
              <div className="mt-2 border-t border-white/[0.06] pt-2.5 text-[10.5px] text-ink-4">
                {t("下面是自己填 —— 任何 OpenAI 兼容的中转 / 官方接口都能加。")}
              </div>
            </div>
          )}

          {/* 预设模板库（小型精选，对齐 cc-switch）：点一个自动填好地址，只需补 Key。仅新增时显示。 */}
          {!isEdit && !isDrawer && (
            <div>
              <div className="text-[12px] font-medium text-ink-1 mb-1.5">{t("预设供应商")}</div>
              <div className="flex flex-wrap gap-1.5">
                <button
                  onClick={() => applyTemplate(null)}
                  className={cn(
                    "px-2.5 h-7 rounded-md text-[11.5px] font-medium border transition-colors",
                    !activeTpl
                      ? "bg-accent text-white border-accent"
                      : "border-white/[0.10] text-ink-2 hover:bg-white/[0.04]",
                  )}
                >
                  {t("自定义")}
                </button>
                {templates.map((tpl) => {
                  const on = activeTpl?.name === tpl.name;
                  return (
                    <button
                      key={tpl.name}
                      onClick={() => applyTemplate(tpl)}
                      title={tpl.openai_base}
                      className={cn(
                        "px-2.5 h-7 rounded-md text-[11.5px] font-medium border transition-colors",
                        on
                          ? "bg-accent text-white border-accent"
                          : "border-white/[0.10] text-ink-2 hover:bg-white/[0.04]",
                      )}
                    >
                      {tpl.name}
                    </button>
                  );
                })}
              </div>
              <div className="mt-1.5 text-[10.5px] text-ink-4 leading-snug">
                {t("💡 点一个自动填好接口地址，下方只需补 API Key；选「自定义」则全部手填。存好后可在列表里「🔄 拉取」选具体模型。")}
              </div>
            </div>
          )}

          <Field label={t("名称")} hint={t("给这个供应商起个名字，如「我的中转」")}>
            <input
              value={value.name}
              onChange={(e) => set({ name: e.target.value })}
              placeholder={t("我的供应商")}
              className={IPT}
            />
          </Field>

          <Field label={t("接口地址 (Base URL)")} hint={t("OpenAI 兼容接口，一般以 /v1 结尾")}>
            <input
              value={value.openai_base}
              onChange={(e) => set({ openai_base: e.target.value })}
              placeholder="https://api.example.com/v1"
              className={cn(IPT, "font-mono")}
            />
          </Field>

          <Field label="API Key">
            <input
              value={value.api_key ?? ""}
              onChange={(e) => set({ api_key: e.target.value })}
              placeholder="sk-..."
              className={cn(IPT, "font-mono")}
            />
          </Field>

          <Field label={t("模型")} hint={t("填好地址和 Key 后点「拉取」，从这家真实有的模型里选 —— 不用去官网抄")}>
            <div className="flex items-center gap-1.5">
              <input
                value={value.model}
                onChange={(e) => set({ model: e.target.value })}
                placeholder="gpt-4o / deepseek-v4-flash ..."
                list="add-provider-models"
                className={cn(IPT, "font-mono flex-1 min-w-0")}
              />
              <button
                onClick={fetchModels}
                disabled={fetchingList || noOpenaiBase}
                title={noOpenaiBase ? t("需要先填 OpenAI 兼容端点") : t("从接口拉取真实模型清单；部分供应商允许匿名读取，Key 请用「测试连通」验证")}
                className="shrink-0 inline-flex items-center gap-1 h-9 px-2.5 rounded-lg border border-white/[0.10] text-ink-2 text-[11.5px] hover:bg-white/[0.04] disabled:opacity-40 transition-colors"
              >
                {fetchingList ? <Loader2 size={12} className="animate-spin" /> : <RefreshCw size={12} />}
                {t("拉取")}
              </button>
            </div>
            <datalist id="add-provider-models">
              {modelList.map((m) => (
                <option key={m} value={m} />
              ))}
            </datalist>
            {modelList.length > 0 && (
              <p className="mt-1 text-[10.5px] text-success-400">
                {t("✓ 拉到 {n} 个模型 —— 接口地址可达；Key 请点「测试连通」确认。点输入框从清单里选", { n: modelList.length })}
              </p>
            )}
            {listErr && (
              <p className="mt-1 text-[10.5px] leading-snug text-danger-400 break-all">{listErr}</p>
            )}
          </Field>

          <details className="group">
            <summary className="cursor-pointer text-[11.5px] text-ink-3 hover:text-ink-1 list-none select-none">
              {t("＋ 高级（小模型 / Claude 格式地址，可不填）")}
            </summary>
            <div className="mt-2.5 space-y-3">
              <Field label="Small Model" hint={t("省 token 的轻量模型，留空 = 同上")}>
                <input
                  value={value.small_model}
                  onChange={(e) => set({ small_model: e.target.value })}
                  placeholder={t("留空则用上面的模型")}
                  className={cn(IPT, "font-mono")}
                />
              </Field>
              <Field label={t("Codex 模型（可空）")} hint={t("Codex 固定使用新版 Responses 协议；保存前请确认供应商支持 /responses。")}>
                <input
                  value={value.codex_model ?? ""}
                  onChange={(e) => set({ codex_model: e.target.value })}
                  placeholder={t("沿用默认模型")}
                  className={cn(IPT, "font-mono")}
                />
              </Field>
              <Field label="Anthropic Base URL" hint={t("给 Claude Code 用的 Anthropic 格式地址；纯 OpenAI 接口留空")}>
                <input
                  value={value.anthropic_base ?? ""}
                  onChange={(e) => set({ anthropic_base: e.target.value })}
                  placeholder={t("留空 = 仅 OpenAI 兼容")}
                  className={cn(IPT, "font-mono")}
                />
              </Field>
            </div>
          </details>
        </div>

        {/* 试连结果 —— 紧贴按钮区，成败都说人话。绿 = 这套填法真能回话；红 = 原样透出上游报错，
            此刻表单还开着，改完再试，不用保存-失败-再回来。 */}
        {probe && (
          <div
            className={cn(
              "mx-5 mb-2.5 rounded-lg px-3 py-2 text-[11px] leading-snug flex items-start gap-2 border",
              probe.ok
                ? "bg-success-500/[0.08] text-success-400 border-success-500/20"
                : "bg-danger-500/[0.08] text-danger-400 border-danger-500/20",
            )}
          >
            {probe.ok ? <CheckCircle2 size={13} className="shrink-0 mt-px" /> : <XCircle size={13} className="shrink-0 mt-px" />}
            <span className="min-w-0 break-all">
              {probe.ok ? t("「{reply}」· {ms}ms · 可以保存了", { reply: probe.reply ?? "", ms: probe.latency_ms }) : probe.error}
            </span>
          </div>
        )}
        <div className="flex items-center justify-end gap-2 px-5 py-3.5 border-t border-white/[0.08]">
          {/* 「彻底删除」只在编辑既有自定义供应商时出现 —— 行里那个垃圾桶是高频的
              「这个 AI 不用它」（只移出当前列表），真要连定义带 Key 一起毁得进来这里点，
              免得顺手把别的 AI 还在用的东西也删了。 */}
          {isEdit && !value.builtin && onPurge && (
            <button
              data-action-id="runtime.provider.delete"
              onClick={() => onPurge(value)}
              title={t("从全部 AI 的列表里删掉，并销毁它的地址和已保存的 Key")}
              className="mr-auto inline-flex items-center gap-1.5 h-9 px-3 rounded-lg border border-danger-500/25 text-danger-400 text-[12px] font-medium hover:bg-danger-500/10 transition-colors"
            >
              <Trash2 size={13} /> {t("彻底删除")}
            </button>
          )}
          <button
            onClick={runProbe}
            disabled={probing || noOpenaiBase}
            title={noOpenaiBase ? t("需要先填 OpenAI 兼容端点") : t("用当前填的地址 / Key / 模型真发一条消息 —— 通了再保存，错了当场看到原因")}
            className="inline-flex items-center gap-1.5 h-9 px-3.5 rounded-lg border border-accent/40 text-accent text-[12px] font-medium hover:bg-accent/[0.08] disabled:opacity-40 transition-colors"
          >
            {probing ? <Loader2 size={13} className="animate-spin" /> : <Zap size={13} />}
            {t("测试连通")}
          </button>
          <button
            onClick={onClose}
            className="h-9 px-4 rounded-lg border border-white/[0.10] text-ink-2 text-[12px] font-medium hover:bg-white/[0.04] transition-colors"
          >
            {t("取消")}
          </button>
          {freeRoute?.stage === "added" ? (
            <button onClick={onEnableFreeRoute} disabled={enablingFreeRoute} className="h-9 px-5 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-40 shadow-sm transition-colors">
              {enablingFreeRoute ? t("验证并启用中…") : t("启用到 {tool}", { tool: TOOL_LABELS[freeRoute.target] ?? freeRoute.target })}
            </button>
          ) : (
            <button
              data-action-id="runtime.provider.save"
              onClick={submit}
              disabled={!canSave}
              className="h-9 px-5 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-40 shadow-sm transition-colors"
            >
              {isDrawer ? t("保存 Key 和供应商") : t("保存")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
