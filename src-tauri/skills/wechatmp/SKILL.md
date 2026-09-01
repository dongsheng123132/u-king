---
name: wechat-mp
description: 把 Markdown 排版成公众号可直接粘贴的内联样式 HTML。三个主题（default/grace/simple），不联网、不上传、不需要 AppID/Secret。写完公众号文章要发之前用它。
---

# 公众号排版

```bash
node ~/.uking/skills/wechat-mp/scripts/render-mp.mjs 文章.md --theme grace --out 文章.html
```

出 `{"ok":true,"out":"...","theme":"grace","bytes":1350,"inline_css":true}`。

**公众号编辑器只认内联样式**，所以 `inline_css` 必须是 true——不是就别往里粘。
主题：`default`（稳）· `grace`（雅致）· `simple`（极简）。

**这一步只排版。** 上传草稿、群发都不在这条路里——发布是不可逆动作，留给人在公众号后台点。
