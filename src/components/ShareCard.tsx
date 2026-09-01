/**
 * 统一「成果海报」组件 —— 把各功能的战绩数据画成一张想晒出去的竖版 PNG，一键存到本地。
 *
 * 设计目标：**激起分享欲 + 引流**。Token 压缩机 / AI 优化大师 / Token 账单三处复用同一张海报模板，
 * 只传各自的 `spec`（已本地化）。海报底部带 U-King 二维码（扫码去官网下载）——分享 = 拉新客。
 *
 * **整块可插拔 · 纯前端**：
 *  - 只靠 props（`spec` + `onToast`）通信，不碰后端新命令；
 *  - 出图走原生 Canvas 2D（零第三方截图库，合"体积优先"），二维码用已有 `qrcode` 依赖；
 *  - 存盘**复用**现成的 `export_qr_merge`（通用 base64→原生另存为 PNG，不为它新增 Rust 命令）。
 *  删除本组件只需删本文件 + 三处 `<ShareButton>` 引用。
 */
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import QRCode from "qrcode";
import { Share2, Download, X, Loader2 } from "lucide-react";
import { useI18n } from "../i18n";

/** 一张海报要展示的内容（调用方用 t() 本地化好再传进来）。 */
export type ShareSpec = {
  /** 文件名 slug（如 "token-saver"）。 */
  kind: string;
  /** 顶部小徽章，如「AI 省钱战绩」。 */
  badge: string;
  /** 主标题一行，如「我用 U-King 省下」。 */
  title: string;
  /** 巨号主数字（字符串，调用方自己格式化好，如 "1,240万"）。 */
  heroValue: string;
  /** 主数字单位，如 "token"。 */
  heroUnit: string;
  /** 主数字下方补充，如「≈ 少充 ¥24.8」。 */
  heroSub?: string;
  /** 底部 2~3 个小指标。 */
  stats: { label: string; value: string }[];
  /** 最下方一行小字脚注（可选）。 */
  footnote?: string;
};

/** 官网（扫码引流；国内必开 → 用 u-claw.org.cn/uking/ 副本；品牌域 u-king.org 境内 SNI reset
 *  点不动，等备案迁国内后统一收敛回 www.u-king.org —— 会审 2026-08-27 P0）。 */
const SITE_URL = "https://u-claw.org.cn/uking/";
const SITE_LABEL = "U-King · 官网";

/** 取当前主题的 accent 真实色值，拿不到就回落 app 默认蓝紫。 */
function themeAccent(): string {
  try {
    const v = getComputedStyle(document.documentElement).getPropertyValue("--color-accent").trim();
    if (v) {
      const [r, g, b] = v.split(/\s+/).map(Number);
      if ([r, g, b].every((n) => Number.isFinite(n))) return `rgb(${r}, ${g}, ${b})`;
    }
  } catch {
    /* ignore */
  }
  return "rgb(79, 90, 192)";
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = reject;
    img.src = src;
  });
}

