/** AI 优化大师体检报告 → u-chat 的纯文本交接提示词（不落盘，避免泄露客户环境）。 */
export type DoctorReportCheck = {
  id: string;
  category: string;
  name: string;
  status: "pass" | "warn" | "fail" | "info";
  points: number;
  earned: number;
  bonus: number;
  detail: string;
  fix_hint: string;
};

export type DoctorReport = {
  tool: string;
  version: string;
  score: number;
  earned: number;
  total: number;
  checks: DoctorReportCheck[];
};

const MAX_LENGTH = 1500;
const PATH_WITH_USERNAME = /C:\\Users\\[^\\\s"'`]+/gi;

function redact(value: string): string {
  return value.replace(PATH_WITH_USERNAME, "~");
}

function shorten(value: string, maxLength: number): string {
  if (value.length <= maxLength) return value;
  return maxLength <= 1 ? "…".slice(0, maxLength) : `${value.slice(0, maxLength - 1)}…`;
}

function formatCheck(check: DoctorReportCheck): string {
  const name = redact(check.name);
  const detail = redact(check.detail);
  const fixHint = redact(check.fix_hint);
  return `- [${check.status}] ${name}${detail ? `：${detail}` : ""}${fixHint ? `（修法：${fixHint}）` : ""}`;
}

/**
 * 将体检快照压成可直接投递给 u-chat 的建议请求。
 * 只保留失分项摘要，完整体检仍由 AI 通过只读 Action 按需读取。
 */
export function buildDoctorPrompt(report: DoctorReport): string {
  const intro = "这是一台 Windows/Mac 电脑的「AI 优化大师」体检结果，请根据失分项给具体的优化建议，按优先级排序，每条说清为什么、怎么修、能省多少 token 或避免什么翻车。";
  const passedCount = report.checks.filter((check) => check.status === "pass").length;
  const summary = `总分 ${report.score}/${report.total}（已获得 ${report.earned}/${report.total} 分），已达标 ${passedCount} 项。`;
  const tail = [
    "如需重新获取完整体检数据，可调用只读动作 runtime.optimizer.inspect。",
    "如需直接修改机器配置，可调用写动作 runtime.optimizer.apply（会先弹确认）。",
  ];
  const rows = report.checks
    .filter((check) => check.status !== "pass")
    .sort((a, b) => (b.points - b.earned) - (a.points - a.earned))
    .slice(0, 10)
    .map(formatCheck);
  const infoNames = report.checks
    .filter((check) => check.status === "info")
    .map((check) => redact(check.name))
    .filter(Boolean);
  let infoLine = infoNames.length ? `不计分提示：${infoNames.join("；")}` : "";
  let selectedRows = rows;
  let truncated = false;
  const truncationNote = "体检失分项较多，已截断；可在 AI 优化大师查看完整清单。";
  const compose = (includeNote: boolean) => [intro, summary, ...selectedRows, infoLine, includeNote ? truncationNote : "", ...tail]
    .filter(Boolean)
    .join("\n");

  while (compose(truncated).length > MAX_LENGTH && selectedRows.length) {
    selectedRows = selectedRows.slice(0, -1);
    truncated = true;
  }
  if (compose(truncated).length > MAX_LENGTH && infoLine) {
    truncated = true;
    const available = Math.max(0, MAX_LENGTH - compose(true).length + infoLine.length);
    infoLine = shorten(infoLine, available);
  }
  // 静态提示与摘要远小于上限；这层兜底只保护异常超长的服务端字段。
  return shorten(compose(truncated), MAX_LENGTH);
}
