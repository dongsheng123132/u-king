建议采用：**右栏两 tab＋紧凑品牌行；Logo 使用本地 SVG 混合方案；工具卡压缩维护区，供应商卡先保证宽度再减高度。**

建议保存为 `docs/ui-spec-astra-3-provider-myai-2026-09-06.md`。当前权限只读，以下为完整改动清单，未写入文件。

依据：四张截图、上一轮规格及仓库最新提交 `1c17fe5`。审阅期间其他终端提交导致 `App.tsx` 行号前移，以下采用最新行号。已运行模板同步检查：三份清单均为 **24 条，一致**。

**1．右栏：采用两个 tab，免费算力保留快捷入口**

图2/3值得借鉴的是“同一位置切换来源分组”。建议保留上一轮主区＋右栏结构，把右栏改成：

```text
快速添加                         免费算力 →
┌──────────────┬──────────────┐
│ 模型厂商 16  │ 模型平台 8   │
└──────────────┴──────────────┘
 品牌标志  名称                    ＋  ↗
           接口域名
 品牌标志  名称                    ✎  ↗
           接口域名
 ……
 展开其余 8 家
```

这里的分类是 UI 分类建议：**模型厂商**放自有模型品牌；**模型平台**包含模型聚合、托管推理和中转服务。Groq、魔搭、硅基流动归入模型平台，避免把所有平台都叫“中转站”。

不采用“免费额度／官方直连／中转站”三个互斥 tab，因为免费是套餐属性，会与来源类型交叉。同一家不应为了免费档再出现一次。

| 改动位置 | 可执行修改 |
|---|---|
| [Manager.tsx:2227](src/Manager.tsx:2227) | 移除 `firstScreenNames` 与 `moreTemplates` 的首屏推荐分法，按展示元数据划分两个 tab；数量根据当前 `templates` 动态计算。 |
| [Manager.tsx:2297](src/Manager.tsx:2297) | iFlow、魔搭各自只保留一条模板行，归入模型平台。免费入口继续调用 `setSettingsTab("free")`，完整免费路线仍在已有分区。 |
| [Manager.tsx:2301](src/Manager.tsx:2301) | 默认显示所选组前 8 家，超出时显示“展开其余 N 家／收起”。保留组内现有顺序，不另造推荐排名。 |
| [Manager.tsx:2478](src/Manager.tsx:2478) | 右栏继续宽 `272px`，内边距从 `p-3.5` 收为 `p-2`，释放名称宽度。 |
| [Manager.tsx:2397](src/Manager.tsx:2397) | 窄窗口仍使用已有折叠区，内部复用同一目录；继续整页滚动。 |

当前代码的确有重复：`moreTemplates` 只排除了首屏四家，没有排除免费组，因此展开后 iFlow、魔搭会再次出现。

**单行结构建议**

替换 [Manager.tsx:2235](src/Manager.tsx:2235) 的 `renderQuickAddRow`。下面是结构示意，事件接回现有添加、编辑及外链函数：

```tsx
<li className="flex items-center gap-1 rounded-lg
               hover:bg-bg-3 focus-within:bg-bg-3">
  <button
    type="button"
    onClick={onAddOrEdit}
    aria-label={actionLabel}
    className="grid min-h-[52px] min-w-0 flex-1
               grid-cols-[24px_minmax(0,1fr)_32px]
               items-center gap-2 rounded-lg px-2 py-2 text-left
               focus-visible:outline-none focus-visible:ring-2
               focus-visible:ring-accent"
  >
    <ProviderLogo logo={ui.logo} label={tpl.name} size={24} />

    <span className="min-w-0">
      <span className="block truncate text-[13px] font-semibold
                       leading-[18px] text-ink-1">
        {tpl.name}
      </span>
      <span className="block truncate text-[12px]
                       leading-4 text-ink-3">
        {displayHost}
      </span>
    </span>

    <span aria-hidden="true" className="grid h-8 w-8 place-items-center">
      {existing ? <Pencil size={14} /> : <Plus size={16} />}
    </span>
  </button>

  {tpl.key_url && (
    <button
      type="button"
      onClick={onOpenKeyUrl}
      aria-label={keyLinkLabel}
      className="grid h-8 w-8 shrink-0 place-items-center
                 rounded-md text-ink-3 hover:text-accent
                 focus-visible:outline-none focus-visible:ring-2
                 focus-visible:ring-accent"
    >
      <ExternalLink size={14} />
    </button>
  )}
</li>
```

