/**
 * 技术支持 / 报告问题 —— 明确的求助入口（侧栏底部常驻可达）。
 * 2026-08-16 由「意见反馈」改名为「技术支持」：客户来这儿多半不是来提建议的，
 * 是**用不了了**。名字叫「反馈」会让人觉得说了也没人管，于是转头去骂而不是来找我们。
 *
 * 三条路，任选：
 *  ① 一键提交（走服务端上报，自动带**脱敏**诊断日志，作者能收到 → 建 Issue）；
 *  ② 发邮件给作者 hefangsheng@gmail.com（mailto 预填件名 + 脱敏摘要；日志走「打开日志文件夹」手动附）；
 *  ③ 复制脱敏诊断 / 打开日志文件夹（自己贴到微信/邮件）。
 *
 * 铁律：所有外发内容**先脱敏**（后端 feedback.rs::desensitize 收口，抹 Key/Token/邮箱/用户名）。
 * 纯前端组件，只靠 props（onToast）通信；后端命令 submit_feedback / collect_diagnostics / open_log_dir。
 */
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Bug,
  ClipboardCopy,
  Download,
  ExternalLink,
  FolderOpen,
  ImagePlus,
  LifeBuoy,
  Loader2,
  Mail,
  MessageCircle,
  MonitorSmartphone,
  QrCode,
  ScreenShare,
  Send,
  ShieldCheck,
  X,
} from "lucide-react";
import { useI18n } from "./i18n";

/** 远程协助状态（后端 remote_assist.rs::AssistStatus 镜像）。 */
type AssistStatus = {
  running: boolean;
  device_id?: string | null;
  remaining_secs?: number | null;
  audit_log: string;
  supported: boolean;
};

/** UU远程（网易官方远控）状态 —— 影核动作 `runtime.uu_remote.inspect` 的输出镜像。
 *
 *  两个字段别混：`ready` = **客户现在能不能接受屏幕协助**（装了就能）；
 *  `can_auto_install` = **我们能不能替他装**（只有 Windows）。Mac 上后者 false 前者仍可能 true。
 *  `portable_available` 由后端给：官方只发安装包、没有绿色版，这个事实存后端一处，
 *  前端照着渲染，免得各页凭印象写成「免安装」把客户骗进来。 */
type UuStatus = {
  installed: boolean;
  ready: boolean;
  blockers: string[];
  portable_available: boolean;
  can_auto_install: boolean;
  download_page: string;
};

/** 作者联系邮箱（后端 feedback.rs 注释同源；非密钥）。 */
const CONTACT_EMAIL = "hefangsheng@gmail.com";

/**
 * 售后微信。放在邮箱**前面**：客户是中文用户，加微信几秒钟，发邮件多半不会发。
 * 🔴 **两处在用，改一处不算改完**：这里，和 `Geo.tsx`（网站 GEO 体检的付费咨询入口）。
 * 2026-08-17 GEO 随 #419 下线时这里改成了「只剩这一处」，08-23 又整块恢复回来 ——
 * 所以这行注释在这两个日期之间是对的，现在不是了。要再加别的入口就从这儿引，别又抄一份。
 */
const CONTACT_WECHAT = "hecare888";

/**
 * 加好友二维码。源图来自 `u-claw.org/website/wechat-qr.jpg`（1083×1464 / 144KB），
 * 这里放的是压到 480 宽的版本（57KB）—— 弹窗最大也就显示到 ~280px，原图纯属白背 exe 体积。
 *
 * 🔴 **微信没有「点一下就加好友」这种东西。** 桌面版微信不接受用微信号发起加好友的深链
 * （`weixin://` 只能把 App 唤起来，加不了人），所以**二维码才是唯一能一步到位的路**：
 * 手机扫 → 直接到「添加朋友」。号码留着是给已经在电脑上开着微信的人搜索用的。
 */
import wechatQr from "./assets/wechat-qr.jpg";

/** 草稿键：本页是条件渲染（切走即卸载），不存盘的话切个 tab 回来内容就没了（客户实锤反馈）。 */
const DRAFT_KEY = "uking.feedback.draft";

/** 一张粘贴进来的截图：`path` 是后端落盘路径，`preview` 是本次会话内的缩略图（不落 localStorage，太大），
 *  `upload` 是压缩后的 JPEG base64（不含 data: 前缀），只有用户勾选「同意上传」时才随反馈发出。 */
type Shot = { path: string; preview?: string; upload?: string };

