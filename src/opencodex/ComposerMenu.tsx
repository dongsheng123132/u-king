/**
 * 输入框里的 `@` 和 `/` —— 借鉴 WorkBuddy 输入框那行小字「@ 引用对话文件，/ 调用技能与指令」。
 *
 * **一份实现两处用**（宪法第 12 条）：轻助手（`Chat.tsx`）和 Claude/Codex（`panels/ChatPanel.tsx`）
 * 的输入框共用这一个 hook。
 *
 * `@` = 引用工作文件夹里的文件。走后端现成的 `list_dir` 一层层点进去，选中把**相对路径**插进
 *       输入框（含空格自动加引号）。这不是装饰：在这之前想让 AI 读某个文件，客户只能拖进来、
 *       或者手敲一条自己也记不准的路径。没选工作文件夹时**明说**为什么用不了，不给空菜单。
 *
 * `/` = 指令面板，**只列真存在的东西**。上半是宿主传进来的真指令（开终端 / 开文件面板 / 清空对话…），
 *       下半是起手词全表（复用 `QuickPrompts` 那份词库，点了填输入框）。
 *       坚决不做「技能」这种我们其实没有的假菜单 —— 点了没反应，比压根没这个菜单更伤信任。
 *
 * 触发条件卡得很死，两条都是防误触发的硬要求：
 * - `/` **只认输入框的第一个字符**。否则「帮我改 src/foo.ts」打到斜杠就弹菜单。
 * - `@` 前面必须是开头或空白。否则邮箱、`user@host` 全中招。
 *
 * Esc 关掉后**同一个触发点不再自动弹回来**（`dismissedRef`）—— 不然用户按了 Esc、
 * 接着打下一个字，菜单又跳出来，等于 Esc 没用。
 */
import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CornerDownLeft, FileText, Folder, Wand2, Zap } from "lucide-react";
import { useI18n } from "../i18n";
import { cn } from "../lib/cn";
import { ALL_QUICK, type Best } from "./QuickPrompts";

type Trigger = { kind: "at" | "slash"; start: number; query: string };
type Entry = { name: string; path: string; is_dir: boolean; size: number };
/** 宿主传进来的**真**指令。label 是给人看的，run 必须真的干活。 */
export type ComposerCommand = { label: string; hint?: string; run: () => void };

type Row = {
  key: string;
  label: string;
  hint?: string;
  icon: typeof FileText;
  /** 选中后做什么。返回 true = 菜单继续开着（点进子目录用）。 */
  pick: () => boolean | void;
};

/** 只在输入框开头认 `/`；`@` 要前面是开头或空白。 */
function detect(value: string, caret: number): Trigger | null {
  const before = value.slice(0, caret);
  if (/^\/[^\s/]*$/.test(before)) return { kind: "slash", start: 0, query: before.slice(1) };
  const m = /(?:^|\s)@([^\s]*)$/.exec(before);
  if (m) return { kind: "at", start: caret - m[1].length - 1, query: m[1] };
  return null;
}

const joinDir = (root: string, sub: string) => (sub ? root.replace(/[\\/]+$/, "") + "/" + sub : root);
/** 路径含空格要加引号，否则 AI 把它当两个参数（跟拖放插路径同一个套路）。 */
const quote = (p: string) => (/\s/.test(p) ? `"${p}"` : p);

