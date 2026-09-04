/** 跨组件共用的后端返回类型（与 Rust 端 serde 结构对齐）。 */

export type Balance = { tokens: number; cny?: number; text: string };

export type DeviceKey = {
  key: string;
  recharge_url: string;
  balance: Balance | null;
  charged: boolean;
  /** 🔴 余额 >0 但可能**不够垫一次请求**（上游按「最多可能用掉多少」预扣）。
   *  判据在后端（`device.rs`，带实测依据），前端只读不算 —— 这条规则原来在
   *  App.tsx 和 Manager.tsx 各写了一份 `cny < 0.5` 裸字面量，改门槛就会漂两份。 */
  low_balance?: boolean;
  /** 钱包 id（客服排障用）。老路径下为空。 */
  wallet_id?: string;
  /** 🔴 这台机器换过密钥体系，但本机配置丢了 —— 老余额技术上找不回来。
   *  界面必须据此明说「凭充值订单号联系客服」，不能默默给个 ¥0 的新钱包
   *  让客户以为钱蒸发了。丢状态不可怕，丢了还装作没丢才可怕。 */
  legacy_balance_unrecoverable?: boolean;
};

/**
 * 「AI 作图走哪家」的回显（后端 `providers::DrawRouteView` 镜像）。
 *
 * 放在这里而不是各页各写一份：Manager（选）和 Draw（显示走了谁）都要用它，
 * 而这个仓库曾经有一个 `DriverStatus` 被抄成四份的现场（本文件下面这个类型就是）。
 * App.tsx / Manager.tsx 已改成从这里 import；`CodexZone.tsx` 还留着一份 4 字段的
 * 窄子集（用不到全部字段，可以用 `Pick<>` 收编，记进了需求榜，这次没动）。
 * **不含 api_key** —— 前端不需要，少一处泄漏面少一次事故。
 */
export type DrawRoute = {
  provider_id: string;
  provider_name: string;
  /** 空 = 不覆盖，听「AI 作图」页那个模型下拉的。 */
  model: string;
  /** true = 走内置虾盘云端点 + 设备钱包 Key（默认，绝大多数客户是这个）。 */
  builtin: boolean;
  /** 这次真正会打的文生图端点，回显给用户看「到底打谁」。 */
  gen_url: string;
};

export type DriverStatus = {
  claude_base: string | null;
  claude_model: string | null;
  codex_provider: string | null;
  codex_model: string | null;
  clawx_model: string | null;
  clawx_installed: boolean;
  hermes_model: string | null;
  hermes_installed: boolean;
  dsh_model: string | null;
  dsh_installed: boolean;
  /** 用户在用自己的 Key（官方登录 / 自备 Key，非虾盘云）→ 不推虾盘云、不抢 Key
   *  （后端 providers.rs 探测）。**产品红线：不抢客户的模型/登录态。** */
  claude_own_key?: boolean;
  codex_own_key?: boolean;
  /** 每个工具当前生效的 provider id（后端按 cc-switch is_current 口径给出）。 */
  active: Record<string, string>;
  /** Claude Code 正走本机翻译桥（ANTHROPIC_BASE_URL 指着 127.0.0.1）。
   *  链路里多了 U-King 自己：**U-King 一退桥就没了**，界面必须一直摆着这句。 */
  claude_via_bridge?: boolean;
  /** 后上架那批 CLI（pi/qwen/crush/opencode）装没装。后端 `EXTRA_APPLY_TOOLS` 那一批，
   *  判据 = 它 apply 时用的同一个 `tool_installed` —— 界面能勾的必须 ⊇ 后端会配的。 */
  extra_installed?: Record<string, boolean>;
  /** 每个 AI 工具的可执行文件在哪发现的，不止「装没装」（后端 providers.rs discover_tools）。
   *  同名多处发现时全部保留，按 machine > portable 排好序，第一条是当前
   *  `search_paths`/`tool_installed` 实际会用到的那个。可选：老版本后端不返回它。 */
  discovered?: ToolDiscovery[];
};

/** `DriverStatus.discovered` 的一条记录（对应后端 `providers.rs::ToolDiscovery`）。 */
export type ToolDiscovery = {
  name: string;
  path: string;
  /** `"machine"` = 装在本机系统位置；`"portable"` = 自包含目录（绿色位/U 盘/usb_genie）。 */
  source: string;
  /** 只在零成本时填（同目录/上级目录有 package.json），拿不到就是 null。 */
  version: string | null;
  /** 这一处发现是不是 `active` 里记录的、当前实际生效的那份。 */
  configured: boolean;
};
