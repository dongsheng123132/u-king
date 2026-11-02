# vendor/ —— 锁死来源的第三方制品

这个目录只放**无法从 npm registry 正常拿到**的第三方制品：上游停止在 npm 发布、修复版只在
自有 CDN 上架的包。每个制品都必须在这里登记来源与校验和，`package.json` 用 `file:` 协议引用，
这样 `pnpm-lock.yaml` 会把 tarball 的 integrity 一起锁进锁文件。

---

## xlsx-0.20.3.tgz

| 项 | 值 |
| --- | --- |
| 包名 / 版本 | `xlsx` 0.20.3（SheetJS Community Edition） |
| 来源 URL | <https://cdn.sheetjs.com/xlsx-0.20.3/xlsx-0.20.3.tgz> |
| 体积 | 2 409 319 字节（2.30 MiB） |
| sha256 | `8dc73fc3b00203e72d176e85b50938627c7b086e607c682e8d3c02c02bb99fe8` |
| lockfile integrity | `sha512-oLDq3jw7AcLqKWH2AhCpVTZl8mf6X2YReP+Neh0SJUzV/BdZYjth94tG5toiMB1PPrYtxOCfaoUCkvtuH+3AJA==` |
| 用途 | 替换 npm 上停更的 `xlsx@0.18.5`，关闭 CVE-2023-30533（原型链污染） |

为什么不是 `pnpm add xlsx@0.20.3`：npm registry 上的 `xlsx` 最后一次发布就是 0.18.5（2023-03），
之后上游把发布渠道整体搬到了自己的 CDN，npm 上永远收不到 0.19.3+。所以只能把 tarball 落进仓库。

### 校验和复现

```bash
curl -fsSL -o /tmp/xlsx-0.20.3.tgz https://cdn.sheetjs.com/xlsx-0.20.3/xlsx-0.20.3.tgz
shasum -a 256 /tmp/xlsx-0.20.3.tgz   # 8dc73fc3b00203e72d176e85b50938627c7b086e607c682e8d3c02c02bb99fe8
```

### 升级步骤

1. 从 <https://cdn.sheetjs.com/> 取新版本 tarball，落盘到本目录（文件名 `xlsx-<版本>.tgz`）。
2. `shasum -a 256` 核对，更新本文件的来源/体积/校验和三行。
3. 改 `package.json` 的 `"xlsx": "file:vendor/xlsx-<版本>.tgz"`。
4. **用 pnpm 重装**（`pnpm install`），确认 `pnpm-lock.yaml` 里的 `xlsx@file:...` 段跟着变。
5. `node scripts/poc-cve-2023-30533.mjs` 必须绿；`pnpm build` 必须过。
6. 旧 tarball 不要留在目录里，仓库只保留当前在用的一份。
