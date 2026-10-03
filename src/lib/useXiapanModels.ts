/**
 * `useXiapanModels()` —— 虾盘云「换模型」下拉用的模型清单，**内嵌版先上、线上更新版到了就换**。
 *
 * 为什么不是直接用 `XIAPAN_MODELS`：那是编译进程序的内嵌版，模型下线 / 新增要等发版才到客户手上
 * （2026-10-03 实测：下拉里挂着上游已下线的 gemini-3.5-flash，客户点了只会收到一句 "no longer available"）。
 * 后端 `model_catalog.rs` 启动后在后台拉线上 `xiapan-models.json`，版本比内嵌大才采用；这里挂载时问一次
 * 只读动作 `runtime.model_catalog.inspect`，拿到更新版就换上，**任何失败都继续用内嵌版**（纯 vite 预览没有
 * Tauri、后端没起来、返回的数据形状不对……都一样），下拉永远不会因为这一步变空。
 *
 * 同一时刻多个下拉组件一起挂载共享同一次请求；拿到更新版后记在模块里。「没有更新版」和失败都不记 ——
 * 后端的线上刷新在启动后几秒才跑完，下次再打开页面时重问一次（只读、本地、很便宜）就能拿到。
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ACTION, createTauriActionClient } from "../generated/action-client";
import { XIAPAN_CATALOG_VERSION, XIAPAN_MODELS, parseModelGroups, type ModelGroup } from "./models";

export type XiapanModelsView = {
  groups: ModelGroup[];
  /** embedded = 编进程序的；cache / online = 后端给出的更新版 */
  source: "embedded" | "cache" | "online";
  version: number;
};

const EMBEDDED_VIEW: XiapanModelsView = { groups: XIAPAN_MODELS, source: "embedded", version: XIAPAN_CATALOG_VERSION };

const actionClient = createTauriActionClient((command, args) => invoke(command, args), { surface: "gui:model-catalog" });

let settled: XiapanModelsView | null = null;
let inflight: Promise<XiapanModelsView | null> | null = null;

/** 问后端一次。只有「合法、非空、版本严格大于内嵌」才返回更新版，其余一律 null（= 用内嵌）。 */
async function fetchUpdated(): Promise<XiapanModelsView | null> {
  try {
    const envelope = await actionClient(ACTION.RUNTIME_MODEL_CATALOG_INSPECT, {});
    if (!envelope.ok) return null;
    const r = envelope.result as { version?: unknown; source?: unknown; groups?: unknown };
    if (typeof r.version !== "number" || r.version <= XIAPAN_CATALOG_VERSION) return null;
    const groups = parseModelGroups(r.groups);
    if (!groups.length) return null;
    const source = r.source === "cache" || r.source === "online" ? r.source : "embedded";
    return { groups, source, version: r.version };
  } catch {
    return null;
  }
}

export function useXiapanModels(): XiapanModelsView {
  const [view, setView] = useState<XiapanModelsView>(settled ?? EMBEDDED_VIEW);
  useEffect(() => {
    if (settled) return;
    let alive = true;
    inflight ??= fetchUpdated().finally(() => {
      inflight = null;
    });
    void inflight.then((v) => {
      if (v) settled = v;
      if (alive && v) setView(v);
    });
    return () => {
      alive = false;
    };
  }, []);
  return view;
}
