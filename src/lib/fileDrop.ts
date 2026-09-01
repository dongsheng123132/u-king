/**
 * 全站文件拖放（统一走 Tauri 原生拖放事件）——终端 / 作图 / 视频共用一套。
 *
 * 为什么这样做：内嵌终端是 webview 里的 xterm，OS 拖文件进来 HTML5 拿不到**真实路径**。
 * 把 `dragDropEnabled` 设为 true 后，webview 的 HTML5 拖放整体失效、改由 Tauri 原生事件
 * 送来真实文件路径（`event.payload.paths`）。本模块只在一处装一个全局监听，按落点命中
 * 已注册的「拖放区」派发路径 —— 各业务组件只需 `useDropZone(onDrop)` 拿到 ref + 悬停高亮，
 * 逻辑集中、可插拔（删一个组件不影响其它）。
 */
import { useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";

export type DropHandler = (paths: string[]) => void;

/** 把拖入的文件/文件夹路径拼成可插入输入框/终端的一段文本：含空格的路径加引号，
 *  末尾留一个空格便于接着敲。终端、工作台对话共用同一套（复用不复制）。 */
export function pathsToText(paths: string[]): string {
  return paths.map((p) => (/\s/.test(p) ? `"${p}"` : p)).join(" ") + " ";
}

type Zone = {
  el: HTMLElement;
  onDrop: DropHandler;
  setOver?: (over: boolean) => void;
};

const zones: Zone[] = [];
let started = false;

/** 落点（物理像素）落在哪个已注册区里 —— 换算成 CSS 像素后与元素矩形比对。
 *  后注册的（通常更内层）优先，返回最后一个命中的区。 */
function zoneAt(px: number, py: number): Zone | null {
  const dpr = window.devicePixelRatio || 1;
  const x = px / dpr;
  const y = py / dpr;
  let hit: Zone | null = null;
  for (const z of zones) {
    if (!z.el.isConnected) continue;
    const r = z.el.getBoundingClientRect();
    if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) hit = z;
  }
  return hit;
}

function clearOver(except?: Zone) {
  for (const z of zones) {
    if (z !== except) z.setOver?.(false);
  }
}

/** 全局只装一次（App 启动时调）。监听 Tauri 拖放：over 更新悬停高亮，drop 派发真实路径。 */
export function startFileDrop() {
  if (started) return;
  started = true;
  void getCurrentWebview().onDragDropEvent((event) => {
    const pl = event.payload as { type: string; paths?: string[]; position?: { x: number; y: number } };
    if (pl.type === "over" || pl.type === "enter") {
      const z = pl.position ? zoneAt(pl.position.x, pl.position.y) : null;
      clearOver(z ?? undefined);
      z?.setOver?.(true);
    } else if (pl.type === "leave") {
      clearOver();
    } else if (pl.type === "drop") {
      clearOver();
      if (!pl.paths?.length || !pl.position) return;
      zoneAt(pl.position.x, pl.position.y)?.onDrop(pl.paths);
    }
  });
}

/** 注册一个拖放区。返回注销函数。 */
export function registerDropZone(el: HTMLElement, onDrop: DropHandler, setOver?: (over: boolean) => void): () => void {
  const z: Zone = { el, onDrop, setOver };
  zones.push(z);
  return () => {
    const i = zones.indexOf(z);
    if (i >= 0) zones.splice(i, 1);
  };
}

/** React 组件用：把返回的 ref 挂到拖放容器上；`over` 为拖拽悬停中（可做高亮）。 */
export function useDropZone<T extends HTMLElement = HTMLDivElement>(onDrop: DropHandler) {
  const ref = useRef<T | null>(null);
  const [over, setOver] = useState(false);
  const onDropRef = useRef(onDrop);
  onDropRef.current = onDrop;
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    return registerDropZone(el, (paths) => onDropRef.current(paths), setOver);
  }, []);
  return { ref, over };
}