/** 把 spec 画成一张竖版海报，返回 PNG dataURL。 */
async function renderPoster(spec: ShareSpec): Promise<string> {
  const W = 1080;
  const H = 1500;
  const accent = themeAccent();
  const canvas = document.createElement("canvas");
  canvas.width = W;
  canvas.height = H;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("canvas 不可用");
  const YAHEI = '"Microsoft YaHei UI", "Microsoft YaHei", -apple-system, "PingFang SC", sans-serif';

  // ── 背景：近黑中性底 + 顶部 accent 辉光（跟 app 深色主题一致）──
  ctx.fillStyle = "#0d0d0f";
  ctx.fillRect(0, 0, W, H);
  const glow = ctx.createRadialGradient(W / 2, 300, 60, W / 2, 300, 760);
  glow.addColorStop(0, accent.replace("rgb", "rgba").replace(")", ", 0.30)"));
  glow.addColorStop(1, "rgba(13,13,15,0)");
  ctx.fillStyle = glow;
  ctx.fillRect(0, 0, W, 760);

  const M = 96; // 页边距

  // ── 品牌头 ──
  ctx.textBaseline = "alphabetic";
  ctx.fillStyle = "#f7f8f8";
  ctx.font = `700 46px ${YAHEI}`;
  ctx.fillText("U-King", M, 150);
  ctx.fillStyle = "rgba(161,161,170,0.9)";
  ctx.font = `400 26px ${YAHEI}`;
  ctx.fillText("· AI 装机管家", M + 200, 150);

  // ── 徽章 ──
  ctx.font = `600 28px ${YAHEI}`;
  const badgeW = ctx.measureText(spec.badge).width + 56;
  ctx.fillStyle = accent.replace("rgb", "rgba").replace(")", ", 0.18)");
  roundRect(ctx, M, 230, badgeW, 58, 29);
  ctx.fill();
  ctx.fillStyle = accent;
  ctx.fillText(spec.badge, M + 28, 269);

  // ── 主标题 ──
  ctx.fillStyle = "#e3e4e6";
  ctx.font = `400 40px ${YAHEI}`;
  ctx.fillText(spec.title, M, 400);

  // ── 巨号主数字 ──
  ctx.fillStyle = accent;
  ctx.font = `800 168px ${YAHEI}`;
  const numW = ctx.measureText(spec.heroValue).width;
  ctx.fillText(spec.heroValue, M, 570);
  ctx.fillStyle = "#a1a1aa";
  ctx.font = `600 44px ${YAHEI}`;
  ctx.fillText(spec.heroUnit, M + numW + 20, 570);

  if (spec.heroSub) {
    ctx.fillStyle = "#27a644";
    ctx.font = `600 40px ${YAHEI}`;
    ctx.fillText(spec.heroSub, M, 650);
  }

  // ── 小指标卡片（横排）──
  const gy = 740;
  const gh = 190;
  const gap = 28;
  const n = Math.min(spec.stats.length, 3) || 1;
  const gw = (W - M * 2 - gap * (n - 1)) / n;
  spec.stats.slice(0, 3).forEach((s, i) => {
    const gx = M + i * (gw + gap);
    ctx.fillStyle = "rgba(255,255,255,0.04)";
    roundRect(ctx, gx, gy, gw, gh, 24);
    ctx.fill();
    ctx.strokeStyle = "rgba(255,255,255,0.08)";
    ctx.lineWidth = 1.5;
    roundRect(ctx, gx, gy, gw, gh, 24);
    ctx.stroke();
    ctx.fillStyle = "#f7f8f8";
    ctx.font = `700 60px ${YAHEI}`;
    ctx.textAlign = "center";
    ctx.fillText(s.value, gx + gw / 2, gy + 100);
    ctx.fillStyle = "#8a8f98";
    ctx.font = `400 28px ${YAHEI}`;
    ctx.fillText(s.label, gx + gw / 2, gy + 150);
    ctx.textAlign = "left";
  });

  // ── 分隔线 ──
  ctx.strokeStyle = "rgba(255,255,255,0.08)";
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(M, 1010);
  ctx.lineTo(W - M, 1010);
  ctx.stroke();

  // ── 脚注 ──
  if (spec.footnote) {
    ctx.fillStyle = "#6b7280";
    ctx.font = `400 26px ${YAHEI}`;
    ctx.fillText(spec.footnote, M, 1068);
  }

  // ── 底部：二维码 + 引流文案 ──
  const qrData = await QRCode.toDataURL(SITE_URL, { margin: 1, width: 240, errorCorrectionLevel: "M", color: { dark: "#0d0d0f", light: "#ffffff" } });
  const qrImg = await loadImage(qrData);
  const qrSize = 200;
  const qrX = M;
  const qrY = 1150;
  ctx.fillStyle = "#ffffff";
  roundRect(ctx, qrX - 14, qrY - 14, qrSize + 28, qrSize + 28, 18);
  ctx.fill();
  ctx.drawImage(qrImg, qrX, qrY, qrSize, qrSize);

  ctx.fillStyle = "#f7f8f8";
  ctx.font = `700 40px ${YAHEI}`;
  ctx.fillText("扫码免费下载 U-King", qrX + qrSize + 44, qrY + 74);
  ctx.fillStyle = accent;
  ctx.font = `600 34px ${YAHEI}`;
  ctx.fillText(SITE_LABEL, qrX + qrSize + 44, qrY + 130);
  ctx.fillStyle = "#6b7280";
  ctx.font = `400 26px ${YAHEI}`;
  ctx.fillText("插上就用 · 一键配好全部 AI", qrX + qrSize + 44, qrY + 180);

  return canvas.toDataURL("image/png");
}

