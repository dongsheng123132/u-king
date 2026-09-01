/**
 * Redline 的主入口组件 —— 宿主只需要传 host + 文件路径，其余（格式识别、懒加载 viewer、
 * unit 切换）全部自己搞定。这是"通用可插拔"落地的地方：
 * opencodex 的 FilesPanel 挂这个组件，以后任何宿主也是挂这一个组件，不用重新接线。
 */
import { Suspense, useCallback, useEffect, useMemo, useState } from "react";
import type { RedlineDocument, RedlineFormat, RedlineUnit } from "./document-model";
import type { RedlineHost } from "./host-adapter";
import { detectFormat, loadViewer } from "./viewers/registry";
import { UnitSwitcher } from "./UnitSwitcher";
import { useI18n } from "../../i18n";

export interface RedlinePanelProps {
  host: RedlineHost;
  /** 文件绝对路径。 */
  path: string;
  /** 展示用的文件名（一般就是 path 的 basename，交由调用方决定怎么截）。 */
  fileName: string;
}

/** 这些格式我们**自己渲染得不好**，宿主要是能转 PDF 就该走那条路。
 *
 *  - `.pptx`/`.ppt` → 现在只能抽出文字大纲，而客户想看的恰恰是「它长什么样」
 *  - `.doc`/`.odt` → 老二进制 / 非 OOXML，mammoth 一个字都解不出来
 *
 *  **`.docx`/`.xlsx`/`.xls` 故意不在这** —— 那三个 mammoth/SheetJS 渲染得又快又好、
 *  文字还能选中，换成 PDF 是**降级**，而且要多等十几秒。 */
const WORTH_PDF = ["pptx", "ppt", "doc", "odt", "odp"];
function worthRenderingToPdf(fileName: string): boolean {
  return WORTH_PDF.includes(fileName.split(".").pop()?.toLowerCase() ?? "");
}

export function RedlinePanel({ host, path, fileName }: RedlinePanelProps) {
  const { t } = useI18n();
  const rawFormat = useMemo(() => detectFormat(fileName), [fileName]);
  const [bytes, setBytes] = useState<ArrayBuffer | null>(null);
  const [doc, setDoc] = useState<RedlineDocument | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [activeUnitId, setActiveUnitId] = useState<string>("");
  /** 实际用来渲染的格式和取字节的路径。转成 PDF 后这两个会指向缓存里的 PDF，
   *  但 `doc.docId` / `sourcePath` 始终是**原文件** —— 对外身份认原件，
   *  临时 PDF 只是渲染中转站。 */
  const [render, setRender] = useState<{ format: RedlineFormat; loadPath: string }>({ format: rawFormat, loadPath: path });
  /** 正在借 LibreOffice 转 —— 冷启动十几秒是常事，不给个说法就是"点了没反应"。 */
  const [converting, setConverting] = useState(false);
  /** 该转但转不成（这台机器没装 LibreOffice）。**如实说出来**，别让客户以为
   *  「这软件预览 PPT 就是只有文字」——那是我们的短板，不是他的文件有问题。 */
  const [noRenderer, setNoRenderer] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setBytes(null);
    setDoc(null);
    setErr(null);
    setActiveUnitId("");
    setNoRenderer(false);
    setConverting(false);
    (async () => {
      let format = rawFormat;
      let loadPath = path;
      // 能转就先转：转成了走 PdfViewer 出真版式，转不成/没装安静退回原来的档。
      // **绝不因为转换失败就让预览整个失败** —— 大纲档虽然弱，但比一片红字强。
      if (host.renderToPdf && worthRenderingToPdf(fileName)) {
        setConverting(true);
        try {
          const pdf = await host.renderToPdf(path);
          if (cancelled) return;
          if (pdf) {
            format = "pdf";
            loadPath = pdf;
          } else {
            setNoRenderer(true);
          }
        } catch {
          setNoRenderer(true); // 转换报错也退回大纲，理由同上
        }
        if (cancelled) return;
        setConverting(false);
      }
      try {
        const b = await host.readFileBytes(loadPath);
        if (cancelled) return;
        setBytes(b);
        setRender({ format, loadPath });
        setDoc({ docId: path, sourcePath: path, format, units: [] });
      } catch (e) {
        if (!cancelled) setErr(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [host, path, rawFormat, fileName]);

  const format = render.format;

  const handleUnitsResolved = useCallback((units: RedlineUnit[]) => {
    setDoc((d) => (d ? { ...d, units } : d));
    setActiveUnitId((cur) => cur || units[0]?.id || "");
  }, []);

  const Viewer = useMemo(() => loadViewer(format), [format]);

  const openExternal = host.openExternal ? () => host.openExternal!(path) : undefined;

  if (err) {
    return <div className="p-4 text-red-400 text-[12px]">{t("打开失败：{err}", { err })}</div>;
  }
  if (converting) {
    return (
      <div className="p-4 text-gray-400 text-[12px]">
        {t("正在渲染真实版式（借本机 LibreOffice，首次约十几秒）…")}
      </div>
    );
  }
  if (!bytes || !doc) {
    return <div className="p-4 text-gray-400 text-[12px]">{t("加载中…")}</div>;
  }

  return (
    // select-text：globals.css 给 body 设了 user-select:none（防误选整个界面），
    // 预览的正文是**要给人复制**的，必须显式要回来 —— 只给这棵子树，不动全站。
    <div className="flex flex-col h-full min-h-0 select-text">
      {noRenderer && (
        <div className="shrink-0 px-3 py-1.5 text-[11px] text-warning-700 dark:text-warning-400 bg-amber-500/[0.08] border-b border-amber-500/20">
          {t("这台电脑没装 LibreOffice，只能看文字大纲。装上它（工具箱 →「厨具工具箱」）就能看到真实版式。")}
        </div>
      )}
      {doc.units.length > 1 && (
        <UnitSwitcher units={doc.units} activeUnitId={activeUnitId} onChange={setActiveUnitId} />
      )}
      <Suspense fallback={<div className="p-4 text-gray-400 text-[12px]">{t("渲染中…")}</div>}>
        <Viewer
          doc={doc}
          bytes={bytes}
          srcUrl={host.getSrcUrl?.(render.loadPath)}
          activeUnitId={activeUnitId}
          onUnitsResolved={handleUnitsResolved}
          renderMarkdown={host.renderMarkdown}
          openExternal={openExternal}
        />
      </Suspense>
    </div>
  );
}
