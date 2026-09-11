/**
 * Local OpenTu host.  The iframe is intentionally only a canvas surface: all
 * state and paid work goes through the narrow ActionParity bridge below.
 */
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Clapperboard, Download, Expand, ExternalLink, ImagePlus, LoaderCircle, Minimize, MoreHorizontal, PanelTopOpen, Plus, RefreshCw, Trash2 } from "lucide-react";
import { AnchoredMenu } from "./components/AnchoredMenu";

type Envelope = { ok: boolean; result?: Record<string, unknown>; error?: { message?: string } };
type BridgeRequest = {
  type?: string;
  request_id?: string;
  input?: Record<string, unknown>;
};
type ComponentStatus = { state?: "not_installed" | "installed" | "damaged"; detail?: string; bundle_id?: string };
type ComponentOffer = { available?: boolean; archive_bytes?: number; bundle_id?: string; error?: string };
const LAST_PROJECT_KEY = "uking.creator.last_project_id";
async function readLocalAssetAsDataUrl(assetUrl: string, baseUrl: string, sessionCapability: string): Promise<string> {
  const response = await fetch(new URL(assetUrl, baseUrl).toString(), {
    headers: { "X-Uking-Capability": sessionCapability },
  });
  if (!response.ok) throw new Error("本地图片素材读取失败");
  const blob = await response.blob();
  return await new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error("本地图片素材转换失败"));
    reader.onload = () => typeof reader.result === "string" ? resolve(reader.result) : reject(new Error("本地图片素材为空"));
    reader.readAsDataURL(blob);
  });
}
async function action(id: string, input: Record<string, unknown> = {}, confirmed = false): Promise<Record<string, unknown>> {
  const response = await invoke<Envelope>("action_parity_call", { request: { action_id: id, input, confirmed, surface: "gui" } });
  if (!response.ok) throw new Error(response.error?.message || "本地画布操作失败");
  return response.result || {};
}

