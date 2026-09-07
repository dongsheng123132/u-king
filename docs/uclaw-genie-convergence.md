未在 D: 全盘搜到 u-king / u-claw 仓（全盘 glob 超时），以下方案基于你给的事实陈述写，未做代码核对；落地前 P2 第一步就是核 `usb_genie.rs` 现有形状。

---

# U 盘 AI 精灵 × U-Claw 合流开发方案

## 0. 一句话结论

**不做两条产品线。** U-King 是唯一壳，U 盘上跑的是「target」，PicoClaw 和 U-Claw 是同一张列表里的两张卡片，共用一个「U 盘工具盘」板块、共用一套配置屏外壳、字段由 manifest 驱动各自渲染。

**先做的一刀：把已有的 `usb_genie.rs` 反向收编成第一个 manifest target，用 PicoClaw 自证抽象层够用——然后才接 U-Claw。** 先接 U-Claw 会让你用一个 target 去验一套抽象，验不出来。

---

## 1. 产品形态终局图

用户只装一个东西：**U-King**（本机安装版或绿色版）。插上 U 盘后单屏里看到：

```
┌─ U-King ────────────────────────────────────────────┐
│  [本机工具]   [U 盘工具盘]   [钱包]   [设置]         │
├─────────────────────────────────────────────────────┤
│  U 盘工具盘        F: KINGSTON (exFAT, 57.2G 可用)   │
│                                                     │
│  ┌───────────────────────┐ ┌───────────────────────┐│
│  │ 🧞 AI 精灵 (PicoClaw) │ │ 🦞 U-Claw (OpenClaw)  ││
│  │ v0.4.2 · 已就绪       │ │ v2.2.1 · 运行中 :18788││
│  │ 轻量 · 不联网也能开    │ │ 完整内核 · 需配 API   ││
│  │ [启动] [配置] [日志]  │ │ [停止] [配置] [日志]  ││
│  └───────────────────────┘ └───────────────────────┘│
│  ⚠ U 盘上的凭据为明文保存。钱包与充值只在本机，不写入 U 盘。│
└─────────────────────────────────────────────────────┘
```

- 两张卡片，**同一个板块**。分板块是把「一个 U-King 一套界面」当场作废，不做。
- 点「配置」→ 同一个抽屉式配置屏，标题换成 target 名，字段列表由该 target 的 manifest `config.fields` 生成。用户学一次就会两个。
- 差异只体现在**卡片副标题的一句人话定位**：精灵=轻、开箱、离线可用；U-Claw=完整 OpenClaw、能力全、要配 key。这句话是唯一的产品分工说明，不写第二处。
- 没插 U 盘 / 盘上什么都没有 → 板块显示引导：「把 U-Claw 或 AI 精灵装到这个 U 盘」+ 两个安装按钮（复用已有 pack-usb 通道）。

---

## 2. 阶段切分

### P1（进行中，**不并入**）：PicoClaw 精灵 GUI 开关 + 绿色版本体
按 809 行原计划收口，不为合流改一行。

**验收判据**
- 绿色版 U-King 从 U 盘/桌面解压即跑，不写注册表、不依赖本机 Node。
- GUI 开关能开关精灵，`usb_genie.rs` 现有链路 F 盘真机通。
- 真机门槛：**exFAT U 盘**上完成一次冷启（拔插后重来）。

**门槛没过就不进 P2。** P1 拖着尾巴做抽象，等于在流沙上建楼。

---

### P2（合流第一刀）：target 抽象层落地 + PicoClaw 收编
不加任何新功能，纯把 P1 已通的链路搬到抽象层背后，产出**零行为变化**。

工作项：
1. 静态注册 8 个 action（见 §3.3），进 action-parity + allowlist。
2. `targets/picoclaw.manifest.json` 打进 U-King 包内，`usb_genie.rs` 改成 manifest 解释器的一个调用方。
3. UI：`UsbToolDisk.tsx` 改成渲染 `target.list` 返回的数组（此时数组长度=1）。
4. 配置屏改成 schema 驱动渲染。

