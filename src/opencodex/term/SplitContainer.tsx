/**
 * 终端分屏容器 —— 把右侧终端区拆成左右/上下多格，每格一个独立终端 group。
 *
 * 用于「claude code 平行多开几个窗口」：每个叶子格挂一个 TermPanel（内含独立 useTermGroup，
 * seqRef 实例内化，多格零冲突）。分屏树是二叉树，纯内存不落盘。分隔条原生 mousemove 拖动，
 * 不引 react-resizable/allotment（守体积红线）。
 */
import { useCallback, useRef, useState } from "react";
// (useRef 已用于分隔条拖动 + firstPaneId)
import { Columns2, Rows2, X } from "lucide-react";
import { TermPanel } from "../panels/TermPanel";
import { useI18n } from "../../i18n";

type Pane = { kind: "pane"; id: number };
/** split 也要有 id —— 拖分隔条只能按 id 认，不能按对象引用认，原因见 updateRatio */
type Split = { kind: "split"; id: number; dir: "row" | "col"; ratio: number; a: Node; b: Node };
type Node = Pane | Split;

export function SplitContainer({
  cwd,
  active,
  tool,
  initialCmd,
}: {
  cwd: string;
  active: boolean;
  tool?: string;
  initialCmd?: string;
}) {
  // 节点序号实例内化（每个 SplitContainer 独立计数，左右两侧多实例不撞 React key）；
  // pane 和 split 共用一个计数器，省得两套 id 撞车。
  const paneSeqRef = useRef(0);
  const nextId = useCallback(() => ++paneSeqRef.current, []);
  const newPane = useCallback((): Pane => ({ kind: "pane", id: nextId() }), [nextId]);
  const [root, setRoot] = useState<Node>(() => ({ kind: "pane", id: ++paneSeqRef.current }) as Node);
  // initialCmd 只给最初那个 pane（分屏新增的不再重复跑启动命令）
  const firstPaneId = useRef<number | null>(null);
  if (firstPaneId.current === null && root.kind === "pane") firstPaneId.current = root.id;

  // 把某个 pane 拆成 split（在它原位放一个新 split，含原 pane + 新 pane）
  const splitPane = useCallback(
    (targetId: number, dir: "row" | "col") => {
      setRoot((r) =>
        transform(r, targetId, (p) => ({ kind: "split", id: nextId(), dir, ratio: 0.5, a: p, b: newPane() })),
      );
    },
    [newPane, nextId],
  );

  // 关闭某个 pane（用兄弟节点替换它的父 split）
  const closePane = useCallback((targetId: number) => {
    setRoot((r) => {
      if (r.kind === "pane") return r; // 只剩一个不关
      return removePane(r, targetId) ?? r;
    });
  }, []);

  const setRatio = useCallback((splitId: number, ratio: number) => {
    setRoot((r) => updateRatio(r, splitId, ratio));
  }, []);

  return (
    <div className="h-full min-h-0">
      <RenderNode
        node={root}
        cwd={cwd}
        active={active}
        tool={tool}
        initialCmd={initialCmd}
        firstPaneId={firstPaneId.current}
        onSplit={splitPane}
        onClose={closePane}
        onRatio={setRatio}
        canClose={root.kind === "split"}
      />
    </div>
  );
}

function RenderNode({
  node,
  cwd,
  active,
  tool,
  initialCmd,
  firstPaneId,
  onSplit,
  onClose,
  onRatio,
  canClose,
}: {
  node: Node;
  cwd: string;
  active: boolean;
  tool?: string;
  initialCmd?: string;
  firstPaneId: number | null;
  onSplit: (id: number, dir: "row" | "col") => void;
  onClose: (id: number) => void;
  onRatio: (splitId: number, ratio: number) => void;
  canClose: boolean;
}) {
  const { t } = useI18n();
  if (node.kind === "pane") {
    return (
      <div className="relative h-full min-h-0 min-w-0">
        {/* 每格右上角：拆分 / 关闭 */}
        <div className="absolute top-1 right-1 z-10 flex items-center gap-0.5 opacity-0 hover:opacity-100 focus-within:opacity-100 transition-opacity">
          <button
            onClick={() => onSplit(node.id, "row")}
            title={t("左右分屏")}
            className="w-6 h-6 rounded bg-bg-2/90 border border-white/[0.10] flex items-center justify-center text-ink-3 hover:text-ink-0 hover:bg-accent/[0.20]"
          >
            <Columns2 size={12} />
          </button>
          <button
            onClick={() => onSplit(node.id, "col")}
            title={t("上下分屏")}
            className="w-6 h-6 rounded bg-bg-2/90 border border-white/[0.10] flex items-center justify-center text-ink-3 hover:text-ink-0 hover:bg-accent/[0.20]"
          >
            <Rows2 size={12} />
          </button>
          {canClose && (
            <button
              onClick={() => onClose(node.id)}
              title={t("关闭此格")}
              className="w-6 h-6 rounded bg-bg-2/90 border border-white/[0.10] flex items-center justify-center text-ink-3 hover:text-danger-400 hover:bg-white/[0.08]"
            >
              <X size={12} />
            </button>
          )}
        </div>
        <TermPanel
          cwd={cwd}
          active={active}
          tool={tool}
          initialCmd={node.id === firstPaneId ? initialCmd : undefined}
        />
      </div>
    );
  }
  return (
    <SplitView
      node={node}
      cwd={cwd}
      active={active}
      tool={tool}
      initialCmd={initialCmd}
      firstPaneId={firstPaneId}
      onSplit={onSplit}
      onClose={onClose}
      onRatio={onRatio}
    />
  );
}