export function CreatorCanvas({ onToast, onGoDraw, onGoVideo }: {
  onToast: (message: string) => void;
  onGoDraw?: () => void;
  onGoVideo?: () => void;
}) {
  const [url, setUrl] = useState<string>();
  const [capability, setCapability] = useState<string>();
  const [projectId, setProjectId] = useState<string>();
  const [projectTitle, setProjectTitle] = useState<string>();
  const [startupError, setStartupError] = useState<string>();
  const frame = useRef<HTMLIFrameElement>(null);
  const canvasContainer = useRef<HTMLDivElement>(null);
  const [prompt, setPrompt] = useState("");
  const [busy, setBusy] = useState(false);
  const [taskStatus, setTaskStatus] = useState<string>();
  const [projects, setProjects] = useState<{ id: string; title: string }[]>([]);
  const [saveError, setSaveError] = useState<string>();
  const [saving, setSaving] = useState(false);
  const [component, setComponent] = useState<ComponentStatus>();
  const [offer, setOffer] = useState<ComponentOffer>();
  const [checkingComponent, setCheckingComponent] = useState(true);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [managementOpen, setManagementOpen] = useState(false);
  const [advancedMaintenanceOpen, setAdvancedMaintenanceOpen] = useState(false);
  const [maintenanceConfirmation, setMaintenanceConfirmation] = useState<"reinstall" | "uninstall">();
  const [materialGeneratorOpen, setMaterialGeneratorOpen] = useState(false);
  const managementButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const syncFullscreen = () => setIsFullscreen(document.fullscreenElement === canvasContainer.current);
    document.addEventListener("fullscreenchange", syncFullscreen);
    return () => document.removeEventListener("fullscreenchange", syncFullscreen);
  }, []);

  const refreshComponent = async (): Promise<{ component: ComponentStatus; offer: ComponentOffer } | undefined> => {
    setCheckingComponent(true);
    try {
      const inspected = await action("runtime.creator.component.inspect");
      const nextComponent = (inspected.component || {}) as ComponentStatus;
      const nextOffer = (inspected.offer || {}) as ComponentOffer;
      setComponent(nextComponent);
      setOffer(nextOffer);
      const blockers = inspected.blockers as string[] | undefined;
      if (blockers?.length) setStartupError(blockers[0]);
      return { component: nextComponent, offer: nextOffer };
    } catch (error) {
      setStartupError(error instanceof Error ? error.message : String(error));
    } finally { setCheckingComponent(false); }
  };

  const start = async ({ createNew = false, openId }: { createNew?: boolean; openId?: string } = {}) => {
    setBusy(true);
    setStartupError(undefined);
    try {
      const started = await action("runtime.creator.canvas.start", {}, true);
      setUrl(String(started.url));
      setCapability(String(started.capability));
      const listed = await action("runtime.creator.project.list");
      setProjects(listed.projects as { id: string; title: string }[] || []);
      const remembered = openId || (createNew ? undefined : window.localStorage.getItem(LAST_PROJECT_KEY));
      if (remembered) {
        // A remembered project is evidence of user work. If it is missing or
        // damaged, stop visibly instead of silently creating an empty canvas.
        const opened = await action("runtime.creator.project.inspect", { project_id: remembered });
        window.localStorage.setItem(LAST_PROJECT_KEY, String(opened.id));
        setSaveError(undefined);
        setProjectId(String(opened.id));
        setProjectTitle(String(opened.title || "未命名画布"));
        return;
      }
      const created = await action("runtime.creator.project.create", { title: "未命名画布" }, true);
      const id = String(created.id);
      window.localStorage.setItem(LAST_PROJECT_KEY, id);
      setSaveError(undefined);
      setProjectId(id);
      setProjects(current => [...current, { id, title: String(created.title || "未命名画布") }]);
      setProjectTitle(String(created.title || "未命名画布"));
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setStartupError(message);
      onToast(message);
    }
    finally { setBusy(false); }
  };
  useEffect(() => { void refreshComponent(); }, []);
  const initializeBridge = () => {
    if (!url || !capability || !projectId) return;
    // The capability lives only in this React state and this one postMessage;
    // it is not an URL parameter, log line, or browser-storage value.
    frame.current?.contentWindow?.postMessage({ type: "uking:bridge:init", projectId }, new URL(url).origin);
  };

  useEffect(() => {
    if (!url || !projectId) return;
    const origin = new URL(url).origin;
    const reply = (requestId: string | undefined, body: Record<string, unknown>) => {
      frame.current?.contentWindow?.postMessage({ type: "uking:bridge:result", request_id: requestId, ...body }, origin);
    };
    const onMessage = (event: MessageEvent<BridgeRequest>) => {
      // The iframe is a replaceable web surface. It may request only project
      // reads/saves for the project the host created; it cannot supply an
      // Action ID, confirmation, or ActionParity execution ID.
      if (event.origin !== origin || event.source !== frame.current?.contentWindow) return;
      const request = event.data;
      if (request?.type === "uking:bridge:saving") { setSaving(true); return; }
      if (request?.type === "uking:bridge:save-error") {
        setSaving(false);
        setSaveError("画布尚未保存，请保留此页面并重试保存。");
        return;
      }
      if (request?.type === "uking:bridge:saved") { setSaveError(undefined); setSaving(false); return; }
      if (request?.type === "uking:bridge:ready") {
        initializeBridge();
        return;
      }
      const actionId = request?.type === "uking:bridge:project.inspect"
        ? "runtime.creator.project.inspect"
        : request?.type === "uking:bridge:project.save"
          ? "runtime.creator.project.save"
          : undefined;
      if (!actionId || !request?.input || request.input.project_id !== projectId) return;
      const input = request.input;
      void action(actionId, input, actionId === "runtime.creator.project.save")
        .then((result) => reply(request.request_id, { ok: true, result }))
        .catch((error) => reply(request.request_id, { ok: false, error: error instanceof Error ? error.message : String(error) }));
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [url, projectId, capability]);

  const waitForTask = async (taskId: string) => {
    if (!projectId) return;
    for (let attempt = 0; attempt < 30; attempt += 1) {
      await new Promise((resolve) => window.setTimeout(resolve, 300));
      const inspected = await action("runtime.creator.image.inspect", { project_id: projectId, task_id: taskId });
      const task = inspected.task as { status?: string; error?: string; result?: { asset?: { url?: string } } } | undefined;
      const status = task?.status || "unknown";
      setTaskStatus(status);
      if (status === "completed") {
        const asset = task?.result?.asset;
        if (asset?.url && url && capability) {
          // The asset route requires the session header. Convert it to an
          // in-memory data URL in the trusted host so the iframe never learns
          // the capability and the saved canvas survives a reload.
          const dataUrl = await readLocalAssetAsDataUrl(asset.url, url, capability);
          frame.current?.contentWindow?.postMessage({ type: "uking:bridge:image.insert", asset: { url: dataUrl } }, new URL(url).origin);
        }
        onToast("图片已写入本地项目并插入画布");
        return;
      }
      if (status === "failed" || status === "pending-verify" || status === "not_configured") {
        onToast(task?.error || `图片任务处于 ${status}，未自动重投`);
        return;
      }
    }
    setTaskStatus("pending-verify");
    onToast("图片任务等待超时；请检查任务，未自动重投");
  };

  const generate = async () => {
    if (!projectId || !prompt.trim()) return;
    setBusy(true);
    try {
      const result = await action("runtime.creator.image.submit", { project_id: projectId, prompt, model: "gpt-image-2", size: "1024x1024", quality: "medium" }, true);
      const taskId = String(result.task_id || "");
      setTaskStatus(String(result.status || "pending"));
      if (!taskId) throw new Error("本地图片任务未返回 task_id");
      if (result.status === "not_configured") {
        onToast(String(result.error || "图片生成尚未配置"));
        return;
      }
      onToast(`图片任务已排队：${taskId}`);
      setPrompt("");
      void waitForTask(taskId);
    } catch (error) { onToast(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(false); }
  };

  const hasAvailableUpdate = component?.state === "installed"
    && offer?.available === true
    && Boolean(component.bundle_id)
    && Boolean(offer.bundle_id)
    && component.bundle_id !== offer.bundle_id;

  const clearCanvasSurface = () => {
    setUrl(undefined);
    setCapability(undefined);
    setProjectId(undefined);
    setProjectTitle(undefined);
  };

  const installComponent = async ({ replacing = false }: { replacing?: boolean } = {}) => {
    if (busy || saving || saveError) {
      if (saveError) onToast("画布尚未保存，请先修复保存问题后再更新组件。");
      return;
    }
    setBusy(true);
    setStartupError(undefined);
    try {
      if (replacing) {
        // A running listener has already resolved the old static root. Stop it
        // before promoting a new version so the restarted iframe uses exactly
        // the verified catalogue entry that the user selected.
        await action("runtime.creator.canvas.stop", {}, true);
        clearCanvasSurface();
      }
      await action("runtime.creator.component.install", {}, true);
      const refreshed = await refreshComponent();
      await start();
      if (replacing) onToast(`画布已更新${refreshed?.component.bundle_id ? `为 ${refreshed.component.bundle_id}` : ""}；创作项目已保留。`);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setStartupError(message);
      onToast(message);
    } finally { setBusy(false); }
  };

  const uninstallComponent = async () => {
    // Uninstalling clears the iframe. A failed save means that iframe may be
    // the only remaining copy of the user's work, so enforce this guard here.
    if (busy || saving || saveError) {
      if (saveError) onToast("画布尚未保存，请先修复保存问题后再卸载组件。");
      return;
    }
    setBusy(true);
    try {
      // The component action refuses to delete while the loopback service is
      // serving this version. Stop it first; an already-stopped service is a
      // safe no-op, and the project data remains outside the component root.
      await action("runtime.creator.canvas.stop", {}, true);
      clearCanvasSurface();
      await action("runtime.creator.component.uninstall", {}, true);
      await refreshComponent();
      setManagementOpen(false);
      setMaintenanceConfirmation(undefined);
      onToast("本地画布组件已卸载；你的创作项目仍保留在本机。");
    } catch (error) { onToast(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(false); }
  };

  const reinstallComponent = async () => {
    if (busy || saving || saveError) {
      if (saveError) onToast("画布尚未保存，请先修复保存问题后再重新安装组件。");
      return;
    }
    setBusy(true);
    setStartupError(undefined);
    try {
      // Reinstall is the same maintenance sequence: release the listener
      // before replacing the optional component, never the customer project.
      await action("runtime.creator.canvas.stop", {}, true);
      clearCanvasSurface();
      await action("runtime.creator.component.uninstall", {}, true);
      await action("runtime.creator.component.install", {}, true);
      const refreshed = await refreshComponent();
      await start();
      setManagementOpen(false);
      setMaintenanceConfirmation(undefined);
      onToast(`画布已重新安装${refreshed?.component.bundle_id ? `：${refreshed.component.bundle_id}` : ""}；创作项目已保留。`);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setStartupError(message);
      onToast(message);
    } finally { setBusy(false); }
  };

  const checkForComponentUpdate = async () => {
    const refreshed = await refreshComponent();
    if (!refreshed?.offer.available || !refreshed.offer.bundle_id) {
      onToast("当前没有可用的画布更新。");
    } else if (refreshed.component.state === "installed" && refreshed.component.bundle_id === refreshed.offer.bundle_id) {
      onToast("画布已经是最新版本。");
    } else if (refreshed.component.state === "installed") {
      onToast(`发现画布更新：${refreshed.offer.bundle_id}`);
    } else {
      onToast(`可安装画布版本：${refreshed.offer.bundle_id}`);
    }
  };

  const toggleFullscreen = async () => {
    try {
      if (document.fullscreenElement === canvasContainer.current) {
        await document.exitFullscreen();
      } else if (canvasContainer.current?.requestFullscreen) {
        await canvasContainer.current.requestFullscreen();
      } else {
        onToast("当前设备不支持全屏创作。");
      }
    } catch (error) { onToast(`切换全屏失败：${error instanceof Error ? error.message : String(error)}`); }
  };

  const canvasUrl = url ? (() => {
    const localCanvasUrl = new URL(url);
    // This mode bit is intentionally the only iframe URL addition. It makes
    // OpenTu disable its upstream workspace persistence before bridge init.
    localCanvasUrl.searchParams.set("uking_host", "1");
    return localCanvasUrl.toString();
  })() : undefined;

  return <div ref={canvasContainer} className={`flex min-h-0 flex-1 flex-col gap-3 ${isFullscreen ? "h-screen w-screen bg-[#161616] p-4" : ""}`}>
    <div className="flex flex-wrap items-center gap-2 rounded-xl border border-white/[0.08] bg-white/[0.03] p-3">
      <PanelTopOpen size={16} className="text-accent" />
      <span className="mr-auto text-sm font-medium">{projectTitle ? `本地创作画布 · ${projectTitle}` : "本地创作画布"}</span>
      {component?.state === "installed" &&
      <select aria-label="打开本地项目" value={projectId || ""} disabled={busy || saving || Boolean(saveError)} onChange={event => void start({ openId: event.target.value })} className="rounded-lg border border-white/[0.1] bg-black/20 px-2 py-2 text-sm"><option value="" disabled>选择项目</option>{projects.map(project => <option key={project.id} value={project.id}>{project.title} · {project.id.slice(-6)}</option>)}</select>
      }
      {component?.state === "installed" &&
      <button onClick={() => void start({ createNew: true })} disabled={busy || saving || Boolean(saveError)} className="inline-flex items-center gap-1.5 rounded-lg border border-white/[0.1] px-3 py-2 text-sm text-ink-2 hover:bg-white/[0.06] disabled:opacity-50"><Plus size={15} />新建项目</button>
      }
      {component?.state === "installed" && <button type="button" onClick={() => void toggleFullscreen()} disabled={busy || saving || Boolean(saveError)} className="inline-flex items-center gap-1.5 rounded-lg border border-white/[0.1] px-3 py-2 text-sm text-ink-2 hover:bg-white/[0.06] disabled:opacity-50">{isFullscreen ? <Minimize size={15} /> : <Expand size={15} />}{isFullscreen ? "退出大屏" : "大屏创作"}</button>}
      {component?.state === "installed" && <button type="button" onClick={() => setMaterialGeneratorOpen((open) => !open)} aria-expanded={materialGeneratorOpen} disabled={busy || saving || Boolean(saveError)} className="inline-flex items-center gap-1.5 rounded-lg border border-white/[0.1] px-3 py-2 text-sm text-ink-3 hover:bg-white/[0.06] hover:text-ink-1 disabled:opacity-50"><ImagePlus size={15} />生成素材</button>}
      {(component?.state === "installed" || component?.state === "damaged") && !checkingComponent && <button ref={managementButton} type="button" onClick={() => setManagementOpen((open) => !open)} aria-expanded={managementOpen} aria-label="画布管理" title="画布管理" className="inline-flex items-center gap-1.5 rounded-lg border border-white/[0.1] px-3 py-2 text-sm text-ink-3 hover:bg-white/[0.06] hover:text-ink-1"><MoreHorizontal size={16} />画布管理</button>}
    </div>
    {managementOpen && <AnchoredMenu anchorRef={managementButton} onClose={() => setManagementOpen(false)} minWidth={356}>
      <section className="space-y-3 p-3 text-sm text-ink-2">
        <div>
          <h2 className="font-medium text-ink-1">画布管理</h2>
          <p className="mt-1 text-xs leading-5 text-ink-4">这里只维护本机画布运行组件；你的项目、已保存画布和图片不在组件目录内。</p>
        </div>
        <dl className="space-y-1 rounded-lg bg-white/[0.04] p-2 text-xs">
          <div className="flex gap-3"><dt className="w-16 shrink-0 text-ink-4">当前状态</dt><dd className="min-w-0 break-all text-ink-2">{component?.state === "installed" ? "已安装并已校验" : component?.state === "damaged" ? "需要维护" : "未安装"}</dd></div>
          <div className="flex gap-3"><dt className="w-16 shrink-0 text-ink-4">已装版本</dt><dd className="min-w-0 break-all text-ink-2">{component?.bundle_id || "—"}</dd></div>
          <div className="flex gap-3"><dt className="w-16 shrink-0 text-ink-4">可用版本</dt><dd className="min-w-0 break-all text-ink-2">{offer?.available ? offer.bundle_id || "已发布" : "暂未发布"}</dd></div>
        </dl>
        <div className="flex flex-wrap gap-2">
          <button type="button" onClick={() => void checkForComponentUpdate()} disabled={busy || checkingComponent} className="inline-flex items-center gap-1.5 rounded-lg border border-white/[0.1] px-2.5 py-1.5 text-xs text-ink-2 hover:bg-white/[0.06] disabled:opacity-50"><RefreshCw size={14} className={checkingComponent ? "animate-spin" : ""} />检查更新</button>
          {hasAvailableUpdate && <button type="button" onClick={() => { setManagementOpen(false); void installComponent({ replacing: true }); }} disabled={busy || saving || Boolean(saveError)} className="inline-flex items-center gap-1.5 rounded-lg bg-accent px-2.5 py-1.5 text-xs font-medium text-white disabled:opacity-50"><Download size={14} />更新画布</button>}
        </div>
        <div className="border-t border-white/[0.08] pt-3">
          <button type="button" onClick={() => setAdvancedMaintenanceOpen((open) => !open)} aria-expanded={advancedMaintenanceOpen} className="text-xs text-ink-4 hover:text-ink-2">{advancedMaintenanceOpen ? "收起高级维护" : "高级维护"}</button>
          {advancedMaintenanceOpen && <div className="mt-2 space-y-2 rounded-lg border border-amber-400/20 bg-amber-400/[0.04] p-2.5 text-xs">
            <p className="leading-5 text-ink-3">重新安装或卸载只影响画布运行组件；创作项目会保留在本机。</p>
            <div className="flex flex-wrap gap-2">
              <button type="button" onClick={() => { setManagementOpen(false); setMaintenanceConfirmation("reinstall"); }} disabled={busy || saving || Boolean(saveError)} className="rounded-md border border-white/[0.1] px-2.5 py-1.5 text-ink-2 hover:bg-white/[0.06] disabled:opacity-50">重新安装画布</button>
              <button type="button" onClick={() => { setManagementOpen(false); setMaintenanceConfirmation("uninstall"); }} disabled={busy || saving || Boolean(saveError)} className="rounded-md border border-red-400/35 px-2.5 py-1.5 text-red-300 hover:bg-red-400/10 disabled:opacity-50"><Trash2 size={13} className="mr-1 inline" />卸载画布</button>
            </div>
          </div>}
        </div>
      </section>
    </AnchoredMenu>}
    {hasAvailableUpdate && <div role="status" className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-accent/25 bg-accent/[0.06] px-3 py-2.5 text-sm">
      <span className="text-ink-2">画布有可用更新：<strong className="font-medium text-ink-1">{offer?.bundle_id}</strong><span className="ml-1 text-xs text-ink-4">更新不会删除创作项目。</span></span>
      <button type="button" onClick={() => void installComponent({ replacing: true })} disabled={busy || saving || Boolean(saveError)} className="inline-flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-white disabled:opacity-50"><Download size={15} />更新画布</button>
    </div>}
    {component?.state === "installed" && materialGeneratorOpen && <section aria-label="生成素材" className="flex flex-wrap items-center gap-2 rounded-xl border border-white/[0.08] bg-white/[0.03] p-3">
      <div className="mr-auto min-w-36"><p className="text-sm font-medium text-ink-2">生成素材</p><p className="mt-0.5 text-xs text-ink-4">图片会插入当前画布。</p></div>
      <input value={prompt} onChange={(e) => setPrompt(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") e.preventDefault(); }} placeholder="描述想生成的图片…" className="min-w-52 flex-1 rounded-lg border border-white/[0.1] bg-black/20 px-3 py-2 text-sm outline-none focus:border-accent" disabled={busy} />
      <button type="button" onClick={() => void generate()} disabled={busy || !prompt.trim()} className="inline-flex items-center gap-1.5 rounded-lg bg-accent px-3 py-2 text-sm font-medium text-white disabled:opacity-50"><ImagePlus size={15} />生成并插入</button>
      {taskStatus && <span className="w-full text-xs text-ink-4">任务：{taskStatus}</span>}
    </section>}
    {isFullscreen && <p className="text-center text-xs text-ink-3">按 Esc 返回普通视图</p>}
    {saving && <span className="text-xs text-ink-3">正在保存…</span>}
    {saveError && <div role="alert" className="text-sm text-amber-400">{saveError}</div>}
    {checkingComponent ? <div className="grid flex-1 place-items-center text-ink-3"><LoaderCircle className="animate-spin" />正在检查本地画布组件…</div>
      : component?.state === "damaged" ? <div className="grid flex-1 place-items-center gap-3 rounded-xl border border-dashed border-white/[0.12] p-8 text-center text-ink-3"><div><p className="text-sm text-ink-2">本地画布组件需要维护。</p><p className="mt-1 text-xs">{component.detail || "请在画布管理中重新安装组件；你的创作项目会保留。"}</p></div><button type="button" onClick={() => setManagementOpen(true)} disabled={busy || saving || Boolean(saveError)} className="inline-flex items-center gap-1.5 rounded-lg border border-white/[0.1] px-3 py-2 text-sm text-ink-2 hover:bg-white/[0.06] disabled:opacity-50"><MoreHorizontal size={15} />打开画布管理</button>{saveError && <p role="alert" className="text-xs text-amber-400">请先修复保存问题后再维护。</p>}{startupError && <p role="alert" className="text-xs text-amber-400">{startupError}</p>}</div>
      : component?.state !== "installed" ? <div className="grid flex-1 place-items-center gap-3 rounded-xl border border-dashed border-white/[0.12] p-8 text-center text-ink-3"><div><p className="text-sm text-ink-2">本地创作画布是可选下载组件，安装后仅在本机运行。</p><p className="mt-1 text-xs">{offer?.available ? `下载大小：${Math.ceil((offer.archive_bytes || 0) / 1024 / 1024)} MB` : "组件发布包尚未就绪，请等待 U-King 更新组件目录。"}</p></div>{offer?.available && <button onClick={() => void installComponent()} disabled={busy} className="inline-flex items-center gap-1.5 rounded-lg bg-accent px-3 py-2 text-sm font-medium text-white disabled:opacity-50"><Download size={15} />下载并安装本地画布</button>}<div className="flex flex-wrap justify-center gap-2"><button type="button" onClick={onGoDraw} disabled={!onGoDraw} className="inline-flex items-center gap-1.5 rounded-lg border border-white/[0.1] px-3 py-2 text-sm font-medium text-ink-2 hover:bg-white/[0.06] disabled:opacity-50"><ImagePlus size={15} />AI 作图</button><button type="button" onClick={onGoVideo} disabled={!onGoVideo} className="inline-flex items-center gap-1.5 rounded-lg border border-white/[0.1] px-3 py-2 text-sm font-medium text-ink-2 hover:bg-white/[0.06] disabled:opacity-50"><Clapperboard size={15} />视频片段</button></div>{startupError && <p role="alert" className="text-xs text-amber-400">{startupError}</p>}</div>
      : busy && !url ? <div className="grid flex-1 place-items-center text-ink-3"><LoaderCircle className="animate-spin" />正在启动本地画布…</div> : canvasUrl && projectId ? <iframe key={projectId} ref={frame} onLoad={initializeBridge} title="OpenTu 本地创作画布" src={canvasUrl} className="min-h-0 flex-1 rounded-xl border border-white/[0.08] bg-white" sandbox="allow-scripts allow-same-origin allow-downloads" /> : <div className="grid flex-1 place-items-center gap-2 text-ink-3">{startupError ? `无法打开上次项目：${startupError}` : "本地画布尚未启动。"}<div className="flex gap-2"><button onClick={() => void start()} disabled={busy || saving || Boolean(saveError)} className="inline-flex items-center gap-1 text-accent disabled:opacity-50"><ExternalLink size={14} />打开本地画布</button><button onClick={() => void start({ createNew: true })} disabled={busy || saving || Boolean(saveError)} className="inline-flex items-center gap-1 text-accent disabled:opacity-50"><Plus size={14} />新建项目</button></div></div>}
    {maintenanceConfirmation && <div className="fixed inset-0 z-[80] grid place-items-center bg-black/60 p-4" onClick={() => !busy && setMaintenanceConfirmation(undefined)}>
      <div className="w-full max-w-md space-y-4 rounded-xl border border-white/[0.12] bg-bg-2 p-5 shadow-card" onClick={(event) => event.stopPropagation()}>
        <div>
          <h2 className="text-sm font-medium text-ink-1">{maintenanceConfirmation === "uninstall" ? "卸载本地画布？" : "重新安装本地画布？"}</h2>
          <p className="mt-2 text-sm leading-6 text-ink-3">{maintenanceConfirmation === "uninstall" ? "这会删除本机的画布运行组件。创作项目、已保存画布和图片会保留；再次使用画布时可重新下载安装。" : "这会停止并替换本机的画布运行组件。创作项目、已保存画布和图片会保留。"}</p>
        </div>
        <div className="flex justify-end gap-2">
          <button type="button" onClick={() => setMaintenanceConfirmation(undefined)} disabled={busy} className="rounded-lg border border-white/[0.1] px-3 py-2 text-sm text-ink-2 hover:bg-white/[0.06] disabled:opacity-50">取消</button>
          <button type="button" onClick={() => void (maintenanceConfirmation === "uninstall" ? uninstallComponent() : reinstallComponent())} disabled={busy} className={`rounded-lg px-3 py-2 text-sm font-medium text-white disabled:opacity-50 ${maintenanceConfirmation === "uninstall" ? "bg-red-500 hover:bg-red-400" : "bg-accent hover:brightness-110"}`}>{maintenanceConfirmation === "uninstall" ? "确认卸载" : "确认重新安装"}</button>
        </div>
      </div>
    </div>}
  </div>;
}
