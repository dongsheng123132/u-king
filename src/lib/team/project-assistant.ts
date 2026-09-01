import { Channel, invoke } from "@tauri-apps/api/core";
import type { DeviceKey } from "../types";
import { wrapGenAICall } from "../otel/tracer";
import { createTeamSpaceStore, type TeamSpaceStore } from "./store";

export type ProjectContext = { prompt: string; factSummary: string };
export type AssistantAnswer = { answer: string; suggestedAction?: "suggest_approval"; draftChanges: string[]; offline: boolean };

/**
 * 只给模型项目元数据与审计摘要；这里刻意不读资源正文，避免把客户文件全文带出边界。
 */
export function buildProjectContext(projectId: string, store: TeamSpaceStore = createTeamSpaceStore()): ProjectContext {
  const resources = store.listResources(projectId);
  const locks = resources.flatMap((resource) => {
    const lock = store.getLock(resource.id);
    return lock ? [`${resource.name}：${lock.holder_id} 持锁至 ${lock.expires_at}`] : [];
  });
  const approvals = store.listApprovals(projectId).filter((item) => item.status === "pending");
  const activity = store.listActivity(projectId).slice(0, 20);
  const resourceLines = resources.map((item) => `- ${item.name} | ${item.collaboration_mode} | ${item.current_revision_id} | AI=${item.ai_access}`);
  const approvalLines = approvals.map((item) => `- ${item.title}：${item.summary}${item.draft_diff ? `；草稿 ${item.draft_diff.resource_id} ${item.draft_diff.revision_from}→${item.draft_diff.revision_to}` : ""}`);
  const activityLines = activity.map((item) => `- ${item.timestamp} ${item.actor}${item.actor_is_ai ? "(AI)" : ""}：${item.action}${item.resource_id ? ` · ${item.resource_id}` : ""}`);
  const factSummary = [
    `项目资源 ${resources.length} 项：${resources.map((item) => item.name).join("、") || "暂无"}。`,
    `当前锁：${locks.length ? locks.join("；") : "无排他锁"}。`,
    `待审批 ${approvals.length} 项：${approvals.map((item) => item.title).join("、") || "无"}。`,
    `最近活动：${activity.slice(0, 3).map((item) => `${item.actor}${item.action}`).join("；") || "暂无"}。`,
  ].join("\n");
  return {
    factSummary,
    prompt: `你是 U-King 团队空间的项目顾问。仅依据以下项目元数据回答；不要臆测资源正文，不要声称已读取文件全文。输出简明结论，并在适合时给出“建议变更点：”列表。\n\n## 资源\n${resourceLines.join("\n") || "- 无"}\n## 当前锁\n${locks.map((item) => `- ${item}`).join("\n") || "- 无"}\n## 待审批（含草稿摘要）\n${approvalLines.join("\n") || "- 无"}\n## 最近活动（最多20条）\n${activityLines.join("\n") || "- 无"}`,
  };
}

function draftChanges(answer: string) {
  const list = answer.split("\n").map((line) => line.replace(/^[-*\d.\s]+/, "").trim()).filter((line) => line.length > 8);
  return list.slice(0, 5);
}

/** 复用 U-Chat 的 chat_send + Channel 流式通道；不可用时以本地项目摘要降级。 */
export async function askAssistant(projectId: string, question: string, store: TeamSpaceStore = createTeamSpaceStore()): Promise<AssistantAnswer> {
  const context = buildProjectContext(projectId, store);
  const offline = () => ({ answer: `（离线）基于项目现状可给出以下事实摘要：\n${context.factSummary}`, suggestedAction: "suggest_approval" as const, draftChanges: [context.factSummary], offline: true });
  let device: DeviceKey;
  try { device = await invoke<DeviceKey>("get_device_key"); } catch { return offline(); }
  if (!device.key) return offline();
  let answer = "";
  try {
    await wrapGenAICall({ model: "deepseek-v4-flash", operation: "project_assistant", input: question, inputSummary: context.prompt, attributes: { "uking.agent": "team_project_assistant", "team.project_id": projectId } }, async (otel) => {
      const channel = new Channel<{ kind?: string; text?: string; input_tokens?: number; output_tokens?: number; request_id?: string; response_id?: string }>();
      channel.onmessage = (event) => {
        if (event.kind === "delta" && event.text) { otel.firstToken(); answer += event.text; }
        if (event.kind === "usage") {
          const input = Number(event.input_tokens); const output = Number(event.output_tokens);
          otel.response({ id: event.request_id ?? event.response_id, promptTokens: Number.isFinite(input) ? input : null, completionTokens: Number.isFinite(output) ? output : null, totalTokens: Number.isFinite(input) && Number.isFinite(output) ? input + output : null, output: answer });
        }
      };
      await invoke("chat_send", {
        taskId: `team-assistant-${projectId}`,
        messages: [{ role: "system", content: context.prompt }, { role: "user", content: question }],
        model: "deepseek-v4-flash", apiKey: device.key, baseUrl: "https://api.u-claw.org/v1", workspace: null, approvalMode: "ask", onEvent: channel,
      });
    });
  } catch { return offline(); }
  if (!answer.trim()) return offline();
  return { answer, suggestedAction: "suggest_approval", draftChanges: draftChanges(answer), offline: false };
}
