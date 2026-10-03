/**
 * 对话输入框的「待发送图片」—— 轻助手（Chat.tsx）和 Claude/Codex（panels/ChatPanel.tsx）**共用这一份**。
 *
 * 以前两处各抄了一套：拖入时只记路径，发送时才去读。客户机实测，从截图工具窗口直接拖出的图是临时文件，
 * 用过几次就被系统清掉，此后每次发送都「找不到文件」；失败后坏附件又被原样放回、界面上没有任何
 * 移除入口，这一轮就永远发不出去。所以现在是：
 *  - 拖入/粘贴时立刻 `stageImage` 复制成我们自己的副本，识图读副本；
 *  - 每张图在输入框上方有个小标签，带 × 可以自己摘掉；
 *  - 发送失败时按错误码决定是整体恢复，还是只摘掉已经没了的那张。
 *
 * 两处不许再各写一份：复制的那份迟早跟这份漂开，漂开的那次正好是出事那次。
 */
import { useCallback, useRef, useState, type Dispatch, type SetStateAction } from "react";
import { Image as ImageIcon, X } from "lucide-react";
import {
  fileLabel,
  imageLabel,
  stageImage,
  stripImageLabel,
  visionErrorText,
  type PendingImage,
} from "../lib/vision";
import { useI18n } from "../i18n";

export function usePendingImages({
  setInput,
  onStageError,
}: {
  /** 宿主输入框的 setState。一律用函数式更新 —— 暂存是异步的，用户可能正同时在打字。 */
  setInput: Dispatch<SetStateAction<string>>;
  /** 某张图没能暂存（原文件已不在、超过 20MB、复制超时…）。`message` 已去掉协议前缀，可直接展示。 */
  onStageError: (name: string, message: string) => void;
}) {
  const [images, setImages] = useState<PendingImage[]>([]);
  /** 列表的同步真身。所有改动都经 `commit`，ref 和 state 同时更新 ——
   *  `take()` 读的是 ref，不会漏掉刚 `addImages` 完、还没来得及重渲染的那一张。 */
  const listRef = useRef<PendingImage[]>([]);
  const commit = useCallback((fn: (old: PendingImage[]) => PendingImage[]) => {
    listRef.current = fn(listRef.current);
    setImages(listRef.current);
  }, []);
  /** 已经在列表里、或正在暂存中的 source。用 ref 而不是读 state：连着拖两次同一张图时，
   *  第二次进来那一刻第一次的 setState 还没提交，只有同步可见的集合才能去重。 */
  const claimed = useRef<Set<string>>(new Set());
  const onErrorRef = useRef(onStageError);
  onErrorRef.current = onStageError;

  const addImages = useCallback(async (paths: string[]) => {
    const fresh = paths.filter((p) => {
      if (claimed.current.has(p)) return false;
      claimed.current.add(p);
      return true;
    });
    // 逐张串行：输入框里标签的顺序跟拖入顺序一致，也不会同时压一堆复制。
    for (const source of fresh) {
      try {
        const staged = await stageImage(source);
        commit((old) => [...old, { path: staged, source }]);
        setInput((v) => [v.trimEnd(), imageLabel(staged)].filter(Boolean).join(" ") + " ");
      } catch (e) {
        claimed.current.delete(source);
        onErrorRef.current(fileLabel(source), visionErrorText(e));
      }
    }
  }, [setInput, commit]);

  /** 用户点 ×：从列表去掉，并把输入框里它的标签一起摘掉（否则文字里还说着「已附图片」）。 */
  const remove = useCallback((img: PendingImage) => {
    claimed.current.delete(img.source);
    commit((old) => old.filter((i) => i.path !== img.path));
    setInput((v) => stripImageLabel(v, img.path));
  }, [setInput, commit]);

  /** 发送前取走当前列表（并清空）。取走之后用户再拖进来的图归下一轮。 */
  const take = useCallback((): PendingImage[] => {
    const taken = listRef.current;
    for (const i of taken) claimed.current.delete(i.source);
    commit(() => []);
    return taken;
  }, [commit]);

  /**
   * 发送失败后恢复。`text` 是取走时输入框里的文字，`taken` 是 `take()` 取走的图片。
   * `missing` 给了就表示这张图已经不在了（`ImageDescribeError.image`）：它的标签从文字里摘掉、
   * 图片也不放回 —— 放回去只会下次同样失败。
   * 恢复文字放在用户这期间新打的内容前面，不覆盖。
   */
  const restore = useCallback((text: string, taken: PendingImage[], missing?: string) => {
    const keep = missing ? taken.filter((i) => i.path !== missing) : taken;
    for (const i of keep) claimed.current.add(i.source);
    commit((old) => [...keep.filter((k) => !old.some((o) => o.path === k.path)), ...old]);
    const back = missing ? stripImageLabel(text, missing) : text;
    setInput((v) => (v.trim() ? `${back} ${v.trimStart()}` : back));
  }, [setInput, commit]);

  return { images, addImages, remove, take, restore };
}

/** 输入框上方那排待发图片小标签：图标 + 文件名 + ×。没有待发图片时不渲染。 */
export function PendingImageChips({ images, onRemove }: { images: PendingImage[]; onRemove: (img: PendingImage) => void }) {
  const { t } = useI18n();
  if (!images.length) return null;
  return (
    <div className="mb-1.5 flex flex-wrap gap-1.5">
      {images.map((img) => {
        const name = fileLabel(img.path);
        const label = t("移除图片 {name}", { name });
        return (
          <span
            key={img.path}
            title={name}
            className="inline-flex items-center gap-1 h-6 max-w-[220px] rounded-lg border border-white/[0.08] bg-white/[0.04] pl-1.5 pr-0.5 text-[11px] text-ink-2"
          >
            <ImageIcon size={12} className="shrink-0 text-ink-3" />
            <span className="truncate">{name}</span>
            <button
              type="button"
              onClick={() => onRemove(img)}
              aria-label={label}
              title={label}
              data-testid="pending-image-remove"
              className="inline-flex items-center justify-center w-5 h-5 shrink-0 rounded-md text-ink-4 hover:text-ink-1 hover:bg-white/[0.06]"
            >
              <X size={12} />
            </button>
          </span>
        );
      })}
    </div>
  );
}