export function useComposerMenu({
  value,
  setValue,
  textareaRef,
  workspace,
  commands = [],
  onQuickPick,
}: {
  value: string;
  setValue: (v: string) => void;
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  /** 工作文件夹；没有时 `@` 只给一句「先选文件夹」的说明 */
  workspace?: string;
  commands?: ComposerCommand[];
  /** 起手词点到了别的大脑更拿手的活时交给宿主处理（切大脑 + 填词）。
   *  **必须和 QuickPrompts 那排走同一条路** —— 同一个「画张图」在两个入口行为不一样，
   *  就是「同一事实两份实现」，迟早漂移。不传则退化成只填输入框。 */
  onQuickPick?: (template: string, best: Best) => void;
}): { menu: ReactNode; onKeyDown: (e: KeyboardEvent<HTMLTextAreaElement>) => boolean; onBlur: () => void } {
  const { t } = useI18n();
  const [trig, setTrig] = useState<Trigger | null>(null);
  const [sel, setSel] = useState(0);
  const [files, setFiles] = useState<Entry[]>([]);
  const [listErr, setListErr] = useState("");
  const cacheRef = useRef(new Map<string, Entry[]>());
  const dismissedRef = useRef<string | null>(null);

  // 每次输入变化重新判定。此时 DOM 已更新，selectionStart 是准的。
  useEffect(() => {
    const caret = textareaRef.current?.selectionStart ?? value.length;
    const next = detect(value, caret);
    const sig = next ? `${next.kind}:${next.start}` : null;
    if (sig && dismissedRef.current === sig) return; // Esc 关过这一个触发点，别自己弹回来
    if (!sig) dismissedRef.current = null;
    setTrig(next);
    setSel(0);
  }, [value, textareaRef]);

  const atDir = trig?.kind === "at" ? trig.query.slice(0, trig.query.lastIndexOf("/") + 1) : "";
  const atName = trig?.kind === "at" ? trig.query.slice(trig.query.lastIndexOf("/") + 1) : "";

  // 拉当前这一层目录（一层一层来，跟文件面板同一条后端路径）。缓存住，免得每敲一个字重扫一遍盘。
  useEffect(() => {
    if (trig?.kind !== "at" || !workspace) return;
    const dir = joinDir(workspace, atDir);
    const cached = cacheRef.current.get(dir);
    if (cached) {
      setFiles(cached);
      setListErr("");
      return;
    }
    let alive = true;
    invoke<Entry[]>("list_dir", { path: dir, showNoise: false })
      .then((l) => {
        if (!alive) return;
        cacheRef.current.set(dir, l);
        setFiles(l);
        setListErr("");
      })
      .catch(() => {
        if (!alive) return;
        setFiles([]);
        setListErr(t("这个文件夹读不到"));
      });
    return () => {
      alive = false;
    };
  }, [trig?.kind, workspace, atDir, t]);

  const close = useCallback(() => {
    setTrig(null);
    setSel(0);
  }, []);

  /** 把 `@xxx` / `/xxx` 这一段替换成 text。keepOpen=true 用于点进子目录（继续补 query）。 */
  const replaceTrigger = useCallback(
    (text: string, keepOpen = false) => {
      const ta = textareaRef.current;
      const caret = ta?.selectionStart ?? value.length;
      const start = trig?.start ?? 0;
      const next = value.slice(0, start) + text + value.slice(caret);
      setValue(next);
      const pos = start + text.length;
      requestAnimationFrame(() => {
        ta?.focus();
        ta?.setSelectionRange(pos, pos);
      });
      if (!keepOpen) {
        dismissedRef.current = null;
        close();
      }
    },
    [value, setValue, textareaRef, trig, close],
  );

  const rows: Row[] = useMemo(() => {
    if (!trig) return [];
    if (trig.kind === "slash") {
      const q = trig.query.toLowerCase();
      const hit = (s: string) => !q || s.toLowerCase().includes(q);
      const cmdRows: Row[] = commands
        .filter((c) => hit(c.label))
        .map((c) => ({
          key: "cmd:" + c.label,
          label: t(c.label),
          hint: c.hint ? t(c.hint) : undefined,
          icon: Zap,
          pick: () => {
            replaceTrigger(""); // 指令不留在输入框里 —— 它是动作，不是要发给 AI 的话
            c.run();
          },
        }));
      const quickRows: Row[] = ALL_QUICK.filter((q2) => hit(q2.label) || hit(q2.template)).map((q2) => ({
        key: "quick:" + q2.label,
        label: t(q2.label),
        hint: t(q2.scene),
        icon: Wand2,
        pick: () => {
          if (q2.best && onQuickPick) {
            replaceTrigger(""); // 先把 `/xxx` 从输入框抹掉，再交给宿主填正文
            onQuickPick(t(q2.template), q2.best);
            return;
          }
          replaceTrigger(t(q2.template));
        },
      }));
      return [...cmdRows, ...quickRows].slice(0, 40);
    }
    const q = atName.toLowerCase();
    return files
      .filter((f) => !q || f.name.toLowerCase().includes(q))
      .slice(0, 60)
      .map((f) => ({
        key: "file:" + f.path,
        label: f.name,
        hint: f.is_dir ? undefined : atDir || undefined,
        icon: f.is_dir ? Folder : FileText,
        pick: () => {
          // 目录：继续往里走（菜单不关）；文件：插相对路径，这是 AI 那边最好认的形式
          if (f.is_dir) {
            replaceTrigger("@" + atDir + f.name + "/", true);
            return true;
          }
          replaceTrigger(quote(atDir + f.name) + " ");
        },
      }));
  }, [trig, commands, files, atName, atDir, replaceTrigger, onQuickPick, t]);

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLTextAreaElement>): boolean => {
      if (!trig) return false;
      if (e.key === "Escape") {
        e.preventDefault();
        dismissedRef.current = `${trig.kind}:${trig.start}`;
        close();
        return true;
      }
      if (!rows.length) return false;
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSel((i) => (i + 1) % rows.length);
        return true;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setSel((i) => (i - 1 + rows.length) % rows.length);
        return true;
      }
      // Enter 在菜单开着时归菜单 —— 否则选个文件把半截话发出去了
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        rows[Math.min(sel, rows.length - 1)].pick();
        return true;
      }
      return false;
    },
    [trig, rows, sel, close],
  );

  // 失焦关菜单：点别处却留一个浮层挂在那儿，会挡住下面的内容
  const onBlur = useCallback(() => {
    setTimeout(close, 120); // 让点击项先被处理
  }, [close]);

  let menu: ReactNode = null;
  if (trig?.kind === "at" && !workspace) {
    // 诚实的空态：说清楚为什么没东西可选，别摆一个空菜单让人以为坏了
    menu = (
      <div className="absolute bottom-full left-0 right-0 mb-1.5 z-30 rounded-card border border-white/[0.10] bg-bg-1 shadow-lg px-3 py-2 text-[11.5px] text-ink-3">
        {t("先在上面选一个工作文件夹，@ 才能列出里面的文件")}
      </div>
    );
  } else if (trig && (rows.length > 0 || listErr)) {
    menu = (
      <div className="absolute bottom-full left-0 right-0 mb-1.5 z-30 rounded-card border border-white/[0.10] bg-bg-1 shadow-lg py-1 max-h-64 overflow-y-auto">
        {listErr ? (
          <div className="px-3 py-2 text-[11.5px] text-ink-4">{listErr}</div>
        ) : (
          rows.map((r, i) => (
            <button
              key={r.key}
              onMouseDown={(ev) => {
                ev.preventDefault(); // 别让 textarea 先失焦
                r.pick();
              }}
              onMouseEnter={() => setSel(i)}
              className={cn(
                "w-full flex items-center gap-2 px-3 py-1.5 text-left text-[12px]",
                i === sel ? "bg-accent/[0.14] text-ink-0" : "text-ink-2 hover:bg-white/[0.04]",
              )}
            >
              <r.icon size={13} className={cn("shrink-0", i === sel ? "text-accent" : "text-ink-4")} />
              <span className="truncate flex-1">{r.label}</span>
              {r.hint && <span className="shrink-0 text-[10.5px] text-ink-5 truncate max-w-[40%]">{r.hint}</span>}
              {i === sel && <CornerDownLeft size={11} className="shrink-0 text-ink-5" />}
            </button>
          ))
        )}
      </div>
    );
  }

  return { menu, onKeyDown, onBlur };
}
