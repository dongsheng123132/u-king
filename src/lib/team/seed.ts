import type { TeamSpaceData } from "./types";

/** 可重复装载的演示数据；时间在载入时生成，确保 CAD 租约总是还有两小时。 */
export function createTeamSpaceSeed(now = new Date()): TeamSpaceData {
  const later = new Date(now.getTime() + 2 * 60 * 60 * 1000).toISOString();
  const memberships = [
    { workspace_id: "ws-hequbing", user_id: "贺方升", role: "owner", is_ai: false },
    { workspace_id: "ws-hequbing", user_id: "张三", role: "editor", is_ai: false },
    { workspace_id: "ws-hequbing", user_id: "AI产品经理", role: "product", is_ai: true },
    { workspace_id: "ws-hequbing", user_id: "AI架构师", role: "architect", is_ai: true },
    { workspace_id: "ws-hequbing", user_id: "AI开发", role: "developer", is_ai: true },
    { workspace_id: "ws-hequbing", user_id: "AI测试", role: "tester", is_ai: true },
    { workspace_id: "ws-hequbing", user_id: "AI审核员", role: "reviewer", is_ai: true },
  ];
  const cadHolder = memberships.find((member) => member.user_id === "张三")!;
  return {
    workspaces: [
      { id: "ws-personal", name: "我的个人空间", owner: "贺方升", type: "personal" },
      { id: "ws-hequbing", name: "贺去病科技", owner: "贺方升", type: "team" },
    ],
    memberships,
    projects: [
      { id: "project-financing", workspace_id: "ws-hequbing", name: "U-King 融资计划" },
      { id: "project-software", workspace_id: "ws-hequbing", name: "U-King 软件协作" },
      { id: "project-regulation", workspace_id: "ws-hequbing", name: "医疗器械标书与法规创作" },
    ],
    resources: [
      { id: "res-plan", workspace_id: "ws-hequbing", project_id: "project-financing", type: "document", name: "商业计划书.docx", collaboration_mode: "realtime_edit", current_revision_id: "rev_020", visibility: "team", ai_access: "draft_only" },
      { id: "res-finance", workspace_id: "ws-hequbing", project_id: "project-financing", type: "spreadsheet", name: "财务预测.xlsx", collaboration_mode: "realtime_edit", current_revision_id: "rev_005", visibility: "team", ai_access: "draft_only" },
      { id: "res-research", workspace_id: "ws-hequbing", project_id: "project-financing", type: "folder", name: "市场调研/", collaboration_mode: "versioned_file", current_revision_id: "folder_001", visibility: "team", ai_access: "read" },
      { id: "res-screenshots", workspace_id: "ws-hequbing", project_id: "project-financing", type: "folder", name: "产品截图/", collaboration_mode: "versioned_file", current_revision_id: "folder_001", visibility: "team", ai_access: "read" },
      { id: "res-shell-dwg", workspace_id: "ws-hequbing", project_id: "project-financing", type: "cad", name: "外壳设计.dwg", collaboration_mode: "exclusive_lock", current_revision_id: "rev_017", visibility: "team", ai_access: "draft_only" },
      { id: "res-git-desktop", workspace_id: "ws-hequbing", project_id: "project-financing", type: "repository", name: "git-u-king-desktop", collaboration_mode: "git", current_revision_id: "commit_a3f91c7", visibility: "team", ai_access: "read" },
      { id: "res-git-app", workspace_id: "ws-hequbing", project_id: "project-software", type: "repository", name: "github.com/dongsheng123132/u-king-mini", collaboration_mode: "git", current_revision_id: "main@9d68921", visibility: "team", ai_access: "draft_only" },
      { id: "task-login", workspace_id: "ws-hequbing", project_id: "project-software", type: "task", name: "改登录模块：补 OAuth 回调错误态", collaboration_mode: "git", current_revision_id: "task_open", visibility: "team", ai_access: "draft_only" },
      { id: "task-ci", workspace_id: "ws-hequbing", project_id: "project-software", type: "task", name: "修 CI：Windows 冒烟矩阵", collaboration_mode: "git", current_revision_id: "task_open", visibility: "team", ai_access: "draft_only" },
      { id: "res-regulation", workspace_id: "ws-hequbing", project_id: "project-regulation", type: "document", name: "医疗器械投标合规说明.md", collaboration_mode: "mergeable", current_revision_id: "rev_011", visibility: "team", ai_access: "draft_only" },
    ],
    locks: [{ resource_id: "res-shell-dwg", holder_id: cadHolder.user_id, device_id: "pc-zhangsan", lease_token: "seed-lease-dwg-017", base_revision_id: "rev_017", acquired_at: now.toISOString(), expires_at: later, heartbeat_at: now.toISOString() }],
    approvals: [
      { id: "approval-plan-ai-001", project_id: "project-financing", title: "AI 修订商业计划书", description: "AI 审核员已生成修订草稿，等待负责人决定是否合并正式版本。", summary: "补充市场规模、产品定位与融资用途；AI 不能直接覆盖正式版。", pending_files: [{ resource_id: "res-plan", revision_id: "rev_021_draft" }], status: "pending", requested_by: "AI审核员", receipt_hash: null, hlc: null, draft_diff: { resource_id: "res-plan", revision_from: "rev_020", revision_to: "rev_021_draft", changes: [{ action: "insert", line_or_section: "商业模式章节", snippet: "新增 442 字 AI 修订段落：按订阅、设备服务和企业版分层说明收入路径。" }, { action: "delete", line_or_section: "市场假设 2.1 / 2.3", snippet: "删除两处已过时的市场规模与渠道转化假设。" }] } },
      { id: "approval-regulation-ai-001", project_id: "project-regulation", title: "法规标书草稿：人工强审批", description: "AI 审核员只写入草稿区，法规条款与投标承诺必须由人工逐项确认。", summary: "新增风险控制和不良事件响应章节，替换旧版法规引用；未批准前不触及 rev_011 正式版。", pending_files: [{ resource_id: "res-regulation", revision_id: "rev_012_draft" }], status: "pending", requested_by: "AI审核员", receipt_hash: null, hlc: null, draft_diff: { resource_id: "res-regulation", revision_from: "rev_011", revision_to: "rev_012_draft", changes: [{ action: "insert", line_or_section: "第 4 节 风险控制", snippet: "新增产品追溯、投诉处置、风险复评和不良事件上报的 1,126 字草稿。" }, { action: "replace", line_or_section: "第 6 节 法规依据", snippet: "以现行法规清单替换旧版引用，并标出待法务确认的三处条款。" }, { action: "delete", line_or_section: "附录 B", snippet: "删除已失效的 2024 年供应商承诺模板。" }] } },
    ],
    activity: [{ id: "seed-activity-001", project_id: "project-financing", actor: "张三", actor_is_ai: false, action: "签出 CAD 设计文件", resource_id: "res-shell-dwg", revision_id: "rev_017", receipt_hash: "seed:team-space.activity.v1", prev_hash: "team-space.activity.v1:GENESIS", hlc: `${now.getTime()}:0:seed`, timestamp: now.toISOString() }],
    hlc_counter: 1,
  };
}
