/**
 * 复制虾盘云内置 Key 的小按钮 —— 客户要把 Key 粘到别处（第三方工具、模型设置）时一键复制。
 * 零依赖：WebView2 直接用 navigator.clipboard。
 *
 * 历史：曾有 "conn"（{"_type":"newapi_channel_conn",...}）导入格式，但 ClawX 4.x 根本不认那个
 * 串（误导客户），已删除。要把整套配置交给 AI 自动配，用 Guide.tsx 的「复制配置文档」。
 */
import { useState } from "react";
import { Check, Copy } from "lucide-react";
import { cn } from "../lib/cn";
import { copyToClipboard } from "../lib/clipboard";
import { useI18n } from "../i18n";

export function CopyKey({
  apiKey,
  label,
  onToast,
  className,
}: {
  apiKey: string;
  label?: string;
  onToast?: (s: string) => void;
  className?: string;
}) {
  const { t } = useI18n();
  const [done, setDone] = useState(false);

  const copy = async () => {
    if (await copyToClipboard(apiKey)) {
      setDone(true);
      onToast?.(t("已复制 API Key"));
      window.setTimeout(() => setDone(false), 1800);
    } else {
      onToast?.(t("复制失败，请手动选中复制"));
    }
  };

  return (
    <button
      onClick={copy}
      title={apiKey}
      className={cn(
        "inline-flex items-center gap-1.5 px-3 h-8 rounded-lg border border-white/[0.10] text-[12px] transition-colors",
        done ? "text-success-400 border-success-500/30" : "text-ink-1 hover:bg-white/[0.04]",
        className,
      )}
    >
      {done ? <Check size={13} /> : <Copy size={13} />}
      {done ? t("已复制") : label ?? t("复制 Key")}
    </button>
  );
}
