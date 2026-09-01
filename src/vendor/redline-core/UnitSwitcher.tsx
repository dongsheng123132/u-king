import type { RedlineUnit } from "./document-model";

/** 多 unit 格式（PDF 页/PSD 图层/XLSX sheet……）的切换条，单 unit 格式不渲染（由调用方判断）。 */
export function UnitSwitcher({
  units,
  activeUnitId,
  onChange,
}: {
  units: RedlineUnit[];
  activeUnitId: string;
  onChange: (id: string) => void;
}) {
  return (
    <div className="flex items-center gap-1 h-8 px-2 border-b border-white/[0.06] overflow-x-auto shrink-0 text-[11px]">
      {units.map((u) => (
        <button
          key={u.id}
          onClick={() => onChange(u.id)}
          className={
            "px-2 py-0.5 rounded whitespace-nowrap shrink-0 " +
            (u.id === activeUnitId ? "bg-blue-600 text-white" : "text-gray-400 hover:bg-white/10")
          }
          title={u.label}
        >
          {u.label}
        </button>
      ))}
    </div>
  );
}
