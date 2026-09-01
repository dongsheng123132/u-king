import { createHash } from "node:crypto";
import { Node } from "../src/vendor/2origin-kernel/kernel.mjs";

const seed = {
  project: "U-King 融资计划",
  resources: ["商业计划书.docx@rev_020", "财务预测.xlsx@rev_005", "外壳设计.dwg@rev_017", "git-u-king-desktop@commit_a3f91c7"],
};

let clockMs = Date.parse("2026-08-28T09:00:00.000Z");
const now = () => ++clockMs;
const owner = new Node("team-space:ws-hequbing", now);
const zhangSan = new Node("张三", now);
const liSi = new Node("李四", now);
const aiDeveloper = new Node("AI开发", now);
const heFangsheng = new Node("贺方升", now);
const activities = [];
const CHAIN_SEED = "team-space.activity.v1:GENESIS";

function canonical(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value ?? null);
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
}
function sha256(value) { return createHash("sha256").update(value).digest("hex"); }
function activityPayload(activity) {
  return JSON.stringify({ actor: activity.actor, action: activity.action, resource_id: activity.resource_id, revision_id: activity.revision_id, prev_hash: activity.prev_hash });
}
function appendActivity(actor, action, event, resourceId = "res-shell-dwg", revisionId = "rev_017") {
  const prev_hash = activities.at(-1)?.receipt_hash ?? CHAIN_SEED;
  const activity = { actor, action, resource_id: resourceId, revision_id: revisionId, prev_hash, receipt_hash: "", event };
  activity.receipt_hash = sha256(activityPayload(activity));
  activities.push(activity);
  return activity;
}
function compareHlc(a, b) {
  if (a.phy !== b.phy) return a.phy - b.phy;
  if (a.l !== b.l) return a.l - b.l;
  return a.node.localeCompare(b.node);
}
function verifyActivityHashChain(items) {
  let previous = CHAIN_SEED;
  let previousTs = null;
  const receiptHashes = new Set();
  const actorHeads = new Map();
  for (const activity of items) {
    const event = activity.event;
    if (!activity.receipt_hash || receiptHashes.has(activity.receipt_hash)) return { ok: false, reason: "receipt_hash 为空或重复" };
    if (activity.prev_hash !== previous) return { ok: false, reason: "prev_hash 不连续" };
    if (activity.receipt_hash !== sha256(activityPayload(activity))) return { ok: false, reason: "receipt_hash 与活动文本不匹配" };
    if (previousTs && compareHlc(previousTs, event.ts) > 0) return { ok: false, reason: "HLC 非单调" };
    const { hash, sig, ...core } = event;
    if (hash !== sha256(canonical(core))) return { ok: false, reason: "kernel event hash 不匹配" };
    if (event.prevHash !== (actorHeads.get(event.actor) ?? "GENESIS")) return { ok: false, reason: `kernel actor 链断裂: ${activity.action}` };
    actorHeads.set(event.actor, hash);
    receiptHashes.add(activity.receipt_hash); previous = activity.receipt_hash; previousTs = event.ts;
  }
  return { ok: true, checked: items.length };
}
function line(action, hash, event) { return `${event.ts.phy}:${event.ts.l}:${event.ts.node} | ${action} | ${hash}`; }

// ① 张三正常签出、心跳、签入。
const lease = owner.grant({ to: "张三", caps: ["resource.checkout", "resource.heartbeat"], ttlMs: 7_200_000, resource: "res-shell-dwg" });
zhangSan.receive(lease);
const heartbeat = zhangSan.exercise({ leaseId: lease.leaseId, action: "resource.heartbeat", idem: "smoke-heartbeat-1" });
const heartbeatReceipt = owner.ackExercise(heartbeat);
const checkin = owner.revoke({ leaseId: lease.leaseId });
appendActivity("张三", "签出 CAD", lease);
appendActivity("张三", "续租心跳", heartbeatReceipt);
appendActivity("张三", "签入 CAD", checkin);

