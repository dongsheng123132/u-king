# u-king.org → u-claw.org.cn/uking/ 整站跳转（Vercel）

U-King 官网的唯一真相源是 `website/`，由 `scripts/release-uking.mjs` 发布到已备案域名
`https://u-claw.org.cn/uking/`（国内可达，客户端主地址也是它）。

`u-king.org` / `www.u-king.org` 未在国内备案，不能放在国内服务器上
（80 端口会被云厂商重定向到备案拦截页、443 部分网络按 SNI 断开握手，证书也续不了）。
所以 `.org` 只放在 Vercel 上做一件事：**把任意路径 308 跳到 `.cn` 镜像的同一路径**，
证书由 Vercel 自动管理。内容只有一份，发版不需要再同步这里。

- 本目录只有 `vercel.json`（不要往里加注释字段：Vercel 会做严格 schema 校验，构建直接失败）。
- 部署：在本目录执行 `vercel deploy --prod`（项目名 `u-king-org`，域名 `u-king.org`、`www.u-king.org`）。
- 客户端里 `www.u-king.org/...` 只是备用地址；跳转后照常拿到 `.cn` 上的同一文件，
  `irm https://www.u-king.org/dl.ps1 | iex` 也会跟随跳转。
