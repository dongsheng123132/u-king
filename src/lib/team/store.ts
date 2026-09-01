import { TeamKernelBridge } from "./kernel-bridge";
import { createTeamSpaceSeed } from "./seed";
import type { ActivityEvent, Approval, Membership, Project, Resource, ResourceLock, TeamSpaceData, Workspace } from "./types";

const STORAGE_KEY = "uking.team-space.local-provider.v1";
type StorageLike = Pick<Storage, "getItem" | "setItem">;
type Options = { storage?: StorageLike; bridge?: TeamKernelBridge; now?: () => Date };
type ActivityInput = Omit<ActivityEvent, "id" | "timestamp" | "hlc" | "receipt_hash" | "prev_hash" | "kernel_receipt_hash"> & { hlc?: string; kernel_receipt_hash?: string | null };
export type LeaseTakeover = { previous_holder_id: string; expired_at: string; release_receipt_hash: string };
export type LeaseAcquireResult = { lock: ResourceLock; takeover: LeaseTakeover | null };
export type ActivityChainVerification = { ok: boolean; checked: number; reason?: string };
export type AiMemberState = Pick<Membership, "user_id" | "role" | "is_ai"> & { state: "idle" | "locked" | "pending_approval" };

const ACTIVITY_CHAIN_SEED = "team-space.activity.v1:GENESIS";
const SEED_RECEIPT = "seed:team-space.activity.v1";

function clone<T>(value: T): T { return JSON.parse(JSON.stringify(value)) as T; }
function defaultStorage(): StorageLike | undefined { try { return globalThis.localStorage; } catch { return undefined; } }
export function isLeaseExpired(lock: Pick<ResourceLock, "expires_at">, now = new Date()) { return new Date(lock.expires_at).getTime() <= now.getTime(); }
function compareHlc(a: string, b: string) {
  const [aPhy, aLogical, ...aNode] = a.split(":"); const [bPhy, bLogical, ...bNode] = b.split(":");
  const phy = Number(aPhy) - Number(bPhy); if (phy) return phy;
  const logical = Number(aLogical) - Number(bLogical); if (logical) return logical;
  return aNode.join(":").localeCompare(bNode.join(":"));
}
async function sha256(value: string) {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value));
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}
function receiptPayload(event: Pick<ActivityEvent, "actor" | "action" | "resource_id" | "revision_id" | "prev_hash">) {
  return JSON.stringify({ actor: event.actor, action: event.action, resource_id: event.resource_id, revision_id: event.revision_id, prev_hash: event.prev_hash });
}