// ② D4：超短 TTL 到期后由下一位成员接管；这是 local-provider tryAcquireExclusiveLock 的现场语义。
const expiredLease = owner.grant({ to: "张三", caps: ["resource.checkout", "resource.heartbeat"], ttlMs: 1, resource: "res-shell-dwg" });
const expiredAt = expiredLease.notAfter.phy;
const observedAt = expiredAt + 1;
appendActivity("张三", "签出 CAD（短租约）", expiredLease);
console.log(`lease takeover: 张三签出 CAD，expires_at=${new Date(expiredAt).toISOString()}`);
console.log(`lease takeover: ${new Date(observedAt).toISOString()} 发现张三的锁已过期`);
const expiredRelease = owner.revoke({ leaseId: expiredLease.leaseId });
appendActivity("系统", "租约过期自动释放", expiredRelease);
console.log(`lease takeover: 自动释放 lease_token=${expiredLease.leaseId}`);
const takeoverLease = owner.grant({ to: "李四", caps: ["resource.checkout", "resource.heartbeat"], ttlMs: 7_200_000, resource: "res-shell-dwg" });
liSi.receive(takeoverLease);
appendActivity("李四", "签出 CAD（过期接管）", takeoverLease);
console.log(`lease takeover: 李四签出成功 lease_token=${takeoverLease.leaseId}`);

// ②b：李四的超短租约再次过期后，AI 开发成员接管；证明 AI 和真人走同一 holder/Lease 语义。
const aiExpiredLease = owner.grant({ to: "李四", caps: ["resource.checkout", "resource.heartbeat"], ttlMs: 1, resource: "res-shell-dwg" });
const aiObservedAt = aiExpiredLease.notAfter.phy + 1;
appendActivity("李四", "签出 CAD（AI 接管前短租约）", aiExpiredLease);
console.log(`lease takeover: 李四短租约已于 ${new Date(aiObservedAt).toISOString()} 过期`);
const aiExpiredRelease = owner.revoke({ leaseId: aiExpiredLease.leaseId });
appendActivity("系统", "租约过期自动释放（AI 接管）", aiExpiredRelease);
const aiTakeoverLease = owner.grant({ to: "AI开发", caps: ["resource.checkout", "resource.heartbeat"], ttlMs: 7_200_000, resource: "res-shell-dwg" });
aiDeveloper.receive(aiTakeoverLease);
appendActivity("AI开发", "接管 CAD（过期接管）", aiTakeoverLease);
console.log(`lease takeover: AI开发接管成功 lease_token=${aiTakeoverLease.leaseId} (AI member holds lock)`);

// ③ 人工审批的 Receipt。
const approvalLease = owner.grant({ to: "贺方升", caps: ["approval.decide"], ttlMs: 60_000, resource: "approval-plan-ai-001" });
heFangsheng.receive(approvalLease);
appendActivity("系统", "签发审批 Lease", approvalLease, "res-plan", "rev_021_draft");
const approve = heFangsheng.exercise({ leaseId: approvalLease.leaseId, action: "approval.decide", idem: "approve:approval-plan-ai-001" });
const approvalReceipt = owner.ackExercise(approve);
appendActivity("贺方升", "批准 AI 草稿", approvalReceipt, "res-plan", "rev_021_draft");

console.log(`seed: ${seed.project}`);
console.log(`resources: ${seed.resources.join(", ")}`);
console.log(`checkout: lease_token=${lease.leaseId} base_revision=rev_017 expires=${new Date(lease.notAfter.phy).toISOString()}`);
console.log(line("heartbeat", heartbeatReceipt.hash, heartbeatReceipt));
console.log(line("checkin", checkin.hash, checkin));
console.log(`approval: approved receipt_hash=${approvalReceipt.hash}`);
console.log("activity:");
for (const activity of activities) console.log(line(`${activity.actor} ${activity.action}`, activity.receipt_hash, activity.event));

const chain = verifyActivityHashChain(activities);
console.log(`chain: ${chain.ok ? "OK" : "BROKEN"} (${chain.ok ? `${chain.checked} events` : chain.reason})`);
const tampered = activities.map((activity) => ({ ...activity }));
tampered.at(-1).action = "篡改：越权批准 AI 草稿";
const broken = verifyActivityHashChain(tampered);
console.log(`chain: ${broken.ok ? "OK" : "BROKEN"} (${broken.reason})`);
if (!chain.ok || broken.ok) process.exitCode = 1;
