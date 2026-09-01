/**
 * 让步链状态机跑道 —— `src/lib/yieldChain.ts` 的纯逻辑，无 DOM。
 *
 *     pnpm check:yield
 *
 * ## 为什么值得单开一条
 *
 * 这个状态机唯一容易写错的地方是「升级看实测宽度、降级看窗口宽度」这条不对称
 * （理由见 yieldChain.ts 的文件头）。写错的表现不是报错，是**布局在临界宽度上每帧横跳** ——
 * 编译过、类型对、conformance 全绿，只有人眼看得见。第 4 组用例就是钉这一条的：
 * 降级判据一旦改回看实测宽度，那 20 帧立刻抖起来。
 *
 * ## 🔴 它没挂进 pnpm build
 *
 * 本仓没有前端测试框架（CLAUDE.md 明写），跑 .ts 要么加 tsx 这个 devDependency、
 * 要么在 build 里塞一条要联网的 `npx` —— 两样都比这条跑道本身贵。
 * 所以它是**手动跑的**，改 yieldChain.ts 时记得跑一次。
 * 按 CLAUDE.md 那句「一个不阻断构建的预算不是预算，是一句抱怨」，这是一笔明账上的欠款：
 * 哪天前端真有了测试框架，第一个搬进去的就是它。
 */
globalThis.window = { innerWidth: 900 } as any;

const {
  yieldLevel, reportTermWidth, releaseYield, overrideYield, resetYield,
  TERM_FLOOR_PX, RELEASE_MARGIN_PX,
} = await import("../src/lib/yieldChain.ts");

let fail = 0;
const eq = (name: string, got: unknown, want: unknown) => {
  const ok = got === want;
  if (!ok) fail++;
  console.log(`${ok ? "  ok  " : "  FAIL"}  ${name}  got=${got} want=${want}`);
};
const win = (w: number) => { (globalThis.window as any).innerWidth = w; };

console.log(`\nTERM_FLOOR_PX=${TERM_FLOOR_PX}  RELEASE_MARGIN_PX=${RELEASE_MARGIN_PX}\n`);

// ── 1. 饿了就一级级升，最多到 2 ────────────────────────────────
console.log("1. 升级");
resetYield(); win(900);
eq("初始不让步", yieldLevel(), 0);
reportTermWidth("A", 197);              // 实测那个数
eq("197px → level 1", yieldLevel(), 1);
reportTermWidth("A", 343);              // 会话栏收窄后拿回 ~146px，仍不够 480
eq("343px → level 2", yieldLevel(), 2);
reportTermWidth("A", 399);              // 主侧栏也收了，还是不够（900 的窗口就这么大）
eq("封顶在 2", yieldLevel(), 2);

// ── 2. 隐藏的会话报 0 不算饥饿 ──────────────────────────────────
console.log("\n2. 隐藏会话报 0");
resetYield(); win(900);
reportTermWidth("A", 197);
eq("A 是委托人，level 1", yieldLevel(), 1);
reportTermWidth("B", 0);                // B 藏着
eq("别人报 0 不影响", yieldLevel(), 1);
reportTermWidth("A", 0);                // 委托人被切走
eq("委托人报 0 → 复位", yieldLevel(), 0);

// ── 3. 降级只看窗口宽度，且要够余量 ────────────────────────────
console.log("\n3. 降级");
resetYield(); win(900);
reportTermWidth("A", 197);
eq("level 1", yieldLevel(), 1);
win(900 + RELEASE_MARGIN_PX);           // 正好等于余量，不够（要求严格大于）
reportTermWidth("A", 500);
eq("刚好卡在余量上不降", yieldLevel(), 1);
win(900 + RELEASE_MARGIN_PX + 1);
reportTermWidth("A", 520);
eq("超过余量 → 降回 0", yieldLevel(), 0);

// ── 4. 🔴 不横跳：宽度在 floor 上下抖，窗口没动就不该来回 ──────
console.log("\n4. 不横跳（回归：降级若看实测宽度，这里会一帧一个来回）");
resetYield(); win(1000);
reportTermWidth("A", 400);              // 饿 → 升
eq("升到 1", yieldLevel(), 1);
let flips = 0, last = yieldLevel();
for (let i = 0; i < 20; i++) {
  // 让步之后面板变宽了（546 > floor），但窗口一动没动
  reportTermWidth("A", 546);
  if (yieldLevel() !== last) { flips++; last = yieldLevel(); }
}
eq("窗口没变宽 → 20 帧零抖动", flips, 0);

// ── 5. 用户手动推翻 ────────────────────────────────────────────
console.log("\n5. 用户推翻");
resetYield(); win(900);
reportTermWidth("A", 197);
eq("先让步", yieldLevel(), 1);
overrideYield();
eq("推翻后归 0", yieldLevel(), 0);
reportTermWidth("A", 197);
eq("推翻后再饿也不让", yieldLevel(), 0);
releaseYield("A");                      // 关掉终端 = 复位，override 一并清掉
reportTermWidth("A", 197);
eq("重开终端是新处境", yieldLevel(), 1);

// ── 6. 换会话重算 ──────────────────────────────────────────────
console.log("\n6. 换会话");
resetYield(); win(900);
reportTermWidth("A", 197);
overrideYield();
eq("A 推翻了", yieldLevel(), 0);
reportTermWidth("B", 197);              // 切到 B，B 也开着终端
eq("B 接手 → 重新算，让步", yieldLevel(), 1);
releaseYield("A");                      // 🔴 非委托人喊停不作数
eq("A 关自己的终端不影响 B", yieldLevel(), 1);

console.log(fail === 0 ? "\n全部通过\n" : `\n${fail} 条失败\n`);
process.exit(fail === 0 ? 0 : 1);