具体行为：

- **Logo、名称和加号属于同一个大点击区**；外链按钮为同级元素，避免嵌套按钮。
- 行高从至少 `64px` 降到至少 `52px`；列表用 `space-y-1`，不再为每行两个图标按钮画独立边框。
- 第二行始终保留域名。当前“已添加·编辑”会替换域名，建议改由铅笔及其提示表达编辑状态。
- `displayHost` 只显示解析后的 `URL.host`；完整安全地址在聚焦提示或编辑表单查看，实际端点不改。
- [Manager.tsx:2268](src/Manager.tsx:2268) 的网络提示退出名称行，放到域名旁的可聚焦说明入口；长说明不挤占品牌名称。
- tab 容器用 `grid grid-cols-2 gap-1 rounded-lg bg-bg-2 p-1`；按钮高 `32px`，选中态 `bg-bg-1 text-accent`。
- tab 支持左右方向键、Home/End，以及 `role="tablist"`、`aria-selected`、关联面板。状态放在 Manager 顶层，不能在现有条件渲染 IIFE 内新增 Hook。
- 桌面与窄屏各用不同 ID 前缀，避免两份 DOM 出现重复的 tab/panel ID。

远程模板继续来自 [Manager.tsx:501](src/Manager.tsx:501)。无法识别的新来源保留在有明确标题的“其他来源”折叠区，不擅自归为官方，也不丢弃。

**2．Logo：选混合路线，运行时全部离线**

问题不仅是缺素材：

- [Manager.tsx:2259](src/Manager.tsx:2259) 把供应商名称直接传给 `ToolIcon`。
- [ToolIcon.tsx:103](src/components/ToolIcon.tsx:103) 将精确的 `openai` 映射成 **Codex**；“OpenAI 官方”又匹配不到，最终显示字母 O。
- [ToolIcon.tsx:110](src/components/ToolIcon.tsx:110) 将带“智谱”的名称统一归为 GLM，无法区分海外 Z.ai。
- 因此应该新增专用 `ProviderLogo`，不要继续扩展工具名称的模糊匹配。

| 路线 | 覆盖与成本 | 结论 |
|---|---|---|
| a．引入 `@lobehub/icons` React 包 | 本次核到对应品牌条目 **21/24**。支持 tree shaking，但所查源码版本还声明了 `antd-style` 等依赖，以及 `@lobehub/ui`、`antd` 等 peer dependencies。不能把整个 npm 包体积当成最终增量，也不能承诺引入后零成本。 | 本轮不选。若以后采用，必须只静态导入指定品牌，并实测构建及发行物增量。 |
| b．手工收集本地 SVG | 可控制每个素材和体积；缺点是逐家找来源、维护品牌更新。仓库已经有六个相关 SVG 候选，无须重新下载全部。 | 可行，但逐家从零收集效率较低。 |
| c．复用现有＋按需取 Lobe SVG＋补官方素材 | 已有素材核对后复用；缺失品牌从固定版本的 Lobe 静态 SVG 或官方来源补齐，全部提交到仓库。 | **推荐。无新增 React 图标包依赖。** |