/** 草稿结构：正文 + 截图路径（缩略图不存，恢复后显示文件名）。 */
type Draft = { message: string; shots: string[]; includeDiag: boolean; uploadShots?: boolean };

function loadDraft(): Draft {
  try {
    const raw = localStorage.getItem(DRAFT_KEY);
    if (!raw) return { message: "", shots: [], includeDiag: true, uploadShots: false };
    const d = JSON.parse(raw) as Partial<Draft>;
    return {
      message: typeof d.message === "string" ? d.message : "",
      shots: Array.isArray(d.shots) ? d.shots.filter((s) => typeof s === "string") : [],
      includeDiag: d.includeDiag !== false,
      // 隐私默认：不上传。必须用户这次主动勾，才为 true。
      uploadShots: d.uploadShots === true,
    };
  } catch {
    return { message: "", shots: [], includeDiag: true, uploadShots: false };
  }
}

export function Feedback({ version, onToast }: { version?: string; onToast: (s: string) => void }) {
  const { t } = useI18n();
  const draft0 = useRef(loadDraft()).current;
  const [message, setMessage] = useState(draft0.message);
  const [shots, setShots] = useState<Shot[]>(draft0.shots.map((path) => ({ path })));
  const [includeDiag, setIncludeDiag] = useState(draft0.includeDiag);
  // 截图是否随反馈上传。默认 false —— 截图可能拍到桌面上的私人内容，必须用户主动勾。
  const [uploadShots, setUploadShots] = useState(draft0.uploadShots === true);
  const [submitting, setSubmitting] = useState(false);
  const [diag, setDiag] = useState<string | null>(null);
  const [loadingDiag, setLoadingDiag] = useState(false);
  const [sent, setSent] = useState<string | null>(null);
  // 远程协助
  const [assist, setAssist] = useState<AssistStatus | null>(null);
  const [assistBusy, setAssistBusy] = useState(false);
  const [assistLog, setAssistLog] = useState("");
  // 二维码大图（小图 92px 手机不一定扫得动，点开给一张够大的）
  const [qrOpen, setQrOpen] = useState(false);
  // 屏幕协助（UU远程）
  const [uu, setUu] = useState<UuStatus | null>(null);
  const [uuBusy, setUuBusy] = useState(false);
  const [uuLog, setUuLog] = useState("");

  // 进页面拉一次状态；开着的时候每 30s 刷新一次（为了让「还剩 X 分钟」是活的）。
  useEffect(() => {
    let alive = true;
    const pull = () => {
      invoke<AssistStatus>("remote_assist_status")
        .then((s) => alive && setAssist(s))
        .catch(() => {});
    };
    pull();
    const timer = setInterval(pull, 30_000);
    const un = listen<string>("uking:remote_assist", (e) => alive && setAssistLog(String(e.payload)));
    return () => {
      alive = false;
      clearInterval(timer);
      void un.then((f) => f());
    };
  }, []);

  const startAssist = async () => {
    setAssistBusy(true);
    setAssistLog("");
    try {
      setAssist(await invoke<AssistStatus>("remote_assist_start"));
      onToast(t("远程协助已开启，请把协助编号发给作者"));
    } catch (e) {
      onToast(t("开启失败：") + String(e));
    } finally {
      setAssistBusy(false);
    }
  };

  const stopAssist = async () => {
    setAssistBusy(true);
    try {
      await invoke("remote_assist_stop");
      setAssist(await invoke<AssistStatus>("remote_assist_status"));
      setAssistLog("");
      onToast(t("已停止远程协助"));
    } catch (e) {
      onToast(t("停止失败：") + String(e));
    } finally {
      setAssistBusy(false);
    }
  };

  // UU远程状态：进页面拉一次；装完再拉一次刷新按钮文案。
  const pullUu = () =>
    invoke<UuStatus>("uu_remote_status")
      .then(setUu)
      .catch(() => {});
  useEffect(() => {
    void pullUu();
    const un = listen<string>("uking:uuremote_progress", (e) => setUuLog(String(e.payload)));
    return () => void un.then((f) => f());
  }, []);

  /** 帮客户下载 + 安装 UU远程。只帮到「装上」——连接是他自己在 UU远程 里开、把 ID+验证码发过来，
   *  我们不碰他账号，也不代他授权（这条不能为了少一步而妥协）。 */
  const installUu = async () => {
    setUuBusy(true);
    setUuLog("");
    try {
      onToast(await invoke<string>("install_uu_remote"));
    } catch (e) {
      onToast(t("安装失败：") + String(e));
    } finally {
      setUuBusy(false);
      void pullUu();
    }
  };

  const openUuPage = () => {
    const url = uu?.download_page || "https://uuyc.163.com/download/";
    openUrl(url).catch(() => onToast(t("打开失败，请手动访问 {url}", { url })));
  };

  const copyDeviceId = async () => {
    const id = assist?.device_id;
    if (!id) return;
    try {
      await navigator.clipboard.writeText(id);
      onToast(t("已复制协助编号：{id}", { id }));
    } catch {
      onToast(id);
    }
  };

  // 草稿随打随存：切页/关窗都不丢，提交成功才清（下面 submit 里 clearDraft）。
  useEffect(() => {
    const d: Draft = { message, shots: shots.map((s) => s.path), includeDiag, uploadShots };
    try {
      if (!d.message && d.shots.length === 0) localStorage.removeItem(DRAFT_KEY);
      else localStorage.setItem(DRAFT_KEY, JSON.stringify(d));
    } catch {
      /* 存储满/隐私模式：草稿存不了不影响提交 */
    }
  }, [message, shots, includeDiag, uploadShots]);

  /**
   * 按需拉一份脱敏诊断（展示/复制/拼 mailto 用）。缓存到 state 避免重复采集；
   * `force=true` 时强制重采 —— 按钮在已有缓存时文案是「刷新」，此前却直接吃缓存、点了没反应。
   */
  const ensureDiag = async (force = false): Promise<string> => {
    if (diag != null && !force) return diag;
    setLoadingDiag(true);
    try {
      const d = await invoke<string>("collect_diagnostics");
      setDiag(d);
      return d;
    } catch {
      const d = t("（诊断采集失败）");
      setDiag(d);
      return d;
    } finally {
      setLoadingDiag(false);
    }
  };

  /**
   * 粘贴截图（Ctrl+V）：图存到本机 `~/.uking/feedback/`，反馈正文里注明路径。
   * 图默认**不**上传（隐私优先）：正文只记路径，作者需要时让用户点「截图文件夹」发过来。
   * 用户勾选「同意上传截图」后，才把压缩版随反馈一起发出 —— 服务端用建 issue 那把 token
   * 把图传进仓库并贴进 Issue，客户端不接触任何凭证。
   */
  /**
   * 压到「够看清界面、又能走完上报链路」：最长边 1280、JPEG 0.7。
   * 典型窗口截图压完 100~250KB，服务端上限 675KB，够用且留余量。
   * 压缩放在前端做，后端就不必引入图像库（本项目体积优先）。
   */
  const compressForUpload = (dataUrl: string): Promise<string | undefined> =>
    new Promise((resolve) => {
      const img = new Image();
      img.onload = () => {
        try {
          const max = 1280;
          const scale = Math.min(1, max / Math.max(img.width, img.height));
          const w = Math.max(1, Math.round(img.width * scale));
          const h = Math.max(1, Math.round(img.height * scale));
          const c = document.createElement("canvas");
          c.width = w;
          c.height = h;
          const ctx = c.getContext("2d");
          if (!ctx) return resolve(undefined);
          ctx.drawImage(img, 0, 0, w, h);
          const jpeg = c.toDataURL("image/jpeg", 0.7);
          resolve(jpeg.split(",")[1] || undefined);
        } catch {
          resolve(undefined);
        }
      };
      img.onerror = () => resolve(undefined);
      img.src = dataUrl;
    });

  const onPaste = async (e: React.ClipboardEvent) => {
    const files = Array.from(e.clipboardData?.files ?? []).filter((f) => f.type.startsWith("image/"));
    if (files.length === 0) return;
    e.preventDefault();
    for (const f of files) {
      try {
        const dataUrl = await new Promise<string>((res, rej) => {
          const r = new FileReader();
          r.onload = () => res(String(r.result));
          r.onerror = () => rej(new Error("read"));
          r.readAsDataURL(f);
        });
        const path = await invoke<string>("save_feedback_shot", { dataUrl });
        // 压缩版立刻算好放在内存里：上传与否等提交时看勾选框，用户随时可以改主意。
        const upload = await compressForUpload(dataUrl);
        setShots((prev) => [...prev, { path, preview: dataUrl, upload }]);
        onToast(t("截图已附上"));
      } catch (err) {
        onToast(t("附截图失败：") + String(err));
      }
    }
  };

  const removeShot = (path: string) => setShots((prev) => prev.filter((s) => s.path !== path));

  const openShots = () => {
    invoke("open_feedback_shots_dir")
      .then(() => onToast(t("已打开截图文件夹")))
      .catch((e) => onToast(t("打开失败：") + String(e)));
  };

  /** ① 一键提交（服务端上报）。 */
  const submit = async () => {
    if (!message.trim()) {
      onToast(t("请先写一句你遇到的问题或建议"));
      return;
    }
    setSubmitting(true);
    try {
      // 协助开着就把编号带进正文 —— 否则作者收到 Issue 还得回头问「你那个 pc-XXXX 是多少」，
      // 而客户往往这会儿已经离开电脑了。
      const withId =
        assist?.running && assist.device_id
          ? `${message}\n\n【远程协助已开启，编号 ${assist.device_id}】`
          : message;
      const msg = await invoke<string>("submit_feedback", {
        message: withId,
        includeDiagnostics: includeDiag,
        shots: shots.map((s) => s.path),
        // 只有勾了才发图。草稿恢复回来的截图没有内存里的压缩版（upload 为空），
        // 那几张就退回「只记路径」的老行为，不会静默漏发也不会报错。
        shotData: uploadShots ? shots.map((s) => s.upload).filter(Boolean) : [],
      });
      onToast(String(msg));
      // 页面上留一条持久确认（toast 一闪而过，用户会怀疑到底发没发出去）。
      setSent(String(msg));
      setMessage("");
      setShots([]);
      try {
        localStorage.removeItem(DRAFT_KEY);
      } catch {
        /* 忽略 */
      }
    } catch (e) {
      onToast(t("提交失败：") + String(e));
    } finally {
      setSubmitting(false);
    }
  };

  /** ② 发邮件给作者（mailto 预填件名 + 正文；日志走「打开日志文件夹」手动附）。 */
  const sendMail = async () => {
    const subject = t("U-King 反馈 v{v}", { v: version ?? "" });
    let body = message.trim() ? message.trim() + "\n\n" : "";
    if (shots.length > 0) {
      // 邮件能带附件 —— 这是截图唯一能真正到作者手上的路，正文里把路径列清楚。
      body += t("（附了 {n} 张截图，在本机：）", { n: String(shots.length) }) + "\n";
      body += shots.map((s) => "  " + s.path).join("\n") + "\n\n";
    }
    if (includeDiag) {
      const d = await ensureDiag();
      // mailto 正文别太长（部分邮件客户端会截断），诊断截到 ~1500 字，完整版可点「复制脱敏诊断」。
      body += "----\n" + d.slice(0, 1500);
    }
    body += "\n\n" + t("（如需附日志，请点页面「打开日志文件夹」把日志文件拖进邮件附件）");
    const url = `mailto:${CONTACT_EMAIL}?subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(body)}`;
    openUrl(url).catch(() => onToast(t("打开邮件失败，可手动发到 {email}", { email: CONTACT_EMAIL })));
  };

  const copyDiag = async () => {
    const d = await ensureDiag();
    try {
      await navigator.clipboard.writeText(d);
      onToast(t("已复制脱敏诊断，可贴到邮件/微信"));
    } catch {
      onToast(t("复制失败，请手动选择复制"));
    }
  };

  const copyEmail = async () => {
    try {
      await navigator.clipboard.writeText(CONTACT_EMAIL);
      onToast(t("已复制邮箱：{email}", { email: CONTACT_EMAIL }));
    } catch {
      onToast(CONTACT_EMAIL);
    }
  };

  // 复制失败也要把号码**说出来** —— 客户复制不了至少还能照着敲（同 Geo.tsx 的处理）
  const copyWechat = async () => {
    try {
      await navigator.clipboard.writeText(CONTACT_WECHAT);
      onToast(t("微信号已复制：{id}，加我直接说问题", { id: CONTACT_WECHAT }));
    } catch {
      onToast(t("请手动添加微信：{id}", { id: CONTACT_WECHAT }));
    }
  };

  const openLogs = () => {
    invoke("open_log_dir")
      .then(() => onToast(t("已打开日志文件夹")))
      .catch((e) => onToast(t("打开失败：") + String(e)));
  };

  return (
    <div className="space-y-5">
      {/* 二维码大图：点遮罩或按 Esc 都能关（别只留一个小叉，手机扫码时人不想找） */}
      {qrOpen && (
        <div
          className="fixed inset-0 z-50 grid place-items-center bg-black/70 backdrop-blur-sm"
          onClick={() => setQrOpen(false)}
          onKeyDown={(e) => e.key === "Escape" && setQrOpen(false)}
          role="presentation"
        >
          <div className="rounded-card bg-white p-3 shadow-pop" onClick={(e) => e.stopPropagation()}>
            <img src={wechatQr} alt={t("微信二维码")} className="w-[320px] h-auto block" />
            <div className="mt-2 flex items-center justify-between gap-3">
              <span className="text-[12px] text-neutral-600">
                {t("微信号")} <span className="font-mono">{CONTACT_WECHAT}</span>
              </span>
              <button
                onClick={() => setQrOpen(false)}
                className="text-[12px] text-neutral-500 hover:text-neutral-800 px-2 py-0.5"
              >
                {t("关闭")}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 头 */}
      <div className="flex items-center gap-3">
        <span className="grid place-items-center w-10 h-10 rounded-xl bg-accent/[0.12] shrink-0">
          <LifeBuoy size={20} className="text-accent" />
        </span>
        <div>
          <h1 className="text-[17px] font-semibold text-ink-0">{t("技术支持 · 报告问题")}</h1>
          <p className="text-[12px] text-ink-3">{t("遇到 bug、有建议，都可以在这里告诉我们。日志会脱敏后再发送。")}</p>
        </div>
      </div>

      {/* 联系方式：微信优先，邮箱次之 */}
      <section className="rounded-card border border-white/[0.08] bg-bg-1/70 px-5 py-4 shadow-card space-y-2">
        <div className="flex items-center gap-2 mb-0.5">
          <MessageCircle size={14} className="text-accent" />
          <h2 className="text-[13px] font-semibold text-ink-0">{t("直接找我们")}</h2>
        </div>
        {/* 微信 / 邮箱两行同构，都是「一个可点的标识 + 一句说明」——
            二维码**不平铺在页面上**：一张 92px 的码既扫不动又把这块撑得难看，
            真要扫的人点一下就有大图（320px，够手机对焦）。 */}
        <div className="flex flex-wrap items-center gap-2">
          <MessageCircle size={13} className="text-ink-4" />
          <button
            onClick={() => void copyWechat()}
            title={t("点此复制微信号")}
            className="font-mono text-[13px] text-accent hover:text-accent-600 underline underline-offset-2"
          >
            {CONTACT_WECHAT}
          </button>
          <span className="text-[11px] text-ink-4">{t("（点微信号可复制）")}</span>
          <button
            onClick={() => setQrOpen(true)}
            className="inline-flex items-center gap-1 rounded-card border border-white/10 px-2 py-0.5 text-[11px] text-ink-2 hover:bg-white/[0.06]"
          >
            <QrCode size={11} className="text-accent" />
            {t("扫码加好友")}
          </button>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Mail size={13} className="text-ink-4" />
          <button
            onClick={copyEmail}
            title={t("点此复制邮箱")}
            className="font-mono text-[13px] text-accent hover:text-accent-600 underline underline-offset-2"
          >
            {CONTACT_EMAIL}
          </button>
          <span className="text-[11px] text-ink-4">{t("（点邮箱可复制）")}</span>
        </div>
      </section>

      {/* 写反馈 */}
      <section className="rounded-card border border-white/[0.08] bg-bg-1/70 px-5 py-4 shadow-card space-y-3">
        <div className="flex items-center gap-2">
          <Bug size={14} className="text-accent" />
          <h2 className="text-[13px] font-semibold text-ink-0">{t("说说你遇到的问题或建议")}</h2>
        </div>
        <textarea
          value={message}
          onChange={(e) => setMessage(e.target.value)}
          onPaste={onPaste}
          rows={5}
          placeholder={t("例如：装 Claude 一直失败 / 作图点了没反应 / 希望能加某个功能…（截图可以直接 Ctrl+V 粘贴进来）")}
          className="w-full rounded-lg border border-white/[0.10] bg-bg-0 px-3 py-2.5 text-[13px] text-ink-0 placeholder:text-ink-5 outline-none focus:border-accent/50 resize-y"
        />

        {/* 粘贴进来的截图 */}
        <div className="flex flex-wrap items-center gap-2">
          <span className="inline-flex items-center gap-1 text-[11px] text-ink-4">
            <ImagePlus size={12} /> {t("截图直接 Ctrl+V 粘贴（存在本机，作者需要时可发给他）")}
          </span>
          {shots.map((s) => (
            <span
              key={s.path}
              className="group relative inline-flex items-center gap-1.5 rounded-lg border border-white/[0.10] bg-bg-0 pl-1.5 pr-1 py-1"
              title={s.path}
            >
              {s.preview ? (
                <img src={s.preview} alt="" className="w-10 h-10 object-cover rounded" />
              ) : (
                <span className="grid place-items-center w-10 h-10 rounded bg-white/[0.04]">
                  <ImagePlus size={14} className="text-ink-4" />
                </span>
              )}
              <span className="max-w-[140px] truncate text-[11px] text-ink-3">
                {s.path.split(/[\\/]/).pop()}
              </span>
              <button
                onClick={() => removeShot(s.path)}
                title={t("移除")}
                className="grid place-items-center w-5 h-5 rounded text-ink-4 hover:text-ink-0 hover:bg-white/[0.08]"
              >
                <X size={12} />
              </button>
            </span>
          ))}
          {shots.length > 0 && (
            <button
              onClick={openShots}
              className="inline-flex items-center gap-1 text-[11px] text-accent hover:text-accent-600"
            >
              <FolderOpen size={12} /> {t("截图文件夹")}
            </button>
          )}
        </div>

        {shots.length > 0 && (
          <label className="flex items-center gap-2 text-[12px] text-ink-2 cursor-pointer select-none">
            <input
              type="checkbox"
              checked={uploadShots}
              onChange={(e) => setUploadShots(e.target.checked)}
              className="w-4 h-4 accent-accent"
            />
            {t("同意把截图一起上传（能快很多定位问题；不勾则只留在本机）")}
          </label>
        )}

        <label className="flex items-center gap-2 text-[12px] text-ink-2 cursor-pointer select-none">
          <input
            type="checkbox"
            checked={includeDiag}
            onChange={(e) => setIncludeDiag(e.target.checked)}
            className="w-4 h-4 accent-accent"
          />
          {t("附带脱敏诊断日志（版本 / 系统 / 装机状态 / 报错尾部，已抹掉 Key 与隐私）")}
          <button
            type="button"
            onClick={() => void ensureDiag(diag != null)}
            className="ml-1 text-[11px] text-accent hover:text-accent-600"
          >
            {loadingDiag ? t("采集中…") : diag == null ? t("预览") : t("刷新")}
          </button>
        </label>

        {diag != null && (
          <pre className="max-h-48 overflow-auto rounded-lg border border-white/[0.08] bg-bg-0 px-3 py-2 text-[10.5px] leading-relaxed text-ink-3 font-mono whitespace-pre-wrap">
            {diag}
          </pre>
        )}

        {/* 操作按钮 */}
        <div className="flex flex-wrap items-center gap-2 pt-1">
          <button
            onClick={submit}
            disabled={submitting}
            className="inline-flex items-center gap-1.5 h-9 px-4 rounded-lg bg-accent text-white text-[13px] font-semibold hover:bg-accent-600 disabled:opacity-60"
          >
            {submitting ? <Loader2 size={14} className="animate-spin" /> : <Send size={14} />}
            {t("一键提交反馈")}
          </button>
          <button
            onClick={sendMail}
            className="inline-flex items-center gap-1.5 h-9 px-4 rounded-lg border border-white/[0.10] text-ink-1 text-[13px] font-medium hover:bg-white/[0.04]"
          >
            <Mail size={14} className="text-accent" /> {t("发邮件给作者")}
          </button>
          <button
            onClick={openLogs}
            className="inline-flex items-center gap-1.5 h-9 px-3 rounded-lg border border-white/[0.10] text-ink-2 text-[12px] hover:bg-white/[0.04]"
          >
            <FolderOpen size={14} /> {t("打开日志文件夹")}
          </button>
          <button
            onClick={copyDiag}
            className="inline-flex items-center gap-1.5 h-9 px-3 rounded-lg border border-white/[0.10] text-ink-2 text-[12px] hover:bg-white/[0.04]"
          >
            <ClipboardCopy size={14} /> {t("复制脱敏诊断")}
          </button>
        </div>
        {sent && (
          <p className="rounded-lg border border-accent/30 bg-accent/[0.08] px-3 py-2 text-[12px] text-ink-1">
            ✅ {sent}
            {t("（截图留在本机，作者需要时会找你要；也可以点上面「截图文件夹」直接发邮件附上。）")}
          </p>
        )}
        <p className="text-[11px] text-ink-4">
          {t("「一键提交」会把反馈发给作者（自动带脱敏诊断）；也可「发邮件给作者」直接联系。日志需要时点「打开日志文件夹」，把里面的文件拖进邮件附件即可。")}
        </p>
      </section>

      {/* 远程协助 —— 复杂问题靠截图和日志说不清时，让作者直接连上来看现场。
          刻意放在最后、默认关、文案把权限讲透：这是全权限远程执行，不是「诊断上报」。 */}
      {assist?.supported !== false && (
        // 默认折叠（测试报告 #023：「文字内容过多，直接平铺显示视觉冗长」）。
        // 这一段有两屏权限说明 —— 该讲透，但不该挡在「我就想提个 bug」的人面前。
        // **正在协助时强制展开**：那时候屏幕上唯一重要的东西是设备编号，折起来等于把它藏了。
        <details
          open={!!assist?.running}
          className="group rounded-card border border-white/[0.08] bg-bg-1/70 px-5 py-4 shadow-card"
        >
          <summary className="flex items-center gap-2 cursor-pointer select-none list-none">
            <MonitorSmartphone size={14} className="text-accent" />
            <h2 className="text-[13px] font-semibold text-ink-0">{t("远程协助（需要时再开）")}</h2>
            {assist?.running ? (
              <span className="text-[10.5px] text-success-400 border border-success-500/30 bg-success-500/[0.10] rounded px-1.5 py-0.5">
                {t("协助进行中")}
              </span>
            ) : (
              <span className="ml-auto text-[11px] text-ink-5 group-open:hidden">{t("展开 ›")}</span>
            )}
          </summary>
          <div className="space-y-3 mt-3">
          {/* 两条路各有各的适用场景，别让客户以为是重复功能：命令查不出来的（界面点不动、
              弹窗看不懂、装到一半卡着）只能看屏幕；反过来，看屏幕排 PATH/配置又极慢。 */}
          <p className="text-[11px] text-ink-4 leading-relaxed">
            {t("两种方式：① 让作者跑命令排查（不用装东西，查配置/日志最快）；② 让作者看到你的屏幕（界面点不动、弹窗看不懂时用）。")}
          </p>

          {!assist?.running ? (
            <>
              <h3 className="text-[12.5px] font-semibold text-ink-1">{t("① 让作者跑命令排查（U-King 自带）")}</h3>
              <p className="text-[12px] text-ink-2 leading-relaxed">
                {t("装不上、报错说不清、截图看不出问题时，可以让作者直接连上你的电脑排查，不用你再截图描述。")}
              </p>
              {/* 权限必须说人话讲清楚，别用「协助」两个字把「远程执行命令」糊过去。 */}
              <div className="rounded-lg border border-amber-500/25 bg-amber-500/[0.07] px-3 py-2.5 text-[11.5px] text-ink-2 leading-relaxed space-y-1">
                <p className="flex items-start gap-1.5">
                  <ShieldCheck size={13} className="text-amber-400 shrink-0 mt-0.5" />
                  <span>
                    {t("开启后，作者可以在你这台电脑上执行命令、读取文件来排查问题。请只在你正在联系作者时开启。")}
                  </span>
                </p>
                <p className="pl-[18px]">
                  {t("· 你随时可以点「停止协助」立刻断开；{h} 小时后也会自动断开。", { h: 2 })}
                </p>
                <p className="pl-[18px]">{t("· 作者执行过的每条命令都会记进本机审计日志，你可以随时查看。")}</p>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <button
                  onClick={startAssist}
                  disabled={assistBusy}
                  className="inline-flex items-center gap-1.5 h-9 px-4 rounded-lg border border-accent/40 text-accent text-[13px] font-medium hover:bg-accent/[0.10] disabled:opacity-60"
                >
                  {assistBusy ? <Loader2 size={14} className="animate-spin" /> : <MonitorSmartphone size={14} />}
                  {t("开启远程协助")}
                </button>
                <button
                  onClick={() => void invoke("remote_assist_open_audit").catch(() => {})}
                  className="inline-flex items-center gap-1.5 h-9 px-3 rounded-lg border border-white/[0.10] text-ink-2 text-[12px] hover:bg-white/[0.04]"
                >
                  <FolderOpen size={14} /> {t("查看审计日志")}
                </button>
              </div>
              {assistBusy && assistLog && <p className="text-[11.5px] text-ink-3">{assistLog}</p>}
            </>
          ) : (
            <>
              <div className="rounded-lg border border-accent/30 bg-accent/[0.08] px-3.5 py-3">
                <p className="text-[11.5px] text-ink-3 mb-1">{t("把这个编号发给作者：")}</p>
                <div className="flex items-center gap-2">
                  <button
                    onClick={copyDeviceId}
                    title={t("点此复制")}
                    className="font-mono text-[22px] font-semibold text-accent hover:text-accent-600 tracking-wide"
                  >
                    {assist.device_id}
                  </button>
                  <button
                    onClick={copyDeviceId}
                    className="inline-flex items-center gap-1 h-7 px-2 rounded-lg border border-white/[0.10] text-ink-2 text-[11px] hover:bg-white/[0.04]"
                  >
                    <ClipboardCopy size={12} /> {t("复制")}
                  </button>
                </div>
                {typeof assist.remaining_secs === "number" && (
                  <p className="text-[11px] text-ink-4 mt-1.5">
                    {t("约 {m} 分钟后自动断开", { m: Math.max(1, Math.round(assist.remaining_secs / 60)) })}
                  </p>
                )}
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <button
                  onClick={stopAssist}
                  disabled={assistBusy}
                  className="inline-flex items-center gap-1.5 h-9 px-4 rounded-lg border border-danger-500/40 text-danger-400 text-[13px] font-medium hover:bg-danger-500/[0.10] disabled:opacity-60"
                >
                  {assistBusy ? <Loader2 size={14} className="animate-spin" /> : <X size={14} />}
                  {t("停止协助")}
                </button>
                <button
                  onClick={() => void invoke("remote_assist_open_audit").catch(() => {})}
                  className="inline-flex items-center gap-1.5 h-9 px-3 rounded-lg border border-white/[0.10] text-ink-2 text-[12px] hover:bg-white/[0.04]"
                >
                  <FolderOpen size={14} /> {t("查看审计日志")}
                </button>
              </div>
            </>
          )}

          {/* ② 屏幕协助 —— UU远程（网易官方）。这里只做「帮你把它装上」，连接走它自己的界面。
              为什么值得内置：客户自己找官网 → 挑平台 → 在一堆「高速下载器」里挑真包，
              这一段掉队率很高，而正需要远程协助的客户恰恰是最不会走这段的人。 */}
          <div className="pt-3 border-t border-white/[0.06] space-y-2.5">
            <div className="flex items-center gap-2">
              <ScreenShare size={14} className="text-accent" />
              <h3 className="text-[12.5px] font-semibold text-ink-1">{t("② 让作者看到你的屏幕（UU远程）")}</h3>
              {uu?.installed && (
                <span className="text-[10.5px] text-success-400 border border-success-500/30 bg-success-500/[0.10] rounded px-1.5 py-0.5">
                  {t("已安装")}
                </span>
              )}
            </div>
            <p className="text-[12px] text-ink-2 leading-relaxed">
              {t("界面点不动、弹窗看不懂、装到一半卡住 —— 这类问题命令查不出来，得让作者直接看你的屏幕。用网易官方的 UU远程，我们帮你下好装上。")}
            </p>
            {/* 没有绿色版是硬事实，写在最显眼处。用户问过「有没有点开就能用的绿色版」——
                答案是没有，与其让他装完才发现，不如现在就说清楚要装 86MB。 */}
            {uu?.portable_available === false && !uu?.installed && (
              <p className="text-[11px] text-ink-4 leading-relaxed">
                {t("· 官方没有免安装的绿色版，只有安装包（约 86 MB），所以需要装一次。")}
              </p>
            )}
            <p className="text-[11px] text-ink-4 leading-relaxed">
              {t("· 装完打开 UU远程 →「远程协助」，把上面的 ID 和验证码发给作者就能连。你随时可以在它界面里断开。")}
            </p>
            <div className="flex flex-wrap items-center gap-2">
              {uu?.can_auto_install && !uu.installed && (
                <button
                  onClick={installUu}
                  disabled={uuBusy}
                  className="inline-flex items-center gap-1.5 h-9 px-4 rounded-lg border border-accent/40 text-accent text-[13px] font-medium hover:bg-accent/[0.10] disabled:opacity-60"
                >
                  {uuBusy ? <Loader2 size={14} className="animate-spin" /> : <Download size={14} />}
                  {uuBusy ? t("正在下载安装…") : t("帮我下载安装（约 86 MB）")}
                </button>
              )}
              <button
                onClick={openUuPage}
                className="inline-flex items-center gap-1.5 h-9 px-3 rounded-lg border border-white/[0.10] text-ink-2 text-[12px] hover:bg-white/[0.04]"
              >
                <ExternalLink size={14} /> {t("打开官网下载页")}
              </button>
            </div>
            {uuLog && <p className="text-[11.5px] text-ink-3">{uuLog}</p>}
          </div>
          </div>
        </details>
      )}
    </div>
  );
}
