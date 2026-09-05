/**
 * ProviderManager —— 自定义 provider 增 / 删 / 改弹层。
 *
 * 内置预置只读（列出但不可改），自定义 provider 可编辑/删除/新建。
 * 用 editId 直接进编辑态（从 ProviderSwitch 的「编辑」按钮带进来）。
 *
 * 2026-09-06：编辑表单退场，改挂全 app 唯一一份 `CustomProviderModal`（模板预填 / 拉模型清单 /
 * 存前试连 / 彻底删除齐全，原来这里的盲存表单没有这些，Key 抄错一位也要等切驱动失败才看到）。
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { X, Plus, Pencil, Trash2 } from "lucide-react";
import type { ProviderPreset } from "../Wizard";
import { useI18n } from "../i18n";
import { CustomProviderModal } from "./CustomProviderModal";

/** 新建态的空白 preset，字段对齐后端 upsert 期望的形状（save_custom_provider）。 */
const EMPTY_PRESET: ProviderPreset = {
  id: "",
  name: "",
  summary: "自定义中转站",
  openai_base: "",
  anthropic_base: null,
  model: "",
  small_model: "",
  codex_model: "",
  codex_wire_api: "responses",
  key_url: "",
  key_hint: "API Key",
  builtin_recharge: false,
  recommended: false,
  builtin: false,
  api_key: "",
};

export function ProviderManager({
  editId,
  onToast,
  onClose,
  onChanged,
  /** 加到哪个 AI 的列表里（per-tool key，如 "claude"/"codex"）；不传 = 不自动挂到任何工具。 */
  addingTo,
}: {
  /** 打开即编辑的 provider id；undefined = 新建态 */
  editId?: string;
  onToast: (s: string) => void;
  onClose: () => void;
  onChanged: () => void;
  addingTo?: string;
}) {
  const [providers, setProviders] = useState<ProviderPreset[]>([]);
  const [editing, setEditing] = useState<ProviderPreset | null>(null);
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
        const found = p.find((x) => x.id === editId);
        if (found) startEdit(found);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const startEdit = (p: ProviderPreset) => setEditing(p);
  const startNew = () => setEditing({ ...EMPTY_PRESET });

  const handleSave = async (p: ProviderPreset) => {
    const isNew = !editing?.id;
    try {
      const saved = await invoke<ProviderPreset>("add_provider", { provider: p });
      await refresh();
      onChanged();
      // 修「新建后不挂到当前工具」的行为缺口：新建 + 带了工具上下文才自动挂进去，
      // 编辑既有条目不动它已经在哪些工具列表里（那是列表行自己的垃圾桶/一键加回管的事）。
      if (isNew && addingTo) {
        await invoke("restore_provider", { id: saved.id, tool: addingTo });
        onToast(t("已添加「{name}」并加入 {tool} 的列表", { name: saved.name, tool: addingTo }));
      } else {
        onToast(isNew ? t("已添加自定义 provider") : t("已更新"));
      }
      setEditing(null);
    } catch (e) {
      onToast(String(e));
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

  if (editing) {
    return (
      <CustomProviderModal
        value={editing}
        onChange={setEditing}
        onSave={handleSave}
        onClose={() => setEditing(null)}
        onPurge={editing.id && !editing.builtin ? del : undefined}
        variant="modal"
      />
    );
  }

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
        </div>
      </div>
    </div>
  );
}
