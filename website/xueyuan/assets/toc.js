/* U-King 学院 · 阅读增强（所有 xueyuan 页面共享）
 * 用法：任意教程页 <head> 加一行
 *   <script defer src="assets/toc.js"></script>   （modules/ 下用 ../assets/toc.js）
 * 提供三件事，纯前端、零依赖、不改已有内联样式：
 *   1) 飞书式左侧目录：扫 .wrap/.page 里的 h2/h3 → sticky 目录 + 滚动高亮，
 *      窄屏收起为悬浮「目录」按钮；学院首页（有 .hero）自动跳过。
 *   2) 代码块一键复制：每个 <pre> 右上角「复制」按钮（小白教程的核心动作）。
 *   3) 深/浅色切换：按钮挂在页面顶栏（.bknav/.nav），toggle html.light，
 *      存 localStorage("xy-theme")；首次跟随系统（页面 head 里的内联脚本负责预取防闪）。
 * 颜色一律走页面 :root 的 CSS 变量（--gold/--ink/--line…，带兜底），深浅两套皮肤都对。 */
(function () {
  "use strict";

  /* ---------- 公共：找到吸顶栏高度（.bknav 手册页 / .nav 学院页） ---------- */
  function stickyBar() {
    return document.querySelector(".bknav") || document.querySelector("nav.nav");
  }
  function barH() {
    var b = stickyBar();
    return b ? Math.ceil(b.getBoundingClientRect().height) : 0;
  }

  /* ---------- 1) 深/浅色切换 ---------- */
  function initTheme() {
    var root = document.documentElement;
    function label() { return root.classList.contains("light") ? "🌙 深色" : "☀️ 浅色"; }
    var btn = document.createElement("button");
    btn.className = "xy-theme";
    btn.type = "button";
    btn.title = "切换深色 / 浅色阅读";
    btn.textContent = label();
    btn.addEventListener("click", function () {
      root.classList.toggle("light");
      try { localStorage.setItem("xy-theme", root.classList.contains("light") ? "light" : "dark"); } catch (e) {}
      btn.textContent = label();
    });
    // 优先挂进页面自己的顶栏，视觉上是页面的一部分；没有顶栏才右上角悬浮
    var host = document.querySelector(".bknav .r") || document.querySelector(".nav .links");
    if (host) { host.appendChild(btn); }
    else { btn.classList.add("fixed"); document.body.appendChild(btn); }
  }

  /* ---------- 2) 代码块一键复制 ---------- */
  function initCopy() {
    var pres = [].slice.call(document.querySelectorAll("pre"));
    pres.forEach(function (pre) {
      var codeEl = pre.querySelector("code");
      var text = (codeEl ? codeEl.innerText : pre.innerText).replace(/\n$/, "");
      if (!text.trim()) return;
      var btn = document.createElement("button");
      btn.className = "xy-copy";
      btn.type = "button";
      btn.textContent = "复制";
      btn.addEventListener("click", function () {
        function done(ok) {
          btn.textContent = ok ? "✅ 已复制" : "复制失败";
          btn.classList.toggle("ok", ok);
          setTimeout(function () { btn.textContent = "复制"; btn.classList.remove("ok"); }, 1600);
        }
        if (navigator.clipboard && navigator.clipboard.writeText) {
          navigator.clipboard.writeText(text).then(function () { done(true); }, function () { done(fallback()); });
        } else { done(fallback()); }
        function fallback() { // file:// 或旧内核：textarea + execCommand
          try {
            var ta = document.createElement("textarea");
            ta.value = text; ta.style.position = "fixed"; ta.style.opacity = "0";
            document.body.appendChild(ta); ta.select();
            var ok = document.execCommand("copy");
            document.body.removeChild(ta); return ok;
          } catch (e) { return false; }
        }
      });
      pre.appendChild(btn);
    });
  }

  /* ---------- 3) 左侧目录 ---------- */
  function initToc() {
    if (document.querySelector(".hero")) return; // 学院首页是卡片墙，不需要目录
    var wrap = document.querySelector("article.page") || document.querySelector(".wrap");
    if (!wrap) return;
    var heads = [].slice.call(wrap.querySelectorAll("h2, h3"));
    if (heads.length < 2) return; // 章节太少不值得做目录

    var off = barH() + 14;
    heads.forEach(function (h, i) {
      if (!h.id) h.id = "ac-sec-" + i;
      h.style.scrollMarginTop = off + "px";
    });

    var toc = document.createElement("aside");
    toc.className = "ac-toc";
    toc.style.top = barH() + "px";
    toc.style.height = "calc(100vh - " + barH() + "px)";
    var tt = document.createElement("div");
    tt.className = "ac-tt";
    tt.innerHTML = '<span class="d"></span>本页目录';
    toc.appendChild(tt);
    var links = [];
    heads.forEach(function (h) {
      var a = document.createElement("a");
      a.href = "#" + h.id;
      a.textContent = (h.textContent || "").trim();
      if (h.tagName.toLowerCase() === "h3") a.className = "lv3";
      a.addEventListener("click", function (e) {
        e.preventDefault();
        h.scrollIntoView({ behavior: "smooth", block: "start" });
        history.replaceState(null, "", "#" + h.id);
        closeMobile();
      });
      toc.appendChild(a);
      links.push(a);
    });

    // 两栏布局：把正文裹进 .ac-doc > .ac-main，目录放左边
    var doc = document.createElement("div");
    doc.className = "ac-doc";
    var main = document.createElement("div");
    main.className = "ac-main";
    wrap.parentNode.insertBefore(doc, wrap);
    doc.appendChild(toc);
    doc.appendChild(main);
    main.appendChild(wrap);

    // 窄屏：悬浮「目录」按钮 + 遮罩
    var fab = document.createElement("button");
    fab.className = "ac-fab";
    fab.type = "button";
    fab.innerHTML = "📑 目录";
    var mask = document.createElement("div");
    mask.className = "ac-mask";
    document.body.appendChild(mask);
    document.body.appendChild(fab);
    function openMobile() { toc.classList.add("open"); mask.classList.add("open"); }
    function closeMobile() { toc.classList.remove("open"); mask.classList.remove("open"); }
    fab.addEventListener("click", openMobile);
    mask.addEventListener("click", closeMobile);

    // 滚动高亮（scroll-spy）
    var byId = {};
    links.forEach(function (a) { byId[a.getAttribute("href").slice(1)] = a; });
    var current = null;
    function setActive(a) {
      if (a === current) return;
      if (current) current.classList.remove("on");
      if (a) { a.classList.add("on"); }
      current = a;
    }
    if ("IntersectionObserver" in window) {
      var visible = {};
      var io = new IntersectionObserver(function (entries) {
        entries.forEach(function (en) { visible[en.target.id] = en.isIntersecting; });
        var pick = null;
        for (var i = 0; i < heads.length; i++) {
          if (visible[heads[i].id]) { pick = heads[i]; break; }
        }
        if (!pick) return;
        setActive(byId[pick.id]);
        var link = byId[pick.id];
        if (link && link.scrollIntoView) {
          var r = link.getBoundingClientRect();
          if (r.top < 80 || r.bottom > window.innerHeight - 40) {
            link.scrollIntoView({ block: "nearest" });
          }
        }
      }, { rootMargin: "0px 0px -75% 0px", threshold: 0 });
      heads.forEach(function (h) { io.observe(h); });
    }
    setActive(links[0]);
  }

  /* ---------- 注入样式（全部走 CSS 变量 + 中性色叠加，深浅两套皮肤通用） ---------- */
  function injectCss() {
    var css = document.createElement("style");
    css.textContent = [
      "pre{position:relative}",
      ".xy-copy{position:absolute;top:6px;right:6px;z-index:2;padding:3px 11px;border-radius:6px;",
      "  border:1px solid var(--line,rgba(140,130,100,.3));background:var(--card,#16161a);",
      "  color:var(--ink3,#9598a1);font-size:12px;line-height:1.6;cursor:pointer;opacity:.88}",
      ".xy-copy:hover{opacity:1;color:var(--ink,#f5f6f8);border-color:var(--gold,#d4a64a)}",
      ".xy-copy.ok{color:var(--ok,#3ecf8e);border-color:var(--ok,#3ecf8e)}",
      ".xy-theme{padding:4px 12px;border-radius:16px;border:1px solid var(--line,rgba(140,130,100,.3));",
      "  background:transparent;color:var(--ink2,#c4c6cc);font-size:12.5px;line-height:1.6;cursor:pointer;white-space:nowrap}",
      ".xy-theme:hover{color:var(--gold,#d4a64a);border-color:var(--gold,#d4a64a)}",
      ".xy-theme.fixed{position:fixed;top:12px;right:14px;z-index:70;background:var(--card,#16161a)}",
      ".ac-doc{display:flex;align-items:flex-start}",
      ".ac-main{flex:1 1 auto;min-width:0}",
      ".ac-toc{position:sticky;top:0;flex:0 0 252px;width:252px;height:100vh;overflow-y:auto;",
      "  padding:24px 10px 30px 18px;border-right:1px solid var(--line,rgba(140,130,100,.25));",
      "  background:rgba(128,118,90,.04);-webkit-overflow-scrolling:touch}",
      ".ac-toc::-webkit-scrollbar{width:7px}",
      ".ac-toc::-webkit-scrollbar-thumb{background:rgba(128,118,90,.3);border-radius:4px}",
      ".ac-tt{font-size:11.5px;letter-spacing:.10em;color:var(--ink3,#9598a1);font-weight:700;",
      "  text-transform:uppercase;padding:0 12px 12px;display:flex;align-items:center;gap:7px}",
      ".ac-tt .d{width:16px;height:2px;background:var(--gold,#d4a64a);border-radius:2px}",
      ".ac-toc a{display:block;padding:7px 12px;margin:1px 0;border-radius:7px;",
      "  color:var(--ink2,#c4c6cc);font-size:13.5px;line-height:1.55;text-decoration:none;",
      "  border-left:2px solid transparent;transition:background .12s,color .12s}",
      ".ac-toc a:hover{background:rgba(128,118,90,.12);color:var(--ink,#f7f8f8)}",
      ".ac-toc a.lv3{padding-left:28px;font-size:12.5px;color:var(--ink3,#9598a1)}",
      ".ac-toc a.on{color:var(--gold,#d4a64a);background:rgba(212,166,74,.12);",
      "  border-left-color:var(--gold,#d4a64a);font-weight:600}",
      ".ac-fab{display:none}",
      "@media(max-width:1040px){",
      "  .ac-toc{position:fixed;top:0!important;left:0;z-index:60;height:100vh!important;",
      "    transform:translateX(-100%);transition:transform .22s ease;background:var(--card,#16161a);",
      "    box-shadow:0 14px 50px rgba(0,0,0,.55)}",
      "  .ac-toc.open{transform:none}",
      "  .ac-mask{position:fixed;inset:0;z-index:59;background:rgba(0,0,0,.5);opacity:0;",
      "    pointer-events:none;transition:opacity .22s}",
      "  .ac-mask.open{opacity:1;pointer-events:auto}",
      "  .ac-fab{display:flex;align-items:center;gap:7px;position:fixed;right:16px;bottom:18px;",
      "    z-index:61;padding:12px 18px;border-radius:26px;border:1px solid var(--line,rgba(255,255,255,.12));",
      "    background:var(--gold,#d4a64a);color:#1a1206;font-weight:700;font-size:14px;",
      "    box-shadow:0 8px 28px rgba(0,0,0,.4);cursor:pointer}}",
      "@media print{.ac-toc,.ac-fab,.ac-mask,.xy-copy,.xy-theme{display:none!important}.ac-doc{display:block}}",
    ].join("\n");
    document.head.appendChild(css);
  }

  function init() {
    injectCss();
    initToc();
    initCopy();
    initTheme();
  }
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
