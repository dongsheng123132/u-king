import { recordOtelSpan } from "../otel/tracer";

type KernelEvent = { leaseId?: string; hash?: string; ts?: { phy: number; l: number; node: string } };
type KernelNode = {
  grant(input: { to: string; caps: string[]; ttlMs: number; resource: string }): KernelEvent;
  revoke(input: { leaseId: string }): KernelEvent;
  exercise(input: { leaseId: string; action: string; idem: string }): KernelEvent;
  ackExercise(event: KernelEvent): KernelEvent;
  receive(event: KernelEvent): KernelNode;
  hlc: { tick(): { phy: number; l: number; node: string } };
};

export type LeaseResult = { lease_token: string; expires_at: string; heartbeat_at: string; hlc: string; kernel_available: boolean };
export type ReceiptResult = { receipt_hash: string; hlc: string; kernel_available: boolean };

function isNodeRuntime() { return Boolean((globalThis as { process?: { versions?: { node?: string } } }).process?.versions?.node); }
function hlc(ts: { phy: number; l: number; node: string }) { return `${ts.phy}:${ts.l}:${ts.node}`; }
function localHlc() { return `${Date.now()}:0:webview`; }
// 用拆开的路径避免 Vite 将 Node-only 内核静态纳入 WebView bundle；Node 自动化会解析为 vendor 快照的 file URL。
const vendorKernelSpecifier = new URL(["..", "..", "vendor", "2origin-kernel", "kernel.mjs"].join("/"), import.meta.url).href;
async function browserHash(value: string) {
  const bytes = new TextEncoder().encode(value);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}

/**
 * kernel 是 Node-only（node:crypto），不能直接进 Tauri WebView bundle。
 * Node 冒烟/自动化时此桥接会加载仓库内 vendor 的实际 2origin kernel；客户端运行时保留同一对象形状，
 * 以 kernel-unavailable 记录可审计降级，绝不因观测/签名基础设施缺席阻断本地演示。
 */
export class TeamKernelBridge {
  private nodePromise: Promise<KernelNode | null> | null = null;
  private readonly leases = new Map<string, KernelEvent>();
  private readonly leaseWorkers = new Map<string, KernelNode>();
  private async node(workspaceId: string): Promise<KernelNode | null> {
    if (!isNodeRuntime()) return null;
    if (!this.nodePromise) {
      this.nodePromise = (async () => {
        try {
          const load = Function("s", "return import(s)") as (specifier: string) => Promise<{ Node: new (id: string) => KernelNode }>;
          const kernel = await load(vendorKernelSpecifier);
          return new kernel.Node(`team-space:${workspaceId}`);
        } catch (error) { console.warn("[team-space] 2origin kernel unavailable", error); return null; }
      })();
    }
    return this.nodePromise;
  }
  private trace(name: string, attributes: Record<string, string | boolean>) { void recordOtelSpan(name, { "origin.protocol": "2origin", ...attributes }); }

