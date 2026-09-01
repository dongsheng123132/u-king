/** 团队空间的本地 provider 契约。所有文件引用都使用 resource_id + revision_id。 */
export type Workspace = { id: string; name: string; owner: string; type: "personal" | "team" };
export type Membership = { user_id: string; role: string; is_ai: boolean; workspace_id: string };
export type Project = { id: string; workspace_id: string; name: string };
export type CollaborationMode = "realtime_edit" | "mergeable" | "git" | "exclusive_lock" | "versioned_file";
export type Resource = {
  id: string; workspace_id: string; project_id: string; type: string; name: string;
  collaboration_mode: CollaborationMode; current_revision_id: string; visibility: string; ai_access: string;
};
export type ResourceLock = {
  resource_id: string; holder_id: string; device_id: string; lease_token: string; base_revision_id: string;
  acquired_at: string; expires_at: string; heartbeat_at: string;
};
export type PendingFile = { resource_id: string; revision_id: string };
export type Approval = {
  id: string; project_id: string; title: string; description: string; summary: string; pending_files: PendingFile[];
  status: "pending" | "approved" | "rejected"; requested_by: string; receipt_hash: string | null; hlc: string | null;
  /** 草稿与正式版分离；仅人工批准后才能合并。 */
  draft_diff?: {
    resource_id: string; revision_from: string; revision_to: string;
    changes: { action: "insert" | "delete" | "replace"; line_or_section: string; snippet: string }[];
  };
};
export type ActivityEvent = {
  id: string; project_id: string; actor: string; actor_is_ai: boolean; action: string; resource_id: string | null;
  revision_id: string | null; receipt_hash: string; prev_hash: string; kernel_receipt_hash?: string | null; hlc: string; timestamp: string;
};
export type TeamSpaceData = {
  workspaces: Workspace[]; memberships: Membership[]; projects: Project[]; resources: Resource[];
  locks: ResourceLock[]; approvals: Approval[]; activity: ActivityEvent[]; hlc_counter: number;
};
