/**
 * ModelMenu —— 作图 / 视频共用的「富模型选择器」。
 *
 * 取代原来的 <datalist> 组合框：datalist 只能显示一行 value+label，塞不下「优缺点」。
 * 这里下拉里每个模型显示：友好名 + 推荐徽标 + 一句话优缺点(desc)，客户看明白再自己选。
 * 价格不展示——实扣由服务器 new-api ModelPrice 决定，前端不维护价格(免得上游改价就要发版+乱收费)。
 * 渲染模式照搬 ProviderSwitch 的模型下拉（视觉一致）。
 *
 * 仍保留**手填兜底**：底部一个输入框，上游换了模型 id 不发版也能直接填（对齐 cc-switch
 * 「自动化失败别挡手动」原则）。选中后按钮上显示友好名；手填的 id 找不到映射就原样显示。
 *
 * 纯展示组件，只靠 props（models / value / onChange）通信，符合模块解耦铁律。
 */
import { useEffect, useRef, useState } from "react";
import { Check, ChevronDown } from "lucide-react";
import { cn } from "../lib/cn";
import { useI18n } from "../i18n";

export type PickModel = {
  id: string;
  label: string;
  /** 给小白看的一句话优缺点 */
  desc?: string;
  recommend?: boolean;
};

export function ModelMenu({
  models,
  value,
  onChange,
  title,
  className,
  inputPlaceholder,
}: {
  models: PickModel[];
  value: string;
  onChange: (id: string) => void;
  title?: string;
  className?: string;
  /** 手填框占位文案（复用做尺寸选择器时传「手填尺寸…」）。默认按模型文案。 */
  inputPlaceholder?: string;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const cur = models.find((m) => m.id === value);
  // 选中项显示友好名；手填的（不在清单里）原样显示 id
  const display = cur?.label ?? value;

  // 点组件外部关闭下拉
  useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [open]);

  return (
    <div ref={ref} className={cn("relative shrink-0", className)}>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        title={title}
        className="h-9 px-2.5 flex items-center gap-1 rounded-lg bg-bg-3 border border-white/[0.08] text-[12px] text-ink-1 hover:border-accent/40 outline-none max-w-[190px]"
      >
        <span className="truncate">{display}</span>
        <ChevronDown size={12} className="shrink-0 text-ink-4" />
      </button>

      {open && (
        <div className="absolute right-0 bottom-full mb-1.5 z-30 w-[306px] max-h-[58vh] overflow-y-auto rounded-lg border border-white/[0.1] bg-bg-1 p-1 shadow-xl">
          {models.map((m) => (
            <button
              key={m.id}
              type="button"
              onClick={() => {
                onChange(m.id);
                setOpen(false);
              }}
              className={cn(
                "w-full flex items-start gap-1.5 px-2 py-1.5 rounded text-left hover:bg-white/[0.06]",
                value === m.id ? "bg-accent/[0.08]" : "",
              )}
            >
              <span className="w-[12px] pt-0.5 shrink-0">
                {value === m.id && <Check size={12} className="text-accent" />}
              </span>
              <span className="min-w-0 flex-1">
                <span className="flex items-center gap-1.5">
                  <span className="text-[12px] text-ink-1 truncate">{m.label}</span>
                  {m.recommend && (
                    <span className="text-[8.5px] px-1 rounded bg-accent/20 text-accent-400 shrink-0">
                      {t("推荐")}
                    </span>
                  )}
                </span>
                {m.desc && (
                  <span className="block text-[10.5px] leading-tight text-ink-4 mt-0.5">{m.desc}</span>
                )}
              </span>
            </button>
          ))}

          {/* 手填兜底：上游换了模型 id 不发版也能用 */}
          <div className="mt-1 border-t border-white/[0.06] pt-1.5 px-1 pb-0.5">
            <input
              defaultValue={cur ? "" : value}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  const v = (e.target as HTMLInputElement).value.trim();
                  if (v) {
                    onChange(v);
                    setOpen(false);
                  }
                }
              }}
              placeholder={inputPlaceholder ?? t("或手填模型 id，回车确认")}
              className="w-full px-2 h-7 rounded bg-bg-3 border border-white/[0.08] text-[11px] text-ink-2 font-mono outline-none focus:border-accent/40 placeholder:text-ink-5"
            />
          </div>
        </div>
      )}
    </div>
  );
}