  async issueLease(input: { workspace_id: string; resource_id: string; holder_id: string; ttl_ms: number }): Promise<LeaseResult> {
    const now = new Date();
    const node = await this.node(input.workspace_id);
    if (!node) {
      const token = `kernel-unavailable-${await browserHash(`${input.resource_id}:${now.toISOString()}`)}`;
      const result = { lease_token: token, expires_at: new Date(now.getTime() + input.ttl_ms).toISOString(), heartbeat_at: now.toISOString(), hlc: localHlc(), kernel_available: false };
      this.trace("origin.team.resource.checkout", { "origin.resource.id": input.resource_id, "origin.holder.id": input.holder_id, "origin.lease.token": token, "origin.hlc": result.hlc, "origin.kernel.available": false });
      return result;
    }
    const grant = node.grant({ to: input.holder_id, caps: ["resource.checkout", "resource.heartbeat"], ttlMs: input.ttl_ms, resource: input.resource_id });
    this.leases.set(grant.leaseId!, grant);
    const Worker = node.constructor as unknown as new (id: string) => KernelNode;
    const worker = new Worker(input.holder_id);
    worker.receive(grant);
    this.leaseWorkers.set(grant.leaseId!, worker);
    const result = { lease_token: grant.leaseId!, expires_at: new Date(now.getTime() + input.ttl_ms).toISOString(), heartbeat_at: now.toISOString(), hlc: hlc(grant.ts!), kernel_available: true };
    this.trace("origin.team.resource.checkout", { "origin.resource.id": input.resource_id, "origin.holder.id": input.holder_id, "origin.lease.token": result.lease_token, "origin.hlc": result.hlc, "origin.kernel.available": true });
    return result;
  }
  async releaseLease(input: { workspace_id: string; resource_id: string; lease_token: string; holder_id: string }): Promise<ReceiptResult> {
    const node = await this.node(input.workspace_id);
    if (!node || !this.leases.has(input.lease_token)) return this.unavailable("origin.team.resource.checkin", input.resource_id, input.holder_id);
    const event = node.revoke({ leaseId: input.lease_token });
    this.leases.delete(input.lease_token);
    this.leaseWorkers.delete(input.lease_token);
    const result = { receipt_hash: event.hash || "kernel-unavailable", hlc: hlc(event.ts!), kernel_available: true };
    this.trace("origin.team.resource.checkin", { "origin.resource.id": input.resource_id, "origin.holder.id": input.holder_id, "origin.receipt.hash": result.receipt_hash, "origin.hlc": result.hlc, "origin.kernel.available": true });
    return result;
  }
  async heartbeatLease(input: { workspace_id: string; resource_id: string; lease_token: string }): Promise<ReceiptResult> {
    const node = await this.node(input.workspace_id);
    if (!node || !this.leases.has(input.lease_token)) return this.unavailable("origin.team.resource.heartbeat", input.resource_id);
    // kernel 没有 renew 原语：将 heartbeat 作为已授权 action 签名，local-provider 据此延后 expires_at。
    const worker = this.leaseWorkers.get(input.lease_token);
    if (!worker) return this.unavailable("origin.team.resource.heartbeat", input.resource_id);
    const event = worker.exercise({ leaseId: input.lease_token, action: "resource.heartbeat", idem: `heartbeat:${input.lease_token}:${Date.now()}` });
    const receipt = node.ackExercise(event);
    const result = { receipt_hash: receipt.hash || "kernel-unavailable", hlc: hlc(receipt.ts!), kernel_available: true };
    this.trace("origin.team.resource.heartbeat", { "origin.resource.id": input.resource_id, "origin.receipt.hash": result.receipt_hash, "origin.hlc": result.hlc, "origin.kernel.available": true });
    return result;
  }
  async signApproval(input: { workspace_id: string; approval_id: string; actor: string; decision: "approved" | "rejected" }): Promise<ReceiptResult> {
    const node = await this.node(input.workspace_id);
    if (!node) return this.unavailable(`origin.team.approval.${input.decision}`, input.approval_id);
    const grant = node.grant({ to: input.actor, caps: ["approval.decide"], ttlMs: 60_000, resource: input.approval_id });
    const Approver = node.constructor as unknown as new (id: string) => KernelNode;
    const approver = new Approver(input.actor);
    approver.receive(grant);
    const exercise = approver.exercise({ leaseId: grant.leaseId!, action: "approval.decide", idem: `${input.decision}:${input.approval_id}` });
    const receipt = node.ackExercise(exercise);
    const result = { receipt_hash: receipt.hash || "kernel-unavailable", hlc: hlc(receipt.ts!), kernel_available: true };
    this.trace(`origin.team.approval.${input.decision}`, { "origin.approval.id": input.approval_id, "origin.receipt.hash": result.receipt_hash, "origin.hlc": result.hlc, "origin.kernel.available": true });
    return result;
  }
  private async unavailable(name: string, id: string, holderId?: string): Promise<ReceiptResult> {
    console.warn("[team-space] 2origin kernel unavailable; recording local audit event", { name, id });
    const result = { receipt_hash: "kernel-unavailable", hlc: localHlc(), kernel_available: false };
    this.trace(name, { "origin.entity.id": id, ...(holderId ? { "origin.holder.id": holderId } : {}), "origin.receipt.hash": result.receipt_hash, "origin.hlc": result.hlc, "origin.kernel.available": false });
    return result;
  }
}
