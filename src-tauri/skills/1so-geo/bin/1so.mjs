#!/usr/bin/env node
// 一搜商答 / 1so —— 入口。
import { main } from "../src/cli.mjs";
main(process.argv.slice(2)).catch((e) => {
  process.stderr.write("致命错误：" + (e && e.stack || e) + "\n");
  process.exit(1);
});