**验收判据（硬）**
- `action list --json` 里 target.* 全部可枚举，parity 三端（GUI/CLI/MCP）生成物无 diff。
- 578 Rust 测试全绿 + 新增 manifest 解析/校验测试。
- **回归判据**：P1 的每一条真机验收原样重跑一遍，行为逐条相同。抽象层引入的任何行为变化都算 bug，不算改进。
- 真机门槛：exFAT 盘上 P1 全套复测通过。

---

### P3（新增工作）：U-Claw 接入为第二个 target
1. `targets/u-claw.manifest.json`（config transport = `http-api`）。
2. U-King 侧实现 `http-api` transport：起 → 探 `/api/runtime` → 读 `/api/config` → 写 `POST /api/config`。**绝不落 openclaw.json 一个字节。**
3. u-claw 仓侧 patch（跨仓只 patch）：`/api/runtime` 补齐 `diskRoot`（当前进程实际所在盘符）与 `version`，供 U-King 按盘精确判活；补 `GET /api/health` 轻量探针避免每次拉全量 config。
4. 卡片状态机接 config-server 的 gateway-check / local-models 做「配置是否可用」的绿灯。

**并行关系**：P3 的**配置屏视觉**（两个 target 的字段布局、明文凭据警示条、状态色）交给 UI 合伙人，与 P2 的后端抽象**并行**；你自己不并行做 P2/P3 后端。

**验收判据**
- U-Claw 从 U-King 一键启动 → 配置 → 保存 → 重启后配置生效，全程没碰过 Config.html。
- 两张卡片同屏，各自启停互不干扰，端口/进程各归各（launch 去重逻辑覆盖两个 target）。
- 杀掉 U-King，两个运行时继续活着（stdio 脱钩，来自 usb-genie-probe-lessons）。

---

### P4（发版门槛）：exFAT 双 target 共存冒烟 + 双仓发版
**这是 exFAT 死穴的正面冲撞点。** OpenClaw 官方 env key 命中即强装插件 → exFAT 拒启，v2.2.0+ 已修待实机冒烟。

**真机门槛（一条不过就不发）**
1. 出厂 exFAT U 盘，全新格式化，装两个 target。
2. 三种 key 场景各跑一遍：无 key / 官方 env key 命中 / 走 U-King secret provider 注入。
3. 拔插 3 次、换机 2 台（不同盘符、其中一台无本机 Node）。
4. 长路径 + 中文路径 + 盘符 ≥ H。

**exFAT 不过的降级预案**（写进方案，不是临场决定）：U-Claw target 降级为 `capabilities: ["detect","start","stop"]`，配置屏显示「此运行时在 exFAT 上仅支持启停，配置请用 NTFS 盘或本机安装」。**不为了让它过而在 U-King 侧塞 workaround 去改 OpenClaw 行为**——那是替上游背债。

---

### 明确不做的三件

