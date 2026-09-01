/**
 * 设备钱包卡 —— 余额 / 充值 / 复制 Key / 换一把 / 用已有钱包 / 移除本机钱包，
 * **全项目只此一份实现**。
 *
 * 🔴 2026-08-22 收敛：1.0.5 在「我的 AI」页独立长出过第二份 `DeviceWalletCard`（235 行，
 * 功能几乎相同，三处确认框全是 `window.confirm` —— 在 Tauri 里恒真，等于没问就执行）。
 * 两份互不知道对方存在，正是「同一事实存在几份就会漂几份」的现场。已删那份，
 * 它独有的「移除本机钱包」搬进本文件 —— 删的是实现，不是能力。
 *
 * 为什么抽出来：钱包是「虾盘云这个供应商」的一部分，不是 U-King 的全局功能。
 * 它原来在「虾盘云·充值」页和「进阶」页各写了一份（余额条一份、换/填 Key 一份），
 * 同一事实两份实现就会漂两份（宪法第 8 条）—— 已经漂过：一份有余额没换 Key，
 * 另一份有换 Key 没余额，客户在哪页能干什么全凭运气。
 *
 * 归属：挂在虾盘云供应商卡片上。删掉虾盘云 → 卡片没了 → 钱包 UI 跟着消失；加回来就回来。
 * 所以本组件**不自己判断该不该显示**，由挂载方（供应商卡）决定。
 *
 * 充值动作由调用方传 `onRecharge` —— App.tsx 那条「开充值页 + 定时回查到账」是唯一一份，
 * 组件自己 openUrl 会绕过回查，客户付了钱看不到余额变化。
 */
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { KeyRound, RefreshCw, Wallet } from "lucide-react";
import { CopyKey } from "./CopyKey";
import { useI18n } from "../i18n";
import { cn } from "../lib/cn";
import { askConfirm } from "../lib/confirm";
import type { DeviceKey } from "../lib/types";

