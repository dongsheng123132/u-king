/**
 * Windows xterm 残字校正器回归：输出停下能自愈、持续输出也不会永久残、销毁后不误刷。
 * 用法：node scripts/check-term-paint-repair.mjs
 */
import { readFileSync } from "node:fs";
import { transformWithEsbuild } from "vite";

const source = readFileSync("src/opencodex/term/paintRepair.ts", "utf8");
const built = await transformWithEsbuild(source, "paintRepair.ts", { loader: "ts", format: "esm" });
const mod = await import(`data:text/javascript;base64,${Buffer.from(built.code).toString("base64")}`);
const { createPaintRepair } = mod;

function clock() {
  let now = 0;
  let seq = 0;
  const jobs = new Map();
  return {
    setTimer(fn, ms) {
      const id = ++seq;
      jobs.set(id, { at: now + ms, fn });
      return id;
    },
    clearTimer(id) {
      jobs.delete(id);
    },
    advance(ms) {
      const end = now + ms;
      while (true) {
        const next = [...jobs.entries()].sort((a, b) => a[1].at - b[1].at || a[0] - b[0])[0];
        if (!next || next[1].at > end) break;
        now = next[1].at;
        jobs.delete(next[0]);
        next[1].fn();
      }
      now = end;
    },
  };
}

const fails = [];

console.log("[1/3] 一阵输出只做一次校正，且不必靠拖宽度触发…");
{
  const c = clock();
  let refreshes = 0;
  const r = createPaintRepair({ refresh: () => refreshes++, quietMs: 80, maxWaitMs: 400, ...c });
  r.afterWrite();
  c.advance(40);
  r.afterWrite();
  c.advance(79);
  if (refreshes !== 0) fails.push(`输出尚未静止就刷新了 ${refreshes} 次`);
  c.advance(1);
  if (refreshes !== 1) fails.push(`输出静止后刷新 ${refreshes} 次（应恰好 1 次）`);
  else console.log("     ✓ 两块输出合并为一次全屏校正");
}

console.log("[2/3] 持续流式输出也必须在最长等待时间内自愈…");
{
  const c = clock();
  let refreshes = 0;
  const r = createPaintRepair({ refresh: () => refreshes++, quietMs: 80, maxWaitMs: 400, ...c });
  for (let i = 0; i < 8; i++) {
    r.afterWrite();
    c.advance(50);
  }
  if (refreshes !== 1) fails.push(`连续 400ms 输出期间刷新 ${refreshes} 次（应由硬上限触发 1 次）`);
  else console.log("     ✓ 连续输出第 400ms 自动校正一次");
}

console.log("[3/3] 终端销毁后，遗留计时器不能碰已 dispose 的 xterm…");
{
  const c = clock();
  let refreshes = 0;
  const r = createPaintRepair({ refresh: () => refreshes++, quietMs: 80, maxWaitMs: 400, ...c });
  r.afterWrite();
  r.close();
  c.advance(500);
  if (refreshes !== 0) fails.push(`终端关闭后仍刷新了 ${refreshes} 次`);
  else console.log("     ✓ 关闭后 0 次刷新");
}

if (fails.length) {
  console.error(`\n❌ ${fails.length} 条不达标：`);
  for (const f of fails) console.error(`  - ${f}`);
  process.exit(1);
}
console.log("\n✅ 残字校正：静止自愈、持续输出有上限、销毁不误刷");