export class TeamSpaceStore {
  private data: TeamSpaceData;
  private readonly storage?: StorageLike;
  private readonly bridge: TeamKernelBridge;
  private readonly now: () => Date;
  constructor({ storage = defaultStorage(), bridge = new TeamKernelBridge(), now = () => new Date() }: Options = {}) {
    this.storage = storage; this.bridge = bridge; this.now = now;
    this.data = this.load();
  }
  private load(): TeamSpaceData {
    try { const raw = this.storage?.getItem(STORAGE_KEY); if (raw) return JSON.parse(raw) as TeamSpaceData; } catch { /* 保持本地演示可用 */ }
    const seeded = createTeamSpaceSeed(this.now()); this.save(seeded); return seeded;
  }
  private save(data = this.data) { this.data = data; try { this.storage?.setItem(STORAGE_KEY, JSON.stringify(data)); } catch { /* localStorage 配额满也不阻断当前会话 */ } }
  private nextHlc() { this.data.hlc_counter += 1; return `${this.now().getTime()}:${this.data.hlc_counter}:local-provider`; }
  private async activity(event: ActivityInput) {
    const prev_hash = this.data.activity[0]?.receipt_hash || ACTIVITY_CHAIN_SEED;
    const full: ActivityEvent = { ...event, id: `activity-${this.now().getTime()}-${this.data.hlc_counter}`, timestamp: this.now().toISOString(), hlc: event.hlc || this.nextHlc(), prev_hash, receipt_hash: "" };
    full.receipt_hash = await sha256(receiptPayload(full));
    this.data.activity.unshift(full); this.save(); return clone(full);
  }
  listWorkspaces() { return clone(this.data.workspaces); }
  listMemberships(workspaceId?: string) { return clone(this.data.memberships.filter((member) => !workspaceId || member.workspace_id === workspaceId)); }
  listAiMemberStates(workspaceId: string): AiMemberState[] {
    return this.data.memberships.filter((member) => member.workspace_id === workspaceId && member.is_ai).map((member) => {
      const locked = this.data.locks.some((lock) => lock.holder_id === member.user_id && !isLeaseExpired(lock, this.now()));
      const pending = this.data.approvals.some((approval) => approval.requested_by === member.user_id && approval.status === "pending");
      return { user_id: member.user_id, role: member.role, is_ai: true, state: locked ? "locked" : pending ? "pending_approval" : "idle" };
    });
  }
  createWorkspace(workspace: Workspace) { this.data.workspaces.push(clone(workspace)); this.save(); return clone(workspace); }
  listProjects(workspaceId?: string) { return clone(this.data.projects.filter((p) => !workspaceId || p.workspace_id === workspaceId)); }
  listResources(projectId?: string) { return clone(this.data.resources.filter((r) => !projectId || r.project_id === projectId)); }
  getResource(resourceId: string) { const r = this.data.resources.find((item) => item.id === resourceId); return r ? clone(r) : null; }
  getLock(resourceId: string) { const lock = this.data.locks.find((item) => item.resource_id === resourceId); return lock ? clone(lock) : null; }
  listApprovals(projectId?: string) { return clone(this.data.approvals.filter((a) => !projectId || a.project_id === projectId)); }
  async createAssistantDraft(projectId: string, summary: string, changes: string[], requestedBy = "AI项目顾问") {
    this.requiredProject(projectId);
    const resource = this.data.resources.find((item) => item.project_id === projectId) || null;
    const id = `approval-assistant-${this.now().getTime()}-${this.data.hlc_counter}`;
    const revisionFrom = resource?.current_revision_id || "project_summary";
    const approval: Approval = {
      id, project_id: projectId, title: "AI 项目顾问草稿", description: "AI 根据项目元数据生成的建议；未获人工批准前不会写入正式资源。",
      summary: summary.slice(0, 500), pending_files: resource ? [{ resource_id: resource.id, revision_id: `${revisionFrom}_ai_draft` }] : [], status: "pending", requested_by: requestedBy, receipt_hash: null, hlc: null,
      draft_diff: resource ? { resource_id: resource.id, revision_from: revisionFrom, revision_to: `${revisionFrom}_ai_draft`, changes: (changes.length ? changes : [summary]).slice(0, 5).map((snippet, index) => ({ action: "insert", line_or_section: `建议 ${index + 1}`, snippet: snippet.slice(0, 500) })) } : undefined,
    };
    this.data.approvals.unshift(approval); this.save();
    await this.activity({ project_id: projectId, actor: requestedBy, actor_is_ai: true, action: "生成 AI 草稿（待人工审批）", resource_id: resource?.id || null, revision_id: approval.pending_files[0]?.revision_id || null });
    return clone(approval);
  }
  listActivity(projectId?: string) { return clone(this.data.activity.filter((a) => !projectId || a.project_id === projectId)); }
  appendActivity(event: ActivityInput) { return this.activity(event); }
  async lockResource(resourceId: string, holderId: string, deviceId: string): Promise<ResourceLock> { return (await this.tryAcquireExclusiveLock(resourceId, holderId, deviceId)).lock; }
  async tryAcquireExclusiveLock(resourceId: string, holderId: string, deviceId = "local-device"): Promise<LeaseAcquireResult> {
    const resource = this.requiredResource(resourceId); const existing = this.data.locks.find((lock) => lock.resource_id === resourceId);
    let takeover: LeaseTakeover | null = null;
    if (existing) {
      if (!isLeaseExpired(existing, this.now())) throw new Error(`资源正由 ${existing.holder_id} 签出，Lease 到期 ${new Date(existing.expires_at).toLocaleString()}`);
      const release = await this.bridge.releaseLease({ workspace_id: resource.workspace_id, resource_id: resourceId, lease_token: existing.lease_token, holder_id: existing.holder_id });
      this.data.locks = this.data.locks.filter((item) => item.resource_id !== resourceId); this.save();
      takeover = { previous_holder_id: existing.holder_id, expired_at: existing.expires_at, release_receipt_hash: release.receipt_hash };
      await this.activity({ project_id: resource.project_id, actor: "系统", actor_is_ai: false, action: "租约过期自动释放", resource_id: resource.id, revision_id: resource.current_revision_id, kernel_receipt_hash: release.receipt_hash, hlc: release.hlc });
    }
    const lease = await this.bridge.issueLease({ workspace_id: resource.workspace_id, resource_id: resource.id, holder_id: holderId, ttl_ms: 2 * 60 * 60 * 1000 });
    const lock: ResourceLock = { resource_id: resource.id, holder_id: holderId, device_id: deviceId, lease_token: lease.lease_token, base_revision_id: resource.current_revision_id, acquired_at: this.now().toISOString(), expires_at: lease.expires_at, heartbeat_at: lease.heartbeat_at };
    this.data.locks = this.data.locks.filter((item) => item.resource_id !== resourceId); this.data.locks.push(lock); this.save();
    await this.activity({ project_id: resource.project_id, actor: holderId, actor_is_ai: this.actorIsAi(resource.workspace_id, holderId), action: takeover ? "接管文件（Lease）" : "签出文件（Lease）", resource_id: resource.id, revision_id: resource.current_revision_id, kernel_receipt_hash: lease.kernel_available ? lease.lease_token : "kernel-unavailable", hlc: lease.hlc });
    return { lock: clone(lock), takeover };
  }
  async unlockResource(resourceId: string, actor: string) {
    const resource = this.requiredResource(resourceId); const lock = this.getLock(resourceId); if (!lock) throw new Error("资源尚未签出");
    const receipt = await this.bridge.releaseLease({ workspace_id: resource.workspace_id, resource_id: resourceId, lease_token: lock.lease_token, holder_id: actor });
    this.data.locks = this.data.locks.filter((item) => item.resource_id !== resourceId); this.save();
    return this.activity({ project_id: resource.project_id, actor, actor_is_ai: this.actorIsAi(resource.workspace_id, actor), action: "签入文件（Lease 释放）", resource_id: resource.id, revision_id: resource.current_revision_id, kernel_receipt_hash: receipt.receipt_hash, hlc: receipt.hlc });
  }
  async heartbeatLock(resourceId: string, actor: string) {
    const resource = this.requiredResource(resourceId); const lock = this.data.locks.find((item) => item.resource_id === resourceId); if (!lock) throw new Error("资源尚未签出");
    const receipt = await this.bridge.heartbeatLease({ workspace_id: resource.workspace_id, resource_id: resourceId, lease_token: lock.lease_token });
    lock.heartbeat_at = this.now().toISOString(); lock.expires_at = new Date(this.now().getTime() + 2 * 60 * 60 * 1000).toISOString(); this.save();
    return this.activity({ project_id: resource.project_id, actor, actor_is_ai: this.actorIsAi(resource.workspace_id, actor), action: "续租心跳", resource_id: resource.id, revision_id: resource.current_revision_id, kernel_receipt_hash: receipt.receipt_hash, hlc: receipt.hlc });
  }
  async approveApproval(approvalId: string, actor = "贺方升") { return this.decideApproval(approvalId, actor, "approved"); }
  async rejectApproval(approvalId: string, actor = "贺方升") { return this.decideApproval(approvalId, actor, "rejected"); }
  private async decideApproval(approvalId: string, actor: string, decision: "approved" | "rejected") {
    const approval = this.requiredApproval(approvalId); if (approval.status !== "pending") throw new Error("审批已处理");
    const project = this.requiredProject(approval.project_id);
    const receipt = await this.bridge.signApproval({ workspace_id: project.workspace_id, approval_id: approval.id, actor, decision });
    approval.status = decision; approval.receipt_hash = receipt.receipt_hash; approval.hlc = receipt.hlc; this.save();
    const pending = approval.pending_files[0] || { resource_id: null, revision_id: null };
    return this.activity({ project_id: approval.project_id, actor, actor_is_ai: false, action: decision === "approved" ? "批准 AI 草稿（Receipt）" : "拒绝 AI 草稿（Receipt）", resource_id: pending.resource_id, revision_id: pending.revision_id, kernel_receipt_hash: receipt.receipt_hash, hlc: receipt.hlc });
  }
  async verifyActivityHashChain(): Promise<ActivityChainVerification> {
    const ordered = [...this.data.activity].reverse(); let previous = ACTIVITY_CHAIN_SEED; const seen = new Set<string>();
    for (let index = 0; index < ordered.length; index += 1) {
      const event = ordered[index];
      if (!event.receipt_hash || seen.has(event.receipt_hash)) return { ok: false, checked: index, reason: "receipt_hash 为空或重复" };
      if (event.prev_hash !== previous) return { ok: false, checked: index, reason: "prev_hash 不连续" };
      if (index === 0 && event.receipt_hash === SEED_RECEIPT) { previous = event.receipt_hash; seen.add(previous); continue; }
      if (index > 0 && compareHlc(ordered[index - 1].hlc, event.hlc) > 0) return { ok: false, checked: index, reason: "HLC 非单调" };
      if (event.receipt_hash !== await sha256(receiptPayload(event))) return { ok: false, checked: index, reason: "receipt_hash 与活动内容不匹配" };
      previous = event.receipt_hash; seen.add(previous);
    }
    return { ok: true, checked: ordered.length };
  }
  resetDemo() { this.save(createTeamSpaceSeed(this.now())); return this.listWorkspaces(); }
  private requiredResource(id: string): Resource { const value = this.data.resources.find((item) => item.id === id); if (!value) throw new Error("资源不存在"); return value; }
  private actorIsAi(workspaceId: string, actorId: string) { return this.data.memberships.some((member) => member.workspace_id === workspaceId && member.user_id === actorId && member.is_ai); }
  private requiredProject(id: string): Project { const value = this.data.projects.find((item) => item.id === id); if (!value) throw new Error("项目不存在"); return value; }
  private requiredApproval(id: string): Approval { const value = this.data.approvals.find((item) => item.id === id); if (!value) throw new Error("审批不存在"); return value; }
}

export function createTeamSpaceStore(options?: Options) { return new TeamSpaceStore(options); }