function SplitView({
  node,
  cwd,
  active,
  tool,
  initialCmd,
  firstPaneId,
  onSplit,
  onClose,
  onRatio,
}: {
  node: Split;
  cwd: string;
  active: boolean;
  tool?: string;
  initialCmd?: string;
  firstPaneId: number | null;
  onSplit: (id: number, dir: "row" | "col") => void;
  onClose: (id: number) => void;
  onRatio: (splitId: number, ratio: number) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const isRow = node.dir === "row";

  const onDown = (e: React.MouseEvent) => {
    e.preventDefault();
    const el = ref.current;
    if (!el) return;
    const move = (ev: MouseEvent) => {
      const rect = el.getBoundingClientRect();
      const r = isRow
        ? (ev.clientX - rect.left) / rect.width
        : (ev.clientY - rect.top) / rect.height;
      onRatio(node.id, Math.min(0.85, Math.max(0.15, r)));
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    document.body.style.cursor = isRow ? "col-resize" : "row-resize";
    document.body.style.userSelect = "none";
  };

  return (
    <div ref={ref} className={"flex h-full min-h-0 min-w-0 " + (isRow ? "flex-row" : "flex-col")}>
      <div style={{ flexBasis: `${node.ratio * 100}%` }} className="min-h-0 min-w-0 overflow-hidden">
        <RenderNode node={node.a} cwd={cwd} active={active} tool={tool} initialCmd={initialCmd} firstPaneId={firstPaneId} onSplit={onSplit} onClose={onClose} onRatio={onRatio} canClose />
      </div>
      <div
        onMouseDown={onDown}
        className={
          "shrink-0 bg-white/[0.06] hover:bg-accent/60 transition-colors " +
          (isRow ? "w-1 cursor-col-resize" : "h-1 cursor-row-resize")
        }
      />
      <div style={{ flexBasis: `${(1 - node.ratio) * 100}%` }} className="min-h-0 min-w-0 overflow-hidden">
        <RenderNode node={node.b} cwd={cwd} active={active} tool={tool} initialCmd={initialCmd} firstPaneId={firstPaneId} onSplit={onSplit} onClose={onClose} onRatio={onRatio} canClose />
      </div>
    </div>
  );
}

/* ---- 树操作（纯函数，不可变更新）---- */

// 把满足 id 的 pane 替换成 fn(pane) 的结果
function transform(node: Node, id: number, fn: (p: Pane) => Node): Node {
  if (node.kind === "pane") return node.id === id ? fn(node) : node;
  return { ...node, a: transform(node.a, id, fn), b: transform(node.b, id, fn) };
}

// 移除 id 对应的 pane：找到它的父 split，用兄弟替换；返回新树或 null（没找到）
function removePane(node: Node, id: number): Node | null {
  if (node.kind === "pane") return null;
  // 直接子是目标 pane → 返回兄弟
  if (node.a.kind === "pane" && node.a.id === id) return node.b;
  if (node.b.kind === "pane" && node.b.id === id) return node.a;
  // 递归
  const a = removePane(node.a, id);
  if (a) return { ...node, a };
  const b = removePane(node.b, id);
  if (b) return { ...node, b };
  return null;
}

// 按 **id** 认，不能按对象引用认。
// 原来是 `node === target`：拖分隔条时 mousemove 监听器是 onDown 那一刻建的，捕获的是当时那个
// Split 对象；而每次 setRoot 都会把整棵树重建成新对象（`{...node, ratio}` 连同各级祖先），
// 于是**第一次 move 之后捕获的那个对象就不在树里了**，后续每一帧都匹配不上、原样返回 ——
// 表现就是分隔条跳一下就再也拖不动。id 在重建时是原样带过去的，认 id 才稳。
function updateRatio(node: Node, targetId: number, ratio: number): Node {
  if (node.kind === "pane") return node;
  if (node.id === targetId) return { ...node, ratio };
  return { ...node, a: updateRatio(node.a, targetId, ratio), b: updateRatio(node.b, targetId, ratio) };
}