1. **U-DSH 接壳**——上一轮已定，不重开。
2. **运行时自带 manifest / 从 U 盘发现 target**——只认 U-King 包内静态 manifest 表。U 盘上的文件能声明自己怎么被执行，等于把任意代码执行权交给一只 U 盘。这条是安全红线不是工程取舍。
3. **U-Claw 的钱包/充值代理**——U-King 不把 wallet/* 那套 API 转发给 U-Claw 的 config-server，也不反过来。钱包只在宿主侧。

---

## 3. manifest / target schema 最小设计

### 3.1 完整字段（v1，够直接开工）

```jsonc
{
  "schemaVersion": 1,
  "id": "u-claw",                    // 稳定标识，进 allowlist、日志、遥测
  "displayName": "U-Claw (OpenClaw)",
  "tagline": "完整 OpenClaw 内核 · 需配置 API",
  "kind": "openclaw",                // openclaw | picoclaw —— 闭集，编译期穷举
  "capabilities": ["detect","start","stop","status","config"],

  "detect": {
    "requiredPaths": ["U-Claw/portable/node/node.exe",
                      "U-Claw/portable/openclaw/package.json"],
    "versionFrom": { "path": "U-Claw/portable/VERSION", "format": "text" },
    "minVersion": "2.2.0"            // 低于此版本：检出但标「需升级」，不给启动按钮
  },

  "launch": {
    "exec": "U-Claw/启动.bat",
    "args": [],
    "cwd": "U-Claw",
    "detach": true,                  // 必须 true
    "stdio": "null",                 // 必须 null —— lessons：不脱钩会连坐
    "envAllow": ["OPENCLAW_PORTABLE"], // 白名单，宿主环境变量默认不透传
    "singletonKey": "disk+id"        // 去重键：盘符+target id，不是全局
  },

  "health": {
    "type": "http",                  // http | file | process
    "url": "http://127.0.0.1:18788/api/health",
    "timeoutMs": 3000,
    "matchDisk": true,               // 必须 true：按盘精确匹配，防止 D 盘实例冒充 F 盘
    "diskField": "diskRoot"          // 响应里用哪个字段比对盘符
  },

  "stop": { "type": "port-owner", "port": 18788, "graceMs": 2000, "killAfterMs": 5000 },

  "config": {
    "transport": "http-api",         // http-api | file —— 只有这两种，不做通用插件
    "baseUrl": "http://127.0.0.1:18788",
    "read":  { "method": "GET",  "path": "/api/config" },
    "write": { "method": "POST", "path": "/api/config" },
    "requiresRunning": true,         // 配置前壳自动拉起，配置完按用户原状态还原
    "fields": [ /* 见 3.2 */ ]
  },

  "constraints": {
    "filesystem": ["exFAT", "NTFS"],
    "credentialPlaintext": true      // 驱动 UI 上那条明文警示，不可被 manifest 关掉
  }
}
```

PicoClaw 的差异只在两块：

```jsonc
  "kind": "picoclaw",
  "config": {
    "transport": "file",
    "path": "picoclaw/credentials.yml",
    "format": "yaml",
    "shapeLock": "picoclaw-v1"       // 凭据 yml 形状生死线：写回前按锁校验，形状不符拒写
  }
