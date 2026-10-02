/**
 * 「启动前按需配好虾盘云」—— 免配置启动的那几个工具（openclaw / hermes / dsh）共用的一份实现。
 *
 * 原来住在 `opencodex/ToolAppView.tsx`（工具专属页点「启动」时调）。2026-10-02 起 Hermes 从
 * 「点启动先切到专属页再点一次」改成「我的 AI 双击 = 选文件夹 → 开终端」，启动链路
 * （`App.tsx::runLaunchAction`）也要在开终端之前做同一件事——同一段逻辑不许抄第二遍
 * （宪法第 8 条），所以抽到这里，两边都调它。
 *
 * 规矩没变（对齐「无主动不切换、全部纯手动」）：
 *  · 只在用户**主动点了启动**时调，打开页面不偷偷写；
 *  · 只在「还没被任何驱动接管」时配；用户已手动切过的（含官方直连 / 自备 Key）一律尊重、绝不覆盖；
 *  · claude / codex 不在此列——用户常有自己的官方登录，点启动只是跑 CLI、绝不替他切驱动；
 *  · 配失败不打断启动（调用方照常往下开终端），用户仍可在 AI 设置里手动切。
 */
import { invoke } from "@tauri-apps/api/core";
import type { DeviceKey, DriverStatus } from "./types";

/** 启动前需要「按需配虾盘云」的工具 id（cline 已下架，不再列）。 */
const AUTO_CONFIG_TOOLS = new Set(["openclaw", "hermes", "dsh"]);

export function needsAutoConfig(toolId: string): boolean {
  return AUTO_CONFIG_TOOLS.has(toolId);
}

/** 这个工具是否已被任何驱动接管（接管了就别自动回灌，尊重用户可能切到的官方直连）。 */
export function targetConfigured(configTargets: string[], d: DriverStatus | null): boolean {
  if (!d) return false;
  const t = configTargets[0];
  // 用户显式选过的驱动（含「官方直连」official）一律算已接管 —— 绝不回灌覆盖。
  // 这条优先级最高：还原到 official 后 config.toml 可能被删（没了 model_provider），
  // 若只看下面的实时配置会误判成「没配过」→ 把虾盘云又写回去，造成「怎么都还原不了」。
  if (d.active?.[t]) return true;
  if (t === "claude") return !!d.claude_base;
  if (t === "codex") return !!d.codex_provider;
  if (t === "clawx") return !!d.clawx_model;
  if (t === "dsh") return !!d.dsh_model;
  // Hermes 特例：**不能**用 `!!d.hermes_model` 兜底 —— Hermes 首次运行会**自造**一个默认
  // 模型（alibaba/qwen3.7-max），config.yaml 里永远有 model.default，导致这里恒为 true →
  // 启动时的自动配置被跳过 → Hermes 用它自己的 qwen 默认 + 空 Key → HTTP 401（客户实锤，
  // 见截图 2026-07-08）。真正的「已配过」信号是 active["hermes"]（上面已处理，来自
  // 显式记录或 base_url 反推的已知 provider）。无该记录 = 从没被我们/用户配过 → 该自动配虾盘云。
  if (t === "hermes") return false;
  return false;
}

export type EnsureDriverOpts = {
  /** 工具 id（TUI_APPS 的 id 或 ToolInfo.id，这几个工具两边同名）。 */
  toolId: string;
  /** 提示里显示的产品名。 */
  name: string;
  /** 要写的驱动配置目标（TUI_APPS[].configTargets）。 */
  configTargets: string[];
  deviceKey: DeviceKey | null;
  onRefreshDriver: () => void;
  onToast: (s: string) => void;
  /** i18n 翻译函数（`useI18n().t`）。 */
  tr: (zh: string, vars?: Record<string, string | number>) => string;
};

/** 按需把虾盘云写成该工具的默认驱动。返回是否这次真的写了（调用方一般不用关心）。 */
export async function ensureDefaultDriver(o: EnsureDriverOpts): Promise<boolean> {
  if (!o.deviceKey?.key) return false;
  if (!needsAutoConfig(o.toolId)) return false;
  const d = await invoke<DriverStatus>("get_driver_status").catch(() => null);
  if (!d) return false;
  if (targetConfigured(o.configTargets, d)) return false; // 已配过（含官方直连）→ 尊重用户选择，不动
  try {
    await invoke("apply_provider", {
      providerId: "xiapan",
      apiKey: o.deviceKey.key,
      model: null,
      targets: o.configTargets,
    });
    o.onRefreshDriver();
    o.onToast(
      o.tr("{name} 已配好虾盘云", { name: o.name }) + (o.toolId === "openclaw" ? o.tr("（ClawX 需重启）") : ""),
    );
    return true;
  } catch {
    /* 配失败不打断启动，右侧 ProviderSwitch / AI 设置仍可手动切 */
    return false;
  }
}
