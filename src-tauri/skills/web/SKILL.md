---
name: uking-web
description: 用单文件 HTML + Tailwind CDN 快速做网站/落地页/H5/产品官网的建站法，成品可直接预览。给网站设计专家用。
---

# U-King 单文件建站法（uking-web）

## 何时用
用户要做网站、落地页、产品官网、H5、个人主页/作品集等，需要能直接看效果的成品。

## 核心原则
1. **用一次 write_file 写出完整 `index.html`**（内联 Tailwind CDN + 全部内容 + 内联 style/script），别拆多文件——避免超工具步数，也方便预览。
2. 写完提示用户点顶栏「预览网页」在右侧看效果，再按反馈迭代。
3. hero/配图用 generate_image 生成。

## 页面骨架（Tailwind CDN）
- `<head>` 引 `<script src="https://cdn.tailwindcss.com"></script>` + 中文字体。
- 常见区块：顶部导航 → hero（大标题+副标题+CTA+配图）→ 卖点卡片 → 案例/数据 → 定价 → 页脚。
- 响应式移动优先（`md:` 断点）；现代感：圆角、柔和阴影、留白、主色渐变。

## 何时升级
真要多文件工程 / 框架（React/Vue）/ 复杂交互 → 建议用户在大脑选择器切到 Claude Code。