依据：[Lobe 图标导出表](https://raw.githubusercontent.com/lobehub/lobe-icons/master/src/icons.ts)、[包依赖声明](https://raw.githubusercontent.com/lobehub/lobe-icons/master/package.json)、[官方静态资源说明](https://github.com/lobehub/lobe-icons)。这是品牌目录核查，尚未安装包测量实际增量。

**24 个模板的键位与素材候选**

下表行号均对应 [providerTemplates.ts](src/lib/providerTemplates.ts:36)。Lobe 名称表示可查到的品牌条目；最终取用前仍需对照品牌当前官方展示，尤其 GLM、MiMo、千帆等产品与公司标志。

| 模板／行号 | 建议 `logo` 键 | 分组 | 素材候选 |
|---|---|---|---|
| OpenAI 官方 :38 | `openai` | 厂商 | 本地 `openai.svg`／Lobe `OpenAI` |
| DeepSeek 官方 :46 | `deepseek` | 厂商 | 本地／`DeepSeek` |
| 智谱 GLM :56 | `zhipu` | 厂商 | 本地／`Zhipu`，复核当前标志 |
| Kimi（月之暗面） :66 | `kimi` | 厂商 | 本地／`Kimi` |
| 小米 MiMo :79 | `xiaomi-mimo` | 厂商 | `XiaomiMiMo`；并非小米橙色集团标志 |
| 阿里百炼（通义） :88 | `bailian` | 厂商 | `Bailian` |
| 火山方舟（豆包） :97 | `volcengine` | 厂商 | `Volcengine` |
| SiliconFlow（硅基流动） :105 | `siliconflow` | 平台 | `SiliconCloud` |
| OpenRouter :115 | `openrouter` | 平台 | `OpenRouter` |
| OpenCode Zen :130 | `opencode` | 平台 | `OpenCode` |
| 腾讯混元 :138 | `hunyuan` | 厂商 | `Hunyuan` |
| 智谱 GLM 海外（z.ai） :148 | `zai` | 厂商 | `ZAI`，单独映射 |
| MiniMax（国内站） :162 | `minimax` | 厂商 | 本地／`Minimax` |
| 百度千帆 ERNIE :171 | `qianfan` | 厂商 | `BaiduCloud`，与千帆官网核对 |
| 阶跃星辰 Stepfun :180 | `stepfun` | 厂商 | `Stepfun` |
| 美团 LongCat :190 | `longcat` | 厂商 | `LongCat` |
| Google Gemini :200 | `gemini` | 厂商 | 本地／`Gemini` |
| xAI Grok :208 | `xai` | 厂商 | `XAI` |
| Mistral :216 | `mistral` | 厂商 | `Mistral` |
| Groq :225 | `groq` | 平台 | `Groq` |
| iFlow 心流 :243 | `iflow` | 平台 | 未找到库内条目；待补官方素材 |
| 魔搭 ModelScope :253 | `modelscope` | 平台 | `ModelScope` |
| B.ai :269 | `bai` | 平台 | 未找到库内条目；待补官方素材 |
| APIMart :281 | `apimart` | 平台 | 未找到库内条目；待补官方素材 |

上述三家本次没有核实到可直接采用的官方 SVG 下载文件，暂保留中性字母兜底，不能解释成“这些品牌没有 Logo”。补素材从其官方页面查找：[iFlow](https://platform.iflow.cn/)、[B.ai](https://b.ai/)、[APIMart](https://apimart.ai/)。

**数据结构建议**

新增 `src/lib/providerPresentation.ts`，维护展示属性；不把 UI 字段写入三份安装模板：

```ts
type ProviderGroup = "vendor" | "platform" | "unknown";

type ProviderPresentation = {
  logo: ProviderLogoKey; // 明确枚举，包括上述 24 个键及 unknown
  group: ProviderGroup;
};

export function resolveProviderPresentation(
  value: Pick<ProviderTemplate, "openai_base" | "anthropic_base">
): ProviderPresentation;
```

实现规则：

- 根据解析后的**准确接口主机名白名单**定位品牌，必要时补路径规则；不靠用户可修改的名称或模型 ID。
- 本地与远程模板经过同一个解析函数，UI 字段不会被远程旧结构覆盖。
- Logo 只允许查本地枚举映射；未知值退回兜底，不接受远程图片 URL。
- OpenCode Zen／Go 可共享 `opencode` 标志，但路径不同的服务仍保留各自模板。
- `ProviderLogo` 在右栏与已保存供应商卡共用；无需给现有用户配置做迁移。

已有六个候选 SVG 合计 **12,567 字节**。建议新增 SVG 总量先按 **100 KiB 未压缩预算**控制，这是验收预算，实际发行物增量待构建测量。

素材放 `src/assets/providers/`；已有文件继续从 `src/assets/logos/` 引用，不复制。每份新增 SVG 记录来源、固定版本和所需许可说明。彩色标志保持原色；单色 SVG 用本地 CSS mask＋`text-ink-1` 适配深浅主题，避免对所有品牌统一反色。

**3．页面长度：先去重复占行，再减间距**

**我的 AI 工具卡**

图1每张卡都反复出现安装状态行、模型行、路径行，以及独立维修横条。可以压缩，但“当前模型”和“实际启动路径”都有排障价值，应保留。

修改 [App.tsx:2246](src/App.tsx:2246) 与 [App.tsx:2375](src/App.tsx:2375)：

```text
图标  工具名称                   [打开终端] […]
      已安装 · 当前模型             升级 / 修复
      实际启动路径……
```

- 卡头 `px-4 py-4` → `p-3`，用 `grid-cols-[36px_minmax(0,1fr)_auto] gap-3`。
- 安装状态与模型合为一行，字号统一 `12px`；“还没配模型”继续显式显示。
- 路径继续单行可见并提供完整路径提示；便携标识保留。
- 打开按钮继续 `h-9`。修复移到其下方，继续提供 `h-8` 点击区，文案缩成“升级 / 修复”。
- 删除原卡底整宽修复横条，仍调用原来的 `onOpen(t)`；维修入口始终可见。
- 移除名称旁“干活最强／越用越懂你”两个营销徽章，为真实工具名腾空间。
- 普通卡目标约 **96–108px**，长文案自然增高；不写死高度裁内容。预计每排节约约 **30–40px**，图1六排约少 **180–240px**，最终以同缩放真机测量为准。
- [App.tsx:2361](src/App.tsx:2361) 的 uu-switch 导入入口仍可见，额外行压到 `h-8`，允许这张卡更高。
- 保留两列，不为了缩短页面把 11 个工具硬塞三列。

**供应商卡**

这里需要先修一个宽度问题：外壳被 [App.tsx:1132](src/App.tsx:1132) 限制为 `1024px`，右栏占 `272px`，但 [Manager.tsx:2405](src/Manager.tsx:2405) 仍按窗口断点强制三列。

按代码估算，主区单卡只有约 **239px**，达不到上一轮规定的 **280px**。仅减高度会让信息更拥挤。

建议：

- 仅 AI 设置的外壳上限改为 `max-w-6xl`，其他页面维持 `max-w-5xl`。
- 主网格改为按实际可用宽度决定列数：

```tsx
className="grid gap-2.5
  grid-cols-[repeat(auto-fill,minmax(min(100%,280px),1fr))]"
```

- 卡片 `p-3.5` → `p-3`。
- Logo＋名称＋域名组成紧凑卡头，域名放在名称下方；模型仍单独一行。
- 协议标签继续保留，压为 `h-5`；测速仍保留至少 `32px` 操作区。
- [Manager.tsx:2457](src/Manager.tsx:2457) 去掉左侧孤立的“延迟”标签，让结果、重测、原因顺序靠近；空间不足时明确换行。
- [Manager.tsx:2464](src/Manager.tsx:2464) 引用行去掉分隔线和额外 `pt-2`，改成 `mt-1 text-[12px] text-ink-2`，文案“用于：Cline · OpenCode”。工具名允许换行。
- 普通卡目标 **160–180px**；错误详情展开或工具引用较多时自然增高。

不把错误状态藏起来换取紧凑。复用 [Manager.tsx:1077](src/Manager.tsx:1077) 的同一个测速渲染函数，避免写第二套状态逻辑。

**4．最伤观感与直观性的三个细节**

| 排名 | 截图及代码证据 | 一句话修法 |
|---|---|---|
| 1 | 图4的 OpenRouter、OpenCode 名称被挤成“Ope…”，旁边网络提示却完整保留；加号、Logo、两侧按钮共同挤压文字。对应 `Manager.tsx:2240、2261、2268`。 | 将来源按两个 tab 切换，名称独占首行，把整行主体变成添加／编辑点击区。 |
| 2 | 图2/3能凭品牌标志迅速辨认；图4多个来源都是相似的 O/B 字母块，而且源码存在 OpenAI→Codex 的误映射。对应 `Manager.tsx:2259`、`ToolIcon.tsx:103`。 | 供应商统一使用按接口品牌解析的 `ProviderLogo`，与工具图标分开。 |
| 3 | 图4“查看原因”、模型和使用情况过淡过小；失败状态横向堆在狭窄卡内，阅读和点击都费劲。对应 `Manager.tsx:1130、1137、2457、2464`。 | 说明与操作至少 12px，用 `ink-2/3`；失败用 `danger-700 dark:text-danger-400`，重测与原因保持明确点击区。 |

这是对截图可见布局的比较，不据截图推断 EchoBird 未展示的实际点击行为。

**5．按小步交付拆分，每条最多两个文件**

新增文件没有现有行号，明确标为“新增”。SVG 补齐按品牌逐条执行，不能合并成“一次改全部素材”。

| 顺序 | 文件范围 | 完成标准 |
|---|---|---|
| ① 展示元数据 | 新增 `src/lib/providerPresentation.ts` | 24 个模板都有 Logo 键和分组；远程未知模板有兜底；不修改业务模板。 |
| ② 两 tab 与目录去重 | `src/Manager.tsx:2227`＋`src/i18n/en/settings.ts` | 两组切换、动态数量、前 8 家展开；免费入口可达；同一模板不重复出现。 |
| ③ 紧凑列表行 | `src/Manager.tsx:2235`＋`src/i18n/en/settings.ts` | 52px 基准行、完整品牌名、大点击区、独立外链、键盘可用。 |
| ④ 专用 Logo 组件 | 新增 `src/components/ProviderLogo.tsx` | 先复用已有六份候选素材，其余明确兜底；不改 `ToolIcon`。 |
| ⑤ 逐品牌补素材 | 每次一个 `src/assets/providers/<logo>.svg`＋`src/components/ProviderLogo.tsx` | 核对官方外观、来源记录、深浅主题及离线显示；每个品牌独立完成。 |
| ⑥ 两处接入 Logo | `src/Manager.tsx:2259、2420` | 右栏和已保存卡片共享同一品牌解析；用户改名后图标仍正确。 |
| ⑦ 修正有效卡宽 | `src/App.tsx:1132`＋`src/Manager.tsx:2405` | 宽屏可容纳三张 ≥280px 卡片；窄屏自然减列，无横向溢出。 |
| ⑧ 供应商卡压缩 | `src/Manager.tsx:2417`＋`src/i18n/en/settings.ts` | 普通卡约 160–180px；模型、协议、测速、引用信息全部可达。 |
| ⑨ 工具卡压缩 | `src/App.tsx:2246、2375`＋`src/i18n/en/app.ts` | 修复仍常显，路径和模型保留；普通卡约 96–108px。 |
| ⑩ 恢复入口收纳 | `src/Manager.tsx:2332`＋`src/i18n/en/settings.ts` | “加回工具”独立折叠；文案显示实际目标工具。当前 `{ tool: label }` 填的是供应商名，应使用 `TOOL_LABELS[activeTab]`。 |

每步完成后做对应界面检查；整轮落地后运行 `pnpm build`、`pnpm run action-parity:verify`。进入提交／推送流程时继续遵守仓库既有泄漏和 Rust 测试闸门。

真机验收覆盖浅色／深色、中英文、宽窄窗口、150% 缩放、断网，以及未添加／已添加／测速失败／未知远程模板。高度数字是目标，不作为本轮已实测结果。

**保持原样**

- **钱包置顶、五个设置分区、左主区＋右栏、窄屏折叠结构**保留；不复制 EchoBird 的整套黑色皮肤。
- **添加、验证、保存、工具分配流程**保留；点击模板不自动启用模型。
- **24 家业务模板的端点、模型、Key 信息和热下发机制**保留；本轮不上架截图中的新服务。
- **图4里重复命名的 DeepSeek、B.ai 已保存卡片不自动合并或删除**，它们可能代表不同账号、模型或已有引用；去重只针对右栏重复展示。
- **实际启动路径、当前模型、修复入口、卸载确认、uu-switch 导入能力**保留。
- **安装成功、已保存、已分配、测速成功之间的区别**保留，不统一画成“可用”。
