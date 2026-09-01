# vendored: redline-core

这是 **redline-core**（宿主无关的「万能文档预览」内核，Apache-2.0）的**源码快照**，
vendored 进 U-King 以便自包含构建 + CI 无需外部依赖（Mac GitHub Actions 只 checkout 自己）。

## 🔴 这份副本已经和上游分叉了（2026-08-17，U-King 1.0.3）

**别再无脑 `cp -r` 覆盖它 —— 那会把删掉的标注子系统整个搬回来，并当场编译失败。**

本副本相对上游做了三处**有意**的改动：

| 改动 | 涉及文件 | 为什么 |
|---|---|---|
| **整块删掉 `annotation/`**（AnnotationLayer + useAnnotations，425 行） | 目录已删 | 它导出给 AI 的锚点是假的：`formatAnnotationsAsText` 里那句「该处内容」附的是**全文前 200 字**，跟框画在哪毫无关系；坐标是 0~1 分数。产品决定移除（用户 2026-08-17：「体验还不好…删除功能」） |
| `renderOverlay` 从 `ViewerProps` **必填字段**里摘掉；`annotationStore` / `sendToAgent` / `annotationStoreKey` 从 `RedlineHost` 摘掉；`RedlineUnit.renderSize` 删掉 | `viewers/types.ts`、`host-adapter.ts`、`document-model.ts`、11 个 viewer | 上面那条的连带。这些字段**只**为标注导出服务，留着等于让 11 个 viewer 永远替一个不存在的功能维护尺寸测量代码 |
| **新增 `viewers/MdViewer.tsx`** + `markdown` / `csv` 两条格式映射 + `RedlineHost.renderMarkdown?` | `viewers/MdViewer.tsx`、`viewers/registry.ts`、`host-adapter.ts` | `.md` 以前落到 TextViewer 直出源码。渲染器**由宿主注入**（U-King 注入 `lib/miniMd.tsx` 的 MiniMd），内核不自带 —— 不涨体积、全站只有一套 md 语法口径 |

**要同步上游时**：不能整目录覆盖，得逐文件看 diff、把上述三处改动重新应用一遍。
更好的做法是**把这三处改动推回上游**（`~/Desktop/claude/u-claw/redline/packages/redline-core`），
让两边重新收敛 —— 分叉每多活一天，下一次同步就更贵。**这件事还没做。**

## 常规说明

- 上游真身：`~/Desktop/claude/u-claw/redline/packages/redline-core`（独立仓库，另一个宿主 opencodex 走 `link:` 引用它）
- 除上表那三处外，**仍按只读副本对待**：要改内核逻辑请改上游，再同步过来。
- 待办：redline 建 GitHub 私有远端后，可迁成 git submodule 去掉本副本（那样彻底消除漂移；
  但**得先把上面那个分叉推回上游**，否则 submodule 一挂就把标注带回来了）。

接入点：`src/opencodex/redline-host-tauri.tsx`（Tauri 宿主适配器）+
`src/opencodex/panels/FilesPanel.tsx`（挂 `<RedlinePanel>`，树 + 就地预览）。
