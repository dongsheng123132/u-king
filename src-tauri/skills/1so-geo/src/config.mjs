// 一搜商答 / 1so —— 项目路径约定。
// 一个“项目” = 一个商家的工作目录：materials/ 放原始资料，.1so/ 放中间产物，site/ 放生成的网页。
import { join, resolve } from "node:path";

export function projectPaths(project) {
  const root = resolve(project || ".");
  return {
    root,
    materials: join(root, "materials"),
    work: join(root, ".1so"),
    cards: join(root, ".1so", "cards.json"),
    detect: join(root, ".1so", "detect.json"),
    scan: join(root, ".1so", "scan.json"),
    aicheck: join(root, ".1so", "aicheck.json"),
    aicheckReport: join(root, ".1so", "报告-各大模型眼里的你.md"),
    report: join(root, ".1so", "报告-AI眼里的你.md"),
    optimize: join(root, ".1so", "优化建议.md"),
    site: join(root, "site"),
    page: join(root, "site", "index.html"),
    panel: join(root, "site", "体检面板.html"),
    aicheckPage: join(root, "site", "AI可见度测试报告.html"),
    llms: join(root, "site", "llms.txt"),
  };
}