export function WalletCard({
  deviceKey,
  onDeviceKeyChange,
  onRefresh,
  onRecharge,
  onToast,
  className,
}: {
  /** 由挂载方持有（App/Manager 各自的真相源），本组件只读它渲染。 */
  deviceKey: DeviceKey | null;
  /** 刷新 / 换一把 / 填一把之后把新 DeviceKey 回传，挂载方好同步自己的状态。 */
  onDeviceKeyChange?: (dk: DeviceKey) => void;
  /** 挂载方自己有一条更全的刷新（比如 App 那条还会顺带刷新「下一步」提示条）时传进来，
   *  本组件就不再自己 invoke —— 同一件事两条路会让两处的状态各走各的。 */
  onRefresh?: () => void | Promise<void>;
  onRecharge: () => void;
  onToast: (s: string) => void;
  className?: string;
}) {
  const { t } = useI18n();
  const [refreshing, setRefreshing] = useState(false);
  const [rotating, setRotating] = useState(false);
  const [adopting, setAdopting] = useState(false);
  const [resetting, setResetting] = useState(false);
  const [adoptOpen, setAdoptOpen] = useState(false);
  const [adoptInput, setAdoptInput] = useState("");

  const key = deviceKey?.key ?? "";
  const charged = !!deviceKey?.charged;

  /** 重新读一次设备钱包。写动作成功后也走这里 —— 「换完 Key 界面还显示旧的」是
   *  最容易被当成「换失败了」的假象，于是客户再点一次，白白又换一把。 */
  const reload = async (toast?: string) => {
    setRefreshing(true);
    try {
      if (onRefresh) {
        // 挂载方那条自己会 toast（App 的还会区分「到账了 / 还没查到」），这里不再重复喊一遍
        await onRefresh();
      } else {
        const dk = await invoke<DeviceKey>("get_device_key");
        onDeviceKeyChange?.(dk);
        if (toast) onToast(toast);
      }
    } catch (e) {
      onToast(t("刷新余额失败：") + String(e));
    } finally {
      setRefreshing(false);
    }
  };

  /**
   * 换一把新 Key。后端两阶段：新的验通了才吊销旧的，所以中途失败旧 Key 照样能用。
   * 会吊销旧 Key，而旧 Key 可能正被客户自己的脚本 / 另一台机器引用着 —— 必须问一句。
   */
  const rotate = async () => {
    if (rotating) return;
    // 🔴 用 askConfirm 不用 window.confirm：Tauri 把 window.confirm 换成了返回 Promise 的版本，
    //    `if (!window.confirm(...))` 恒不成立 —— 那等于没问就换（见 lib/confirm.ts）。
    if (!(await askConfirm(t("换一把新的 API Key？旧 Key 会立即失效，余额自动保留。\n如果你把旧 Key 配到过别的电脑或脚本里，那边需要重新填。")))) return;
    setRotating(true);
    try {
      const r = await invoke<{ message?: string }>("rotate_device_key", { confirm: true });
      await reload(r?.message ?? t("已更新访问密钥"));
    } catch (e) {
      onToast(t("更新密钥失败：") + String(e));
    } finally {
      setRotating(false);
    }
  };

  /**
   * 用一把已有的 Key（换电脑 / 这台机器上的另一份安装 / 自己在官网生成的那把）。
   *
   * 这是整套「Key 就是一张充值卡」的另一半：**能换、也能搬**。没有它，客户换台电脑
   * 就得重新充值，余额留在老机器上花不掉 —— 而那正是我们最怕的那种投诉。
   *
   * 用内联输入框而不是 `window.prompt`：Tauri webview 里 prompt 不保证可用，
   * 而这一步失败的表现是「点了没反应」，客户只会以为功能坏了。
   */
  const adopt = async () => {
    const val = adoptInput.trim();
    if (adopting || !val) return;
    // 会顶掉本机当前那把。当前这把如果有余额而客户没备份，顶掉就找不回来了 —— 要问一句。
    if (!(await askConfirm(t("用这把 Key 替换本机当前的 Key？\n当前这把如果还有余额且你没有备份，替换后将无法自动找回。")))) return;
    setAdopting(true);
    try {
      const r = await invoke<{ message?: string }>("adopt_device_key", { key: val, confirm: true });
      setAdoptOpen(false);
      setAdoptInput("");
      await reload(r?.message ?? t("已启用这把密钥"));
    } catch (e) {
      onToast(t("这把密钥用不了：") + String(e));
    } finally {
      setAdopting(false);
    }
  };

  /**
   * 只从这台机器移除钱包（服务端钱包和余额都还在）。1.0.5 随设备钱包一起发出去的能力，
   * 08-22 收敛 UI 时跟着搬过来 —— **删的是第二份实现，不是这个能力**：老用户上一版
   * 见过这颗按钮，这版找不到会当成功能没了（宪法：能力和入口是两个开关，别一起拉）。
   *
   * 「先复制、复制不成功就不许继续」是这颗按钮的安全带：移除之后本机会重新生成一把零余额
   * 的新 Key，原钱包只能靠那串字符找回。剪贴板写不进去还往下走 = 让用户在没有退路时删账户。
   */
  const resetLocal = async () => {
    if (resetting || !key) return;
    try {
      await navigator.clipboard.writeText(key);
      onToast(t("当前 Key 已自动复制，请先保存好再确认移除"));
    } catch {
      onToast(t("无法自动复制当前 Key，已取消移除；请先手动复制"));
      return;
    }
    // 同 rotate/adopt：askConfirm 而不是 window.confirm（后者在 Tauri 里恒真 = 没问就删）。
    if (!(await askConfirm(t("只从这台机器移除设备钱包？服务端钱包、旧 Key 和余额不会删除。\n下次联网会生成一把新的零余额 Key；你仍可用刚复制的这把找回原钱包。")))) return;
    setResetting(true);
    try {
      const r = await invoke<{ message?: string }>("reset_device_wallet", { confirm: true });
      await reload(r?.message ?? t("已从本机移除设备钱包"));
    } catch (e) {
      // pending 收尾时后端会要求重新确认 —— 先把卡片刷成后端真正的状态，再报错。
      await reload();
      onToast(t("移除本机钱包失败：") + String(e));
    } finally {
      setResetting(false);
    }
  };

  const btn =
    "rounded-md border border-white/[0.12] px-2.5 py-1 text-[11px] text-ink-2 hover:text-ink-1 hover:border-white/25 disabled:opacity-40";

  // 只有 rotate/adopt 两颗按钮挂 data-action-id —— 它们对应真实的影核动作；
  // 余额刷新/充值走的是 GUI 侧的组合流程，没有对应动作 id，硬编一个会让
  // `action bindings` 核对到一个不存在的动作（假绿比不绿更坏）。
  return (
    <div
      className={cn("rounded-lg border border-white/[0.08] bg-bg-0/40 px-3 py-2.5 space-y-2", className)}
    >
      {/* 🔴 这台机器换过密钥体系但本机配置丢了 —— 老余额找不回来了。
          说清楚比装作没事强：客户看到 ¥0 只会以为钱蒸发了，然后去骂人。 */}
      {deviceKey?.legacy_balance_unrecoverable && (
        <div className="rounded-md border border-amber-500/40 bg-amber-500/10 px-2.5 py-1.5 text-[11.5px] text-ink-1">
          {t("检测到本机配置曾丢失，已重新生成一把 Key。原来的余额无法自动找回 —— 请凭充值订单号联系客服迁移。")}
        </div>
      )}

      {/* 余额 + 充值 + 刷新 */}
      <div className="flex flex-wrap items-center gap-2">
        <Wallet size={14} className="text-accent shrink-0" />
        <span className="text-[11px] text-ink-4">{t("余额")}</span>
        <span
          className={cn(
            "font-mono text-[13px] font-semibold",
            deviceKey && !charged ? "text-red-300" : "text-accent",
          )}
        >
          {deviceKey?.balance?.text ?? (deviceKey ? t("待充值") : t("检测中…"))}
        </span>
        <button
          onClick={onRecharge}
          className="ml-auto inline-flex items-center gap-1.5 px-3 h-7 rounded-md bg-accent text-white text-[11.5px] font-semibold hover:bg-accent-600"
        >
          {charged ? t("继续充值") : t("去充值开通")}
        </button>
        <button
          onClick={() => void reload(t("已刷新余额"))}
          disabled={refreshing}
          title={t("充值后点这里确认是否到账")}
          className="inline-flex items-center justify-center w-7 h-7 rounded-md border border-white/[0.10] text-ink-3 hover:text-ink-1 hover:bg-white/[0.04] disabled:opacity-60"
        >
          <RefreshCw size={12} className={refreshing ? "animate-spin" : ""} />
        </button>
      </div>

      {/* Key 本体 + 复制。这串字符就是账户，没有账号密码可找回 —— 所以「复制」是一等操作。 */}
      <div className="flex flex-wrap items-center gap-2">
        <span className="inline-flex items-center gap-1.5 px-2 h-7 rounded-md bg-bg-1 border border-white/[0.08] min-w-0">
          <KeyRound size={12} className="text-accent shrink-0" />
          <span className="font-mono text-[11.5px] text-ink-1 truncate" title={key}>
            {key ? `${key.slice(0, 12)}…${key.slice(-4)}` : t("检测中…")}
          </span>
        </span>
        {key && <CopyKey apiKey={key} onToast={onToast} className="h-7 px-2.5 text-[11.5px]" />}
        <div className="ml-auto flex shrink-0 items-center gap-1.5">
          <button
            type="button"
            data-action-id="runtime.device.key_adopt"
            onClick={() => setAdoptOpen((v) => !v)}
            disabled={adopting}
            className={btn}
          >
            {t("填入已有 Key")}
          </button>
          <button
            type="button"
            data-action-id="runtime.device.key_rotate"
            onClick={() => void rotate()}
            disabled={rotating || !key}
            className={btn}
          >
            {rotating ? t("更新中…") : t("更新 Key")}
          </button>
        </div>
      </div>

      {adoptOpen && (
        <div className="space-y-1.5 rounded-md border border-white/[0.08] bg-bg-1/60 px-2.5 py-2">
          <div className="text-[11px] text-ink-4">
            {t("把你已有的 Key 填进来（换了电脑、或者这台机器上的另一份 U-King，填同一把就能共用余额）：")}
          </div>
          <div className="flex items-center gap-1.5">
            <input
              value={adoptInput}
              onChange={(e) => setAdoptInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void adopt();
              }}
              spellCheck={false}
              placeholder="sk-…"
              className="min-w-0 flex-1 h-7 rounded-md border border-white/[0.10] bg-bg-0 px-2 font-mono text-[11.5px] text-ink-1 outline-none focus:border-accent/60"
            />
            <button type="button" onClick={() => void adopt()} disabled={adopting || !adoptInput.trim()} className={btn}>
              {adopting ? t("验证中…") : t("启用")}
            </button>
            <button
              type="button"
              onClick={() => {
                setAdoptOpen(false);
                setAdoptInput("");
              }}
              className={btn}
            >
              {t("取消")}
            </button>
          </div>
        </div>
      )}

      <div className="flex items-start gap-2">
        <div className="text-[10.5px] text-ink-4 leading-snug">
          {t("上面这把 Key 就是你的账户，复制下来存好。换电脑时填回去就能接着用；怀疑泄露了随时换一把，余额跟着走。")}
        </div>
        {/* 移除是破坏性动作，所以放在说明文字旁边而不是跟「更新 Key」并排 ——
            它和上面两颗不是同一量级，摆一起迟早有人点错。 */}
        <button
          type="button"
          data-action-id="runtime.device.wallet_reset_local"
          onClick={() => void resetLocal()}
          disabled={resetting || !key}
          title={t("服务端钱包和余额不会删除；移除前会先把当前 Key 复制给你")}
          className="ml-auto shrink-0 rounded-md border border-white/[0.08] px-2 py-1 text-[10.5px] text-ink-4 hover:text-red-300 hover:border-red-400/40 disabled:opacity-40"
        >
          {resetting ? t("移除中…") : t("移除本机钱包")}
        </button>
      </div>
    </div>
  );
}
