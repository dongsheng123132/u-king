/**
 * cc-switch 式「添加 / 编辑自定义供应商」弹窗 —— 全 app 唯一一份 provider 编辑表单。
 *
 * 2026-09-06 从 Manager.tsx 原地搬出（纯搬家，零行为变化）+ 合并 ProviderManager 的
 * 盲存表单（补 codex_model 字段、放宽校验），从此全 app 只剩这一份实现。
 *
 * 2026-09-06（同日二次改版，B1-B4）：按 astra-ui-design-2.md B 节重排——
 *  - B1 三种入口形态（模板 / 自定义 / 编辑），模板墙收进「从模板选择」折叠区；
 *  - B2 OpenAI / Anthropic 地址同级放进「连接信息」组，各带粘贴按钮；
 *  - B3 API Key 默认遮挡 + 粘贴 + 眼睛图标；
 *  - B4 底栏「仅保存」+「验证并保存」合流校验，失败详情脱敏折叠展示。
 */
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  CheckCircle2,
  Eye,
  EyeOff,
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

/** URL 取 host 用于模板路径首屏摘要行；解析失败就原样返回，不让用户看见报错。 */
function hostOf(url: string): string {
  try {
    return new URL(url).host || url;
  } catch {
    return url;
  }
}

/** 把当前填的 Key 从错误详情里挖掉——试连失败的原始报错来自上游，可能把整条请求回显回来。 */
function redactKey(msg: string, key?: string | null): string {
  if (!key || key.trim().length < 4) return msg;
  return msg.split(key).join("***");
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
  const isDrawer = variant === "drawer";

  /** 当前表单的 baseUrl 命中哪个模板；都不命中 = 自定义。 */
  const activeTpl = templates.find((tpl) => tpl.openai_base === value.openai_base.trim());
  /** 三种入口形态之一：模板路径（右栏「+」预填进来，命中模板且是新建）。 */
  const isTemplatePath = !isEdit && !isDrawer && !!activeTpl;
  /** 自定义路径：空白新建、还没命中任何模板。 */
  const isCustomNew = !isEdit && !isDrawer && !activeTpl;

  /**
   * 存前试连 + 存前拉模型（2026-08-22，用户亲历「无法准确添加新的供应商」后重做）。
   *
   * 🔴 原来的流程是**盲存**：填完只能保存，Key 抄错一位 / base 少个 /v1 / 模型 id 打错，
   * 第一条报错出现在列表深处甚至切驱动失败时 —— 离「你填错的那一格」隔着三层。
   * 现在错误死在弹窗里：拉得到模型清单只证明端点可达；试连回话才证明 Key 和模型都对了。
   * 某些上游（如 OpenRouter）允许匿名读取 /models，不能把模型清单当作 Key 校验。
   *
   * 2026-09-06 二次改版：「测试连通」与「保存」合流成一个「验证并保存」按钮（B4）；
   * `verifying` 取代原来的 `probing`，`probe` 结果沿用。
   */
  const [verifying, setVerifying] = useState(false);
  const [probe, setProbe] = useState<TestResult | null>(null);
  const [fetchingList, setFetchingList] = useState(false);
  const [modelList, setModelList] = useState<string[]>([]);
  const [listErr, setListErr] = useState<string | null>(null);
  const [showKey, setShowKey] = useState(false);
  const [pasteErrField, setPasteErrField] = useState<string | null>(null);
  const [connInfoOpen, setConnInfoOpen] = useState(!isTemplatePath);
  const apiKeyRef = useRef<HTMLInputElement>(null);
  const prevIsTemplatePath = useRef(isTemplatePath);

  // 入口形态在渲染期间变化（比如自定义路径里点了一个模板）时，跟着切一次连接信息组的展开态、
  // 模板路径下自动聚焦密钥 —— 只在「跨越那条边界」的那一刻动一次，别一直纠缠用户手动的折叠。
  useEffect(() => {
    if (isTemplatePath !== prevIsTemplatePath.current) {
      setConnInfoOpen(!isTemplatePath);
      prevIsTemplatePath.current = isTemplatePath;
    }
    if (isTemplatePath) apiKeyRef.current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isTemplatePath]);

  // 任何一格变了，上一次的试连结果就不再作数 —— 留着一个绿勾伴着改坏的表单，比没有更糟。
  const set = (patch: Partial<ProviderPreset>) => {
    setProbe(null);
    if (freeRoute?.stage === "added") onFreeRouteDirty?.();
    onChange({ ...value, ...patch });
  };

  const pasteInto = async (field: "openai_base" | "anthropic_base" | "api_key") => {
    try {
      const text = await navigator.clipboard.readText();
      if (field === "openai_base") set({ openai_base: text });
      else if (field === "anthropic_base") set({ anthropic_base: text });
      else set({ api_key: text });
      setPasteErrField(null);
    } catch {
      setPasteErrField(field);
    }
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

  /** 「仅保存」（B4）：现有保存路径原样，不测速。 */
  const saveOnly = () => submit();

  /** 「验证并保存」（B4）：先探测 openai 端点，成功再保存；纯 Anthropic 配置无法测，直接保存。 */
  const verifyAndSave = async () => {
    if (!canSave || verifying) return;
    if (noOpenaiBase) {
      submit();
      return;
    }
    setVerifying(true);
    setProbe(null);
    const r = await invoke<TestResult>("probe_endpoint", {
      baseUrl: value.openai_base.trim(),
      apiKey: value.api_key ?? "",
      model: value.model.trim(),
    }).catch((e) => ({ ok: false, api: "openai", latency_ms: 0, reply: null, error: String(e) }) as TestResult);
    setProbe(r);
    setVerifying(false);
    if (r.ok) submit();
  };

  const noOpenaiBase = !value.openai_base.trim();
  const fieldsDisabled = verifying;

  const title = isDrawer
    ? t("正在接入：{name}", { name: freeRoute?.entry.name ?? value.name })
    : isEdit
      ? t("编辑供应商")
      : isTemplatePath && activeTpl
        ? t("添加 {name}", { name: activeTpl.name })
        : t("添加供应商");

  const primaryLabel = verifying
    ? t("验证中…")
    : noOpenaiBase
      ? t("保存")
      : isEdit
        ? t("验证并保存修改")
        : t("验证并保存");

  return (
    <div
      className={cn("fixed inset-0 z-[60]", isDrawer ? "pointer-events-none" : "grid place-items-center bg-black/60 backdrop-blur-sm p-4")}
      onClick={isDrawer ? undefined : onClose}
    >
      <div
        className={cn(
          "border border-white/[0.10] bg-bg-1 shadow-card flex flex-col",
          isDrawer
            ? "pointer-events-auto absolute right-0 top-0 h-full w-full max-w-[480px] rounded-l-card"
            : "w-full max-w-[560px] max-h-[calc(100vh-32px)] rounded-card",
        )}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-5 h-13 py-3.5 border-b border-white/[0.08] bg-bg-1/60">
          <div className="text-[14px] font-semibold text-ink-0">{title}</div>
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

        <div className="px-5 py-5 space-y-4 flex-1 overflow-y-auto">
          {/* 「从模板选择」——只在自定义新建路径出现；选中即预填并自动切到模板路径形态
              （靠 activeTpl 派生，不需要额外状态）。模板路径 / 编辑路径都不重复列模板墙（B1）。 */}
          {isCustomNew && (addable.length > 0 || templates.length > 0) && (
            <details className="group rounded-lg border border-white/[0.08] bg-bg-2/40 px-3 py-2">
              <summary className="cursor-pointer text-[12px] font-medium text-ink-2 hover:text-ink-0 list-none select-none flex items-center justify-between">
                <span>{t("从模板选择")}</span>
                <span className="text-ink-4">▾</span>
              </summary>
              <div className="mt-2.5 space-y-3">
                {addable.length > 0 && (
                  <div>
                    <div className="text-[11.5px] font-medium text-ink-1 mb-1.5 flex items-center gap-1.5">
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
                  </div>
                )}
                <div>
                  <div className="text-[11.5px] font-medium text-ink-1 mb-1.5">{t("预设供应商")}</div>
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
                    {templates.map((tpl) => (
                      <button
                        key={tpl.name}
                        onClick={() => applyTemplate(tpl)}
                        title={tpl.openai_base}
                        className="px-2.5 h-7 rounded-md text-[11.5px] font-medium border border-white/[0.10] text-ink-2 hover:bg-white/[0.04] transition-colors"
                      >
                        {tpl.name}
                      </button>
                    ))}
                  </div>
                  <div className="mt-1.5 text-[10.5px] text-ink-4 leading-snug">
                    {t("💡 点一个自动填好接口地址，下方只需补 API Key；选「自定义」则全部手填。存好后可在列表里「🔄 拉取」选具体模型。")}
                  </div>
                </div>
              </div>
            </details>
          )}

          {/* 名称：模板路径压成一行摘要（名称 + 域名），其余路径正常展示为独立字段（B1）。 */}
          {isTemplatePath ? (
            <div className="flex items-center gap-3">
              <input
                value={value.name}
                onChange={(e) => set({ name: e.target.value })}
                disabled={fieldsDisabled}
                className={cn(IPT, "font-semibold flex-1 min-w-0 disabled:opacity-60")}
              />
              <div
                className="shrink-0 text-[12px] text-ink-3 font-mono truncate max-w-[45%]"
                title={value.openai_base}
              >
                {hostOf(value.openai_base)}
              </div>
            </div>
          ) : (
            <Field label={t("名称")} hint={t("给这个供应商起个名字，如「我的中转」")}>
              <input
                value={value.name}
                onChange={(e) => set({ name: e.target.value })}
                placeholder={t("我的供应商")}
                disabled={fieldsDisabled}
                className={cn(IPT, "disabled:opacity-60")}
              />
            </Field>
          )}

          {/* 模板路径：密钥在名称摘要之后立刻出现（自动聚焦）；自定义/编辑路径：密钥在连接信息组之后。 */}
          {isTemplatePath && (
            <Field label="API Key">
              <div className="flex items-center gap-1.5">
                <input
                  ref={apiKeyRef}
                  type={showKey ? "text" : "password"}
                  value={value.api_key ?? ""}
                  onChange={(e) => set({ api_key: e.target.value })}
                  placeholder="sk-..."
                  disabled={fieldsDisabled}
                  className={cn(IPT, "font-mono flex-1 min-w-0 disabled:opacity-60")}
                />
                <button
                  type="button"
                  onClick={() => pasteInto("api_key")}
                  disabled={fieldsDisabled}
                  className="shrink-0 h-9 w-[52px] rounded-lg border border-white/[0.10] text-ink-2 text-[11.5px] hover:bg-white/[0.04] disabled:opacity-40 transition-colors"
                >
                  {t("粘贴")}
                </button>
                <button
                  type="button"
                  onClick={() => setShowKey((v) => !v)}
                  title={showKey ? t("隐藏密钥") : t("显示密钥")}
                  className="shrink-0 grid place-items-center w-9 h-9 rounded-lg border border-white/[0.10] text-ink-2 hover:bg-white/[0.04] transition-colors"
                >
                  {showKey ? <EyeOff size={14} /> : <Eye size={14} />}
                </button>
              </div>
              {pasteErrField === "api_key" && (
                <div className="mt-1 text-[10.5px] text-danger-400">{t("请使用 Ctrl+V 粘贴")}</div>
              )}
              {value.key_hint && <div className="mt-1.5 text-[10.5px] text-ink-4 leading-snug">{value.key_hint}</div>}
              {value.key_url && (
                <a href={value.key_url} target="_blank" rel="noreferrer" className="mt-1 inline-block text-[10.5px] text-accent hover:underline">
                  {t("获取 Key")}
                </a>
              )}
            </Field>
          )}

          {isTemplatePath && (
            <Field label={t("模型")} hint={t("填好地址和 Key 后点「拉取」，从这家真实有的模型里选 —— 不用去官网抄")}>
              <div className="flex items-center gap-1.5">
                <input
                  value={value.model}
                  onChange={(e) => set({ model: e.target.value })}
                  placeholder="gpt-4o / deepseek-v4-flash ..."
                  list="add-provider-models"
                  disabled={fieldsDisabled}
                  className={cn(IPT, "font-mono flex-1 min-w-0 disabled:opacity-60")}
                />
                <button
                  onClick={fetchModels}
                  disabled={fetchingList || noOpenaiBase || fieldsDisabled}
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
          )}

          {/* 连接信息组（B2）：OpenAI / Anthropic 地址同级并排，各带粘贴按钮。
              模板路径默认折叠（「查看/修改连接信息」）；自定义/编辑路径默认展开。 */}
          <details
            className="group rounded-lg border border-white/[0.08] bg-bg-2/30 px-3 py-2.5"
            open={connInfoOpen}
            onToggle={(e) => setConnInfoOpen(e.currentTarget.open)}
          >
            <summary className="cursor-pointer text-[12px] font-medium text-ink-1 list-none select-none flex items-center justify-between">
              <span>{t("连接信息")}</span>
              {isTemplatePath && (
                <span className="text-ink-4 text-[11px] font-normal">
                  {connInfoOpen ? t("收起") : t("查看/修改连接信息")}
                </span>
              )}
            </summary>
            <div className="mt-2.5 space-y-3">
              <div>
                <div className="text-[12px] font-medium text-ink-0 mb-2">{t("OpenAI 地址")}</div>
                <div className="flex items-center gap-1.5">
                  <input
                    value={value.openai_base}
                    onChange={(e) => set({ openai_base: e.target.value })}
                    placeholder="https://api.example.com/v1"
                    disabled={fieldsDisabled}
                    className="w-full h-10 flex-1 min-w-0 rounded-lg border border-white/[0.10] bg-bg-2 px-3 text-[13px] font-mono text-ink-1 outline-none focus:border-accent/50 placeholder:text-ink-4 disabled:opacity-60"
                  />
                  <button
                    type="button"
                    onClick={() => pasteInto("openai_base")}
                    disabled={fieldsDisabled}
                    className="shrink-0 h-10 w-[52px] rounded-lg border border-white/[0.10] text-ink-2 text-[11.5px] hover:bg-white/[0.04] disabled:opacity-40 transition-colors"
                  >
                    {t("粘贴")}
                  </button>
                </div>
                {pasteErrField === "openai_base" && (
                  <div className="mt-1 text-[10.5px] text-danger-400">{t("请使用 Ctrl+V 粘贴")}</div>
                )}
              </div>
              <div>
                <div className="text-[12px] font-medium text-ink-0 mb-2">{t("Anthropic 地址（可选）")}</div>
                <div className="flex items-center gap-1.5">
                  <input
                    value={value.anthropic_base ?? ""}
                    onChange={(e) => set({ anthropic_base: e.target.value })}
                    placeholder={t("留空 = 仅 OpenAI 兼容")}
                    disabled={fieldsDisabled}
                    className="w-full h-10 flex-1 min-w-0 rounded-lg border border-white/[0.10] bg-bg-2 px-3 text-[13px] font-mono text-ink-1 outline-none focus:border-accent/50 placeholder:text-ink-4 disabled:opacity-60"
                  />
                  <button
                    type="button"
                    onClick={() => pasteInto("anthropic_base")}
                    disabled={fieldsDisabled}
                    className="shrink-0 h-10 w-[52px] rounded-lg border border-white/[0.10] text-ink-2 text-[11.5px] hover:bg-white/[0.04] disabled:opacity-40 transition-colors"
                  >
                    {t("粘贴")}
                  </button>
                </div>
                {pasteErrField === "anthropic_base" && (
                  <div className="mt-1 text-[10.5px] text-danger-400">{t("请使用 Ctrl+V 粘贴")}</div>
                )}
              </div>
              <div className="text-[10.5px] text-ink-4 leading-snug">
                {t("按供应商提供的信息填写，至少填写一种地址。")}
              </div>
            </div>
          </details>

          {/* 自定义 / 编辑路径：密钥和模型排在连接信息组之后（模板路径已在上方提前展示）。 */}
          {!isTemplatePath && (
            <Field label="API Key">
              <div className="flex items-center gap-1.5">
                <input
                  ref={apiKeyRef}
                  type={showKey ? "text" : "password"}
                  value={value.api_key ?? ""}
                  onChange={(e) => set({ api_key: e.target.value })}
                  placeholder="sk-..."
                  disabled={fieldsDisabled}
                  className={cn(IPT, "font-mono flex-1 min-w-0 disabled:opacity-60")}
                />
                <button
                  type="button"
                  onClick={() => pasteInto("api_key")}
                  disabled={fieldsDisabled}
                  className="shrink-0 h-9 w-[52px] rounded-lg border border-white/[0.10] text-ink-2 text-[11.5px] hover:bg-white/[0.04] disabled:opacity-40 transition-colors"
                >
                  {t("粘贴")}
                </button>
                <button
                  type="button"
                  onClick={() => setShowKey((v) => !v)}
                  title={showKey ? t("隐藏密钥") : t("显示密钥")}
                  className="shrink-0 grid place-items-center w-9 h-9 rounded-lg border border-white/[0.10] text-ink-2 hover:bg-white/[0.04] transition-colors"
                >
                  {showKey ? <EyeOff size={14} /> : <Eye size={14} />}
                </button>
              </div>
              {pasteErrField === "api_key" && (
                <div className="mt-1 text-[10.5px] text-danger-400">{t("请使用 Ctrl+V 粘贴")}</div>
              )}
              {value.key_hint && <div className="mt-1.5 text-[10.5px] text-ink-4 leading-snug">{value.key_hint}</div>}
              {value.key_url && (
                <a href={value.key_url} target="_blank" rel="noreferrer" className="mt-1 inline-block text-[10.5px] text-accent hover:underline">
                  {t("获取 Key")}
                </a>
              )}
            </Field>
          )}

          {!isTemplatePath && (
            <Field label={t("模型")} hint={t("填好地址和 Key 后点「拉取」，从这家真实有的模型里选 —— 不用去官网抄")}>
              <div className="flex items-center gap-1.5">
                <input
                  value={value.model}
                  onChange={(e) => set({ model: e.target.value })}
                  placeholder="gpt-4o / deepseek-v4-flash ..."
                  list="add-provider-models"
                  disabled={fieldsDisabled}
                  className={cn(IPT, "font-mono flex-1 min-w-0 disabled:opacity-60")}
                />
                <button
                  onClick={fetchModels}
                  disabled={fetchingList || noOpenaiBase || fieldsDisabled}
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
          )}

          <details className="group">
            <summary className="cursor-pointer text-[11.5px] text-ink-3 hover:text-ink-1 list-none select-none">
              {t("＋ 高级（小模型 / Codex 专用模型，可不填）")}
            </summary>
            <div className="mt-2.5 space-y-3">
              <Field label="Small Model" hint={t("省 token 的轻量模型，留空 = 同上")}>
                <input
                  value={value.small_model}
                  onChange={(e) => set({ small_model: e.target.value })}
                  placeholder={t("留空则用上面的模型")}
                  disabled={fieldsDisabled}
                  className={cn(IPT, "font-mono disabled:opacity-60")}
                />
              </Field>
              <Field label={t("Codex 模型（可空）")} hint={t("Codex 固定使用新版 Responses 协议；保存前请确认供应商支持 /responses。")}>
                <input
                  value={value.codex_model ?? ""}
                  onChange={(e) => set({ codex_model: e.target.value })}
                  placeholder={t("沿用默认模型")}
                  disabled={fieldsDisabled}
                  className={cn(IPT, "font-mono disabled:opacity-60")}
                />
              </Field>
            </div>
          </details>

          {/* 彻底删除移到表单末尾的低频区（B4）——原来在底栏，跟高频的「移出当前列表」
              长得太像，容易误触；确认流程原样保留在 onPurge 回调里。 */}
          {isEdit && !value.builtin && onPurge && (
            <div className="border-t border-white/[0.08] pt-3.5">
              <button
                data-action-id="runtime.provider.delete"
                onClick={() => onPurge(value)}
                title={t("从全部 AI 的列表里删掉，并销毁它的地址和已保存的 Key")}
                className="inline-flex items-center gap-1.5 h-8 px-3 rounded-lg border border-danger-500/25 text-danger-400 text-[12px] font-medium hover:bg-danger-500/10 transition-colors"
              >
                <Trash2 size={13} /> {t("彻底删除")}
              </button>
            </div>
          )}
        </div>

        {/* 试连结果 —— 紧贴按钮区，成败都说人话。绿 = 这套填法真能回话；红 = 先给一句可操作说明，
            原始报错（脱敏）折叠在「查看详情」里，此刻表单还开着，改完再试，不用保存-失败-再回来。 */}
        {probe && (
          <div
            className={cn(
              "mx-5 mb-2.5 rounded-lg px-3 py-2 text-[11px] leading-snug border",
              probe.ok
                ? "bg-success-500/[0.08] text-success-400 border-success-500/20"
                : "bg-danger-500/[0.08] text-danger-400 border-danger-500/20",
            )}
          >
            <div className="flex items-start gap-2">
              {probe.ok ? <CheckCircle2 size={13} className="shrink-0 mt-px" /> : <XCircle size={13} className="shrink-0 mt-px" />}
              <span className="min-w-0 break-all">
                {probe.ok
                  ? t("「{reply}」· {ms}ms · 可以保存了", { reply: probe.reply ?? "", ms: probe.latency_ms })
                  : t("连接失败，请查看详情")}
              </span>
            </div>
            {!probe.ok && probe.error && (
              <details className="mt-1.5 ml-5">
                <summary className="cursor-pointer text-[10.5px] text-danger-400/80 hover:text-danger-400 select-none">
                  {t("查看详情")}
                </summary>
                <div className="mt-1 text-[10.5px] break-all text-danger-400/90">{redactKey(probe.error, value.api_key)}</div>
              </details>
            )}
          </div>
        )}
        <div className="flex items-center justify-end gap-2 px-5 py-3.5 border-t border-white/[0.08]">
          {!isDrawer && freeRoute?.stage !== "added" && (
            <button
              onClick={saveOnly}
              disabled={!canSave || verifying}
              className="mr-auto h-9 px-2 rounded-lg text-ink-2 text-[12px] font-medium hover:text-ink-0 hover:bg-white/[0.04] disabled:opacity-40 transition-colors"
            >
              {t("仅保存")}
            </button>
          )}
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
          ) : isDrawer ? (
            <button
              data-action-id="runtime.provider.save"
              onClick={submit}
              disabled={!canSave}
              className="h-9 px-5 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-40 shadow-sm transition-colors"
            >
              {t("保存 Key 和供应商")}
            </button>
          ) : (
            <>
              {noOpenaiBase && (
                <span className="text-[10.5px] text-ink-4 mr-1">{t("当前不支持此协议测试")}</span>
              )}
              <button
                data-action-id="runtime.provider.save"
                onClick={verifyAndSave}
                disabled={!canSave || verifying}
                className="inline-flex items-center gap-1.5 h-9 px-5 rounded-lg bg-accent text-white text-[12px] font-semibold hover:bg-accent-600 disabled:opacity-40 shadow-sm transition-colors"
              >
                {verifying ? <Loader2 size={13} className="animate-spin" /> : <Zap size={13} />}
                {primaryLabel}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