```

### 3.2 field 字段

```jsonc
{
  "key": "provider.apiKey",          // 点号路径，映射到目标配置结构
  "label": "API Key",
  "type": "secret",                  // string | secret | enum | bool | number
  "required": true,
  "placeholder": "sk-...",
  "enumFrom": { "path": "/api/provider-models", "valueField": "id" }, // type=enum 时
  "help": "保存在 U 盘上，明文。",
  "redact": true                     // secret 强制 true，日志/遥测/错误信息一律打码
}
```

### 3.3 静态 action 注册（8 个，target 是**入参**不是维度）

```
target.list            → 列出当前检出的 target（含盘符、版本、状态）
target.detect  {disk}  → 扫一个盘
target.start   {id,disk}
target.stop    {id,disk}
target.status  {id,disk}
target.config.get {id,disk}
target.config.set {id,disk,patch}
target.manifest.list   → 列出包内 manifest（调试/自检用）
```

**这是本方案对 action-parity 的核心保障**：新增第三个 target 不新增 action，注册表长度恒为 8，allowlist 编译期穷举不受影响。任何提议「给 U-Claw 单开一组 action」的做法直接否掉。

### 3.4 编译期断言（写成 Rust test，不是约定）

- `kind == "openclaw"` ⟹ `config.transport == "http-api"`。**堵死任何人直接写 openclaw.json 的路。**
- `type == "secret"` ⟹ `redact == true`。
- `constraints.credentialPlaintext` 只能是 `true`（U 盘 target 全体适用）。
- manifest 里出现任何形如 key/token/secret 的**值**（非字段声明）→ 测试失败。
- `launch.detach == true && launch.stdio == "null"`。

---

## 4. 开源发布面

### u-king 仓（壳与精灵的真相源）

**带什么**：Tauri 壳全部源码、107+8 action、target manifest 目录 `targets/*.json`、`usb_genie.rs`、`UsbToolDisk.tsx`、578+ 测试、`docs/usb-ai-genie-plan.md`、`docs/portable-ai-experience.md`、新增 `docs/target-manifest-spec.md`。

**发什么包**
| 包 | 内容 | 面向 |
|---|---|---|
| `u-king-setup-x64.exe` | 本机安装版 | 主流用户 |
| `u-king-portable-x64.zip` | 绿色版（P1 产物） | 不想装的人 |
| `usb-ai-genie-<ver>.zip` | 精灵 U 盘本体（PicoClaw 内核） | 直接往 U 盘解压 |

**README 叙事**：「**你的 AI 工具的总控台**。装一个 U-King，管好本机和 U 盘上的所有 AI 运行时。」——讲**壳**，不讲运行时能力。U 盘那段只用一屏带过并链到 u-claw 仓。

### u-claw 仓（OpenClaw 运行时与 config-server 的真相源）

**带什么**：portable 骨架、9 个 bat、config-server 全部源码 + **`docs/config-server-api.md`（新增：把已有 JSON API 写成契约文档，这是 U-King 依赖的公开面）**、Config.html。

**发什么包**：`U-Claw-<openclaw版本>.zip` 一个，保持现状。**不发壳、不内嵌 U-King。**

**README 叙事**：「**OpenClaw 的 U 盘版，插上就能跑。**」——讲**运行时**。底部加一节「想要图形化设置界面？→ U-King」，一个链接，不喧宾夺主。1729 星的资产价值在于它是 OpenClaw 用户的入口，把它改成 U-King 的宣传页会把这批人赶走。

**跨仓规则**：u-king 不 vendor u-claw 的任何代码，只在 manifest 里写路径约定 + 在 CI 里跑一个「对着某个 U-Claw 发行版验 manifest 能否检出」的 smoke。u-claw 侧只接受 §P3.3 那三个小 patch（`diskRoot`、`version`、`/api/health`），不为 U-King 改架构。

---

## 5. 红线清单（绝不抽、绝不做）

1. **不直接读写 `openclaw.json`。** 一律走 config-server。有编译期断言兜底。
2. **钱包 / 充值 / secret provider / allowlist 永远只在 U-King 宿主侧**，不进 U 盘、不代理给 config-server、不写进任何 U 盘文件。
3. **U 盘凭据明文的诚实声明不退。** 不加「已加密」措辞，不做只挡眼睛的混淆。这条在 README 和 UI 上同时在场。
4. **action 注册表恒静态可枚举。** 不做动态 action、不做 target 自注册。
5. **manifest 只从 U-King 包内加载。** U 盘上的文件永远不是可信输入。
6. **不新开仓、不做通用插件系统、不做中立通配壳。**
7. **不为让 exFAT 过关而在壳里 hack 上游 OpenClaw 行为。** 过不了就按 §P4 降级并如实写在 README 里。
8. **U-DSH 不接壳。**

---

## 6. 回本判据与放弃条件

这套合流的本质是：**用 U-Claw 的 1729 星把流量导进 U-King，用 U-King 的钱包把流量变成收入。** 所以判据只看这条链路。

**回本判据（P4 发版后 30 天，按序判定）**
1. u-claw README 的 U-King 链接点击 → U-King 下载转化 ≥ **8%**（低于此说明叙事分工失败，改文案不改架构）。
2. U-King 新增装机里，从 u-claw 来的 ≥ **25%**（低于此说明 U-Claw 不是有效入口，合流的战略前提不成立）。
3. 装了 U-King 的人里，**双 target 同时检出** ≥ 15%（低于此说明两个运行时其实是两拨人，共存设计的价值证伪 → 但**不回退成分板块**，只降低对 U-Claw target 的后续投入）。
4. 钱包充值转化不低于合流前基线（合流不应稀释付费路径）。

**放弃条件（触发即停，不加码）**
- **exFAT 冒烟连续两轮不通且根因在 OpenClaw 上游** → U-Claw target 永久降级为「只检出只启停」，P3 的配置链路封存，不再投入。
- **config-server API 在两个 OpenClaw 小版本内发生破坏性变更** → 说明它不是稳定契约，U-Claw target 降级为启停，配置回归 Config.html。
- **P2 抽象层导致 P1 真机回归出现无法在 3 天内定位的行为差异** → 回退 P2，PicoClaw 保持直连实现，U-Claw 合流整体推迟一个版本。抽象层的唯一价值是不破坏现状。
- **判据 2 低于 10%** → 停止 U-Claw 侧一切新增投入，U-King 回归单 target（精灵）产品，u-claw 仓维持纯运行时开源，两条线各活各的。

---

要我把这份直接写进 u-king 仓的 `docs/uclaw-genie-convergence.md` 的话，给我仓库路径就行。