/**
 * 分享按钮：点它 → 生成海报 → 弹预览层 →「保存图片」另存为 PNG。
 * 放在有战绩数据的地方（数据为空时调用方别渲染它）。
 */
export function ShareButton({
  spec,
  onToast,
  label,
  className,
}: {
  spec: ShareSpec;
  onToast: (s: string) => void;
  label?: string;
  className?: string;
}) {
  const { t } = useI18n();
  const [preview, setPreview] = useState<string | null>(null);
  const [busy, setBusy] = useState<"gen" | "save" | null>(null);

  const generate = async () => {
    setBusy("gen");
    try {
      const url = await renderPoster(spec);
      setPreview(url);
    } catch (e) {
      onToast(t("海报生成失败：") + String(e));
    } finally {
      setBusy(null);
    }
  };

  const save = async () => {
    if (!preview) return;
    setBusy("save");
    try {
      const saved = await invoke<string | null>("export_qr_merge", {
        pngBase64: preview,
        filename: `U-King-${spec.kind}-成果卡.png`,
      });
      if (saved) {
        onToast(t("已保存海报：") + saved);
        setPreview(null);
      }
    } catch (e) {
      onToast(t("保存失败：") + String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <>
      <button
        onClick={generate}
        disabled={busy === "gen"}
        className={
          className ??
          "inline-flex items-center gap-1.5 px-3 h-8 rounded-lg border border-accent/40 bg-accent/[0.12] text-[12px] font-medium text-accent hover:bg-accent/20 disabled:opacity-60"
        }
      >
        {busy === "gen" ? <Loader2 size={13} className="animate-spin" /> : <Share2 size={13} />}
        {label ?? t("生成分享海报")}
      </button>

      {preview && (
        <div
          data-on-dark
          className="fixed inset-0 z-[100] grid place-items-center bg-black/70 p-6"
          onClick={() => busy !== "save" && setPreview(null)}
        >
          <div
            className="relative flex flex-col items-center gap-4 max-h-full"
            onClick={(e) => e.stopPropagation()}
          >
            <img
              src={preview}
              alt="poster"
              className="max-h-[72vh] w-auto rounded-xl shadow-2xl border border-white/10"
            />
            <div className="flex items-center gap-3">
              <button
                onClick={save}
                disabled={busy === "save"}
                className="inline-flex items-center gap-2 px-5 h-11 rounded-xl bg-accent text-white text-[14px] font-semibold hover:bg-accent-600 disabled:opacity-60"
              >
                {busy === "save" ? <Loader2 size={16} className="animate-spin" /> : <Download size={16} />}
                {t("保存图片")}
              </button>
              <button
                onClick={() => setPreview(null)}
                disabled={busy === "save"}
                className="inline-flex items-center gap-2 px-4 h-11 rounded-xl border border-white/15 text-ink-2 text-[14px] hover:bg-white/5 disabled:opacity-60"
              >
                <X size={16} />
                {t("关闭")}
              </button>
            </div>
            <p className="text-[12px] text-white/60">{t("保存后可发朋友圈 / 微信群 —— 扫码即可下载 U-King")}</p>
          </div>
        </div>
      )}
    </>
  );
}
