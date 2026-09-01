/**
 * 内联 diff —— Edit/Write 工具的 old_string/new_string 渲染成 - 红 / + 绿 行。
 * MVP 不引 diff 库：old 全部标删、new 全部标增（够看清改了什么）。
 */
export function DiffView({ path, oldStr, newStr }: { path: string; oldStr: string; newStr: string }) {
  const oldLines = oldStr ? oldStr.split("\n") : [];
  const newLines = newStr ? newStr.split("\n") : [];
  return (
    <div className="border-t border-white/[0.05] text-[11px] font-mono leading-relaxed max-h-64 overflow-y-auto">
      {path && (
        <div className="px-3 py-1 text-ink-4 bg-bg-2/50 sticky top-0 truncate">{path}</div>
      )}
      <div className="py-1">
        {oldLines.map((l, i) => (
          <div key={"o" + i} className="flex">
            <span className="w-5 shrink-0 text-center text-danger-400/70 select-none">-</span>
            <span className="px-1 bg-danger-500/[0.10] text-danger-400 whitespace-pre-wrap flex-1">{l || " "}</span>
          </div>
        ))}
        {newLines.map((l, i) => (
          <div key={"n" + i} className="flex">
            <span className="w-5 shrink-0 text-center text-success-400/70 select-none">+</span>
            <span className="px-1 bg-success-500/[0.10] text-success-400 whitespace-pre-wrap flex-1">{l || " "}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
