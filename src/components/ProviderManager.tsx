/**
 * ProviderManager —— 自定义 provider 增 / 删 / 改弹层。
 *
 * 内置预置只读（列出但不可改），自定义 provider 可编辑/删除/新建。
 * 调后端 add_provider / update_provider / delete_provider（存 ~/.uking/providers.json）。
 * 用 editId 直接进编辑态（从 ProviderSwitch 的「编辑」按钮带进来）。
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { X, Plus, Pencil, Trash2, Save } from "lucide-react";
import type { ProviderPreset } from "../Wizard";
import { cn } from "../lib/cn";
import { useI18n } from "../i18n";

type Form = {
  id: string;
  name: string;
  openai_base: string;
  anthropic_base: string;
  model: string;
  small_model: string;
  codex_model: string;
  codex_wire_api: string;
  api_key: string;
};

const EMPTY: Form = {
  id: "",
  name: "",
  openai_base: "",
  anthropic_base: "",
  model: "",
  small_model: "",
  codex_model: "",
  // 新版 Codex 只接受 responses；后端也会强制归一，前端不能再给一个会被静默忽略的选项。
  codex_wire_api: "responses",
  api_key: "",
};

export function ProviderManager({
  editId,
  onToast,
  onClose,
  onChanged,
}: {
  /** 打开即编辑的 provider id；undefined = 新建态 */
  editId?: string;
  onToast: (s: string) => void;
  onClose: () => void;
  onChanged: () => void;
}) {
  const [providers, setProviders] = useState<ProviderPreset[]>([]);
  const [form, setForm] = useState<Form>(EMPTY);
  const [editing, setEditing] = useState<boolean>(!!editId);
  const [busy, setBusy] = useState(false);
  const { t } = useI18n();

  const refresh = useCallback(async () => {
    const p = await invoke<ProviderPreset[]>("list_providers").catch(() => []);
    setProviders(p);
    return p;
  }, []);

  // 初次加载：列表 + 若带 editId 进编辑态
  useEffect(() => {
    void (async () => {
      const p = await refresh();
      if (editId) {
        const t = p.find((x) => x.id === editId);
        if (t) startEdit(t);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const startEdit = (p: ProviderPreset) => {
    setForm({
      id: p.id,
      name: p.name,
      openai_base: p.openai_base || "",
      anthropic_base: p.anthropic_base || "",
      model: p.model || "",
      small_model: p.small_model || "",
      codex_model: p.codex_model || "",
      codex_wire_api: "responses",
      api_key: p.api_key || "",
    });
    setEditing(true);
  };

  const startNew = () => {
    setForm(EMPTY);
    setEditing(true);
  };

  const save = async () => {
    if (busy) return;
    if (!form.name.trim()) {
      onToast(t("请填写名称"));
      return;
    }
    if (!form.openai_base.trim() && !form.anthropic_base.trim()) {
      onToast(t("至少填一个端点（OpenAI 兼容地址 或 Anthropic 地址）"));
      return;
    }
    setBusy(true);
    try {
      const provider = {
        id: form.id,
        name: form.name.trim(),
        summary: "自定义中转站",
        openai_base: form.openai_base.trim(),
        anthropic_base: form.anthropic_base.trim() || null,
        model: form.model.trim() || "gpt-4o",
        small_model: form.small_model.trim() || form.model.trim() || "gpt-4o-mini",
        codex_model: form.codex_model.trim(),
        codex_wire_api: "responses",
        key_url: "",
        key_hint: "",
        builtin_recharge: false,
        recommended: false,
        api_key: form.api_key.trim(),
      };
      await invoke("add_provider", { provider });
      onToast(form.id ? t("已更新") : t("已添加自定义 provider"));
      await refresh();
      onChanged();
      setEditing(false);
      setForm(EMPTY);
    } catch (e) {
      onToast(String(e));
    } finally {
      setBusy(false);
    }
  };

  /** 彻底删除：不带 `tool` = 从**全部 AI** 的列表里拿掉，连定义带 Key 一起销毁。
   *  这里是「管理自定义供应商」的地方，删就是真删；只想让某个 AI 不用它，去它自己的
   *  列表里点垃圾桶（那条路只移出那一个 AI，见 providers.rs::remove_provider_for）。 */
  const del = async (p: ProviderPreset) => {
    try {
      await invoke("delete_provider", { id: p.id });
      onToast(t("已彻底删除 {name}（全部 AI + 已保存的 Key）", { name: p.name }));
      await refresh();
      onChanged();
    } catch (e) {
      onToast(String(e));
    }
  };

  const fld = (label: string, key: keyof Form, ph: string, type = "text") => (
    <label className="block">
      <span className="text-[11px] text-ink-3">{label}</span>
      <input
        type={type}
        value={form[key]}
        onChange={(e) => setForm((f) => ({ ...f, [key]: e.target.value }))}
        placeholder={ph}
        className="mt-1 w-full h-8 px-2.5 rounded-lg bg-bg-3 border border-white/[0.08] text-[12.5px] text-ink-0 placeholder:text-ink-5 focus:border-accent/50 outline-none"
      />
    </label>
  );

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60" onClick={onClose}>
      <div
        className="w-full max-w-lg max-h-[85vh] overflow-y-auto rounded-card border border-white/[0.10] bg-bg-1 shadow-pop"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center h-11 px-4 border-b border-white/[0.06]">
          <span className="text-[14px] font-semibold text-ink-0 flex-1">{t("管理驱动 / 中转站")}</span>
          <button onClick={onClose} className="w-7 h-7 grid place-items-center rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.06]">
            <X size={16} />
          </button>
        </div>

        <div className="p-4 space-y-3">
          {!editing ? (
            <>
              <div className="space-y-1">
                {providers.map((p) => (
                  <div
                    key={p.id}
                    className="flex items-center gap-2 rounded-lg border border-white/[0.06] bg-white/[0.02] px-3 py-2"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="text-[12.5px] text-ink-1 truncate flex items-center gap-1.5">
                        {p.name}
                        {p.builtin && <span className="text-[9px] text-ink-5">{t("内置")}</span>}
                      </div>
                      <div className="text-[10px] text-ink-4 truncate font-mono">
                        {p.anthropic_base || p.openai_base || "—"}
                      </div>
                    </div>
                    {!p.builtin ? (
                      <>
                        <button onClick={() => startEdit(p)} className="w-7 h-7 grid place-items-center rounded text-ink-4 hover:text-ink-1 hover:bg-white/[0.06]" title={t("编辑")}>
                          <Pencil size={13} />
                        </button>
                        <button data-action-id="runtime.provider.delete" onClick={() => del(p)} className="w-7 h-7 grid place-items-center rounded text-ink-4 hover:text-danger-400 hover:bg-white/[0.06]" title={t("彻底删除（全部 AI + 已保存的 Key）")}>
                          <Trash2 size={13} />
                        </button>
                      </>
                    ) : (
                      <span className="text-[10px] text-ink-5 pr-1">{t("只读")}</span>
                    )}
                  </div>
                ))}
              </div>
              <button
                onClick={startNew}
                className="w-full inline-flex items-center justify-center gap-1.5 h-9 rounded-lg border border-dashed border-white/[0.14] text-[12.5px] text-ink-2 hover:bg-white/[0.03] hover:border-accent/40"
              >
                <Plus size={14} /> {t("添加自定义 provider")}
              </button>
            </>
          ) : (
            <>
              <div className="text-[12px] text-ink-2 font-medium">
                {form.id ? t("编辑：{name}", { name: form.name }) : t("新建自定义 provider")}
              </div>
              {fld(t("名称"), "name", t("我的中转站"))}
              {fld(t("OpenAI 兼容端点（Codex/ClawX/Hermes 用）"), "openai_base", "https://your-relay.com/v1")}
              {fld(t("Anthropic 端点（Claude Code 用，可空）"), "anthropic_base", "https://your-relay.com")}
              <div className="grid grid-cols-2 gap-2">
                {fld(t("默认模型"), "model", "gpt-4o")}
                {fld(t("小任务模型"), "small_model", "gpt-4o-mini")}
              </div>
              {fld(t("Codex 模型（可空）"), "codex_model", t("沿用默认模型"))}
              <p className="text-[10px] leading-4 text-ink-4">
                {t("Codex 固定使用新版 Responses 协议；保存前请确认供应商支持 /responses。")}
              </p>
              {fld("API Key", "api_key", "sk-...", "password")}

              <div className="flex items-center gap-2 pt-1">
                <button
                  data-action-id="runtime.provider.save"
                  onClick={save}
                  disabled={busy}
                  className={cn(
                    "inline-flex items-center gap-1.5 px-4 h-9 rounded-lg bg-accent text-white text-[12.5px] font-semibold hover:bg-accent-600 disabled:opacity-50",
                  )}
                >
                  <Save size={14} /> {busy ? t("保存中…") : t("保存")}
                </button>
                <button
                  onClick={() => {
                    setEditing(false);
                    setForm(EMPTY);
                  }}
                  className="px-4 h-9 rounded-lg border border-white/[0.10] text-[12.5px] text-ink-2 hover:bg-white/[0.04]"
                >
                  {t("取消")}
                </button>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
