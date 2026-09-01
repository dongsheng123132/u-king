// 1so scan —— 一键"搜全网看自己"可视化体检仪表盘（SEO+GEO 融合）。
// 客户自己打开、自己查：40 个渠道逐个"去查↗"，标 有/没有，实时算互联网可见度；查完引导生成企业主页。
// "我们能查的就自动查"：Bing/搜狗/DuckDuckGo 脚本预探测并预填状态；查不了的（百度/小红书/抖音…）留给客户自查。
// 诚实：抓不到≠不存在；自动只是粗测，客户点开可覆盖。全自动抓取（真实浏览器）为后续版本。
import { projectPaths } from "../config.mjs";
import { CHANNELS, buildScan, channelCount } from "../channels.mjs";
import { autoProbe, AUTO_IDS } from "../probe.mjs";
import { readJson, writeJson, writeText, ensureDir, logE, warn, done, fail, esc, today } from "../util.mjs";

export async function cmdScan(args, llmOpts) {
  const jsonMode = !!args.json;
  const P = projectPaths(args.project || ".");
  const cards = readJson(P.cards, null);

  const name = (args.name && args.name !== true) ? String(args.name) : cards?.company?.name;
  if (!name) return fail(jsonMode, "缺少公司名。用 --name \"公司名\" 指定，或先 1so ingest。", 2);
  const region = (args.region && args.region !== true) ? String(args.region) : cards?.company?.region || "";
  const homepage = (args.homepage && args.homepage !== true) ? String(args.homepage) : "./index.html";

  const items = buildScan(name);
  if (region) { // 地图用 公司名+地区 更准
    const mq = encodeURIComponent(`${name} ${region}`);
    for (const it of items) if (it.category === "maps") {
      const tpl = CHANNELS.maps.engines.find((e) => e.id === it.id)?.url;
      if (tpl) it.url = tpl.replace("%s", mq);
    }
  }

  // 自动探测层（可选）：--auto 开启。能抓的（Bing/搜狗）抓真实页 → LLM 判读；其余交客户自查。
  let auto = {};
  if (args.auto) {
    const proxy = (args.proxy && args.proxy !== true) ? String(args.proxy) : (process.env.SO_PROXY || "");
    logE(`自动探测 ${[...AUTO_IDS].join(" / ")}${proxy ? "（走代理 " + proxy + "）" : ""}：抓真实页 → LLM 判读 …`);
    auto = await autoProbe(items, { name, region, proxy, llmOpts });
    const hit = Object.values(auto).filter((v) => v.status === "hit").length;
    const man = Object.values(auto).filter((v) => v.status === "manual").length;
    logE(`  自动判读：搜到 ${hit}，没搜到 ${Object.values(auto).length - hit - man}，连不上(降级人工) ${man}`);
  }
  for (const it of items) { it.auto = auto[it.id]?.status || (AUTO_IDS.has(it.id) ? null : "manual"); it.autoNote = auto[it.id]?.evidence || ""; }

  const manifest = { company: name, region, query: name, channels: channelCount(), autoRan: !!args.auto, generatedAt: today(), items };
  writeJson(P.scan, manifest);

  ensureDir(P.site);
  writeText(P.panel, renderPanel(name, region, items, homepage, !!args.auto));

  logE(`✓ 可视化体检仪表盘：${P.panel}`);
  logE(`  覆盖 ${items.length} 个渠道（AI搜索/AI对话/传统/社交/视频/百科/地图）；客户自己打开逐个查。`);
  return done(jsonMode, { ok: true, panel: P.panel, channels: items.length, autoRan: !!args.auto }, P.panel);
}

function renderPanel(name, region, items, homepage, autoRan) {
  const groups = {};
  for (const it of items) (groups[it.category] ||= { name: it.categoryName, icon: it.categoryIcon, list: [] }).list.push(it);

  const groupHtml = Object.entries(groups).map(([cat, g]) => `
    <section class="grp" data-cat="${cat}">
      <div class="grp-head">
        <h2>${g.icon} ${esc(g.name)} <span class="cnt" data-cat-cnt="${cat}"></span></h2>
        <button class="ghost open-grp" data-grp="${cat}">打开本组 ↗</button>
      </div>
      <div class="cards">
        ${g.list.map((it) => card(it)).join("")}
      </div>
    </section>`).join("");

  const dataItems = JSON.stringify(items.map((it) => ({ id: it.id, grp: it.category, url: it.url, auto: it.auto, note: it.autoNote || "" })));

  return `<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>互联网体检 · ${esc(name)}｜一搜商答</title>
<style>
  :root{--fg:#1a1a2e;--mut:#6b7280;--line:#e9e9ef;--accent:#4f46e5;--green:#16a34a;--red:#dc2626;--blue:#2563eb;--amber:#d97706;--bg:#f6f7fb}
  *{box-sizing:border-box}
  body{font:15px/1.6 -apple-system,"Segoe UI","Microsoft YaHei",sans-serif;color:var(--fg);background:var(--bg);margin:0}
  .wrap{max-width:1000px;margin:0 auto;padding:0 18px 60px}
  header{position:sticky;top:0;z-index:9;background:rgba(246,247,251,.95);backdrop-filter:blur(6px);padding:16px 0 12px;border-bottom:1px solid var(--line)}
  .brand{font-size:13px;color:var(--accent);font-weight:700;letter-spacing:.5px}
  .htop{display:flex;gap:20px;align-items:center;flex-wrap:wrap;margin-top:6px}
  h1{font-size:22px;margin:0}
  .sub{color:var(--mut);font-size:13px;margin:2px 0 0}
  .ring{width:88px;height:88px;border-radius:50%;display:grid;place-items:center;background:conic-gradient(var(--accent) 0%,#e5e7eb 0);flex:0 0 auto}
  .ring b{background:#fff;width:70px;height:70px;border-radius:50%;display:grid;place-items:center;font-size:22px;color:var(--accent);line-height:1;flex-direction:column}
  .ring b small{font-size:10px;color:var(--mut);font-weight:400}
  .stats{display:flex;gap:18px;flex-wrap:wrap}
  .stat{font-size:13px;color:var(--mut)} .stat b{display:block;font-size:20px;color:var(--fg)}
  .bar{display:flex;gap:8px;flex-wrap:wrap;margin-top:12px}
  button{font:inherit;cursor:pointer;border:1px solid var(--line);background:#fff;border-radius:8px;padding:7px 13px}
  #openAll{background:var(--accent);color:#fff;border-color:var(--accent);font-weight:600}
  .ghost{font-size:12px;padding:4px 10px;color:var(--mut)}
  .note{color:var(--mut);font-size:12px;margin:8px 0 0}
  .grp{margin-top:22px} .grp-head{display:flex;justify-content:space-between;align-items:center;margin-bottom:10px}
  .grp h2{font-size:16px;margin:0} .cnt{color:var(--mut);font-size:12px;font-weight:400;margin-left:4px}
  .cards{display:grid;grid-template-columns:repeat(auto-fill,minmax(230px,1fr));gap:10px}
  .card{background:#fff;border:1px solid var(--line);border-radius:12px;padding:12px}
  .card .top{display:flex;align-items:center;gap:8px}
  .card img{width:20px;height:20px;border-radius:4px}
  .card .nm{font-weight:600;flex:1}
  .chip{font-size:11px;padding:2px 8px;border-radius:999px;background:#f1f1f5;color:var(--mut);white-space:nowrap}
  .chip.hit{background:#dcfce7;color:var(--green)} .chip.miss{background:#fee2e2;color:var(--red)}
  .chip.auto{background:#dbeafe;color:var(--blue)} .chip.opened{background:#fef3c7;color:var(--amber)}
  .acts{display:flex;gap:6px;margin-top:10px}
  .acts a,.acts button{flex:1;text-align:center;text-decoration:none;font-size:13px;padding:6px 0;border-radius:8px;border:1px solid var(--line);background:#fff;color:var(--fg)}
  .acts .go{background:var(--accent);color:#fff;border-color:var(--accent)}
  .acts .yes.on{background:#dcfce7;color:var(--green);border-color:#bbf7d0}
  .acts .no.on{background:#fee2e2;color:var(--red);border-color:#fecaca}
  .cta{margin-top:30px;background:linear-gradient(135deg,#4f46e5,#7c3aed);color:#fff;border-radius:16px;padding:22px 24px}
  .cta h3{margin:0 0 6px;font-size:19px} .cta p{margin:0 0 14px;opacity:.92;font-size:14px}
  .cta a{display:inline-block;background:#fff;color:var(--accent);font-weight:700;text-decoration:none;padding:10px 20px;border-radius:10px}
  footer{margin-top:28px;color:var(--mut);font-size:12px;text-align:center}
</style>
</head>
<body>
<div class="wrap">
<header>
  <div class="brand">🔎 一搜商答 · 互联网体检</div>
  <div class="htop">
    <div class="ring" id="ring"><b><span id="ringNum">0</span><small>可见度</small></b></div>
    <div>
      <h1>${esc(name)}</h1>
      <p class="sub">在 ${items.length} 个渠道里搜「${esc(name)}${region ? " · 地图含 " + esc(region) : ""}」，看你在互联网里的样子。${autoRan ? "蓝色=系统已自动粗测。" : ""}</p>
      <div class="stats" style="margin-top:8px">
        <div class="stat"><b id="sHit">0</b>被搜到(渠道)</div>
        <div class="stat"><b id="sChecked">0</b>已核查</div>
        <div class="stat"><b>${items.length}</b>总渠道</div>
      </div>
    </div>
  </div>
  <div class="bar">
    <button id="openAll">🚀 一键全部打开（${items.length}）</button>
    <button class="ghost" id="openAI">只开 AI 类</button>
    <button class="ghost" id="openSocial">只开 社交/视频</button>
    <button class="ghost" id="openMaps">只开 地图</button>
    <button class="ghost" id="reset">重置我的标记</button>
  </div>
  <p class="note">用法：点每个渠道「去查↗」在新标签打开搜索页 → 看看有没有你 → 回来点「有/没有」。系统能自动查的已用蓝色预填，你可覆盖。标记只存在你本机浏览器。</p>
</header>

${groupHtml}

<div class="cta">
  <h3>发现很多渠道搜不到你？</h3>
  <p>互联网上没有你的"官方表达"，AI 和客户自然找不到你。我们帮你把真实资料，做成一个 AI 可读、可被引用的企业主页——先在互联网站住脚。</p>
  <a href="${esc(homepage)}" target="_blank" rel="noopener">查看/生成我的企业主页 →</a>
</div>

<footer>由「一搜商答 / 1so」生成 · ${today()}</footer>
</div>

<script>
const ITEMS = ${dataItems};
const COMPANY = ${JSON.stringify(name)};
const KEY = '1so:scan:' + COMPANY;
let mark = {};
try { mark = JSON.parse(localStorage.getItem(KEY) || '{}'); } catch(e){}

// 有效状态：用户标记优先，其次自动预填
function eff(it){
  if (mark[it.id]) return mark[it.id];              // 'hit' | 'miss' | 'opened'
  if (it.auto === 'hit') return 'auto-hit';
  if (it.auto === 'miss') return 'auto-miss';
  return '';                                        // 未查 / manual
}
function isHit(s){ return s === 'hit' || s === 'auto-hit'; }
function decided(s){ return s === 'hit' || s === 'miss' || s === 'auto-hit' || s === 'auto-miss'; }

function chipFor(it){
  const s = eff(it);
  if (s === 'hit') return ['chip hit','✓ 有'];
  if (s === 'miss') return ['chip miss','✗ 没有'];
  if (s === 'auto-hit') return ['chip auto','自动·搜到'];
  if (s === 'auto-miss') return ['chip auto','自动·没搜到'];
  if (s === 'opened') return ['chip opened','已打开·待确认'];
  return ['chip', it.auto === 'manual' ? '需自查' : '未查'];
}

function save(){ localStorage.setItem(KEY, JSON.stringify(mark)); }
function setMark(id, v){ if(mark[id]===v){ delete mark[id]; } else { mark[id]=v; } save(); render(); }
function onGo(it){ if(!mark[it.id]) { mark[it.id]='opened'; save(); } window.open(it.url,'_blank'); render(); }

function render(){
  let hit=0, dec=0;
  for(const it of ITEMS){ const s=eff(it); if(isHit(s)) hit++; if(decided(s)) dec++; }
  const pct = Math.round(hit / ITEMS.length * 100);
  document.getElementById('ringNum').textContent = pct;
  document.getElementById('ring').style.background = 'conic-gradient(var(--accent) '+pct+'%, #e5e7eb 0)';
  document.getElementById('sHit').textContent = hit;
  document.getElementById('sChecked').textContent = dec;
  // 每张卡片状态
  for(const it of ITEMS){
    const el = document.querySelector('.card[data-id="'+it.id+'"]'); if(!el) continue;
    const [cls,txt] = chipFor(it);
    const chip = el.querySelector('.chip'); chip.className = cls; chip.textContent = txt;
    chip.title = it.note ? ('自动判读依据：'+it.note) : '';
    const s = eff(it);
    el.querySelector('.yes').classList.toggle('on', s==='hit'||s==='auto-hit');
    el.querySelector('.no').classList.toggle('on', s==='miss'||s==='auto-miss');
  }
  // 分组计数
  const byCat={}; for(const it of ITEMS){ const s=eff(it); (byCat[it.grp]||=[0,0]); if(isHit(s))byCat[it.grp][0]++; byCat[it.grp][1]++; }
  document.querySelectorAll('[data-cat-cnt]').forEach(e=>{ const c=byCat[e.dataset.catCnt]||[0,0]; e.textContent='· 被搜到 '+c[0]+'/'+c[1]; });
}

function openList(list){
  if(!list.length) return;
  if(!confirm('即将打开 '+list.length+' 个标签页（浏览器可能拦截弹窗，请允许本页弹窗）。继续？')) return;
  let i=0; const t=setInterval(()=>{ if(i>=list.length){clearInterval(t);return;} const it=ITEMS.find(x=>x.url===list[i].url); if(it&&!mark[it.id]){mark[it.id]='opened';} window.open(list[i].url,'_blank'); i++; }, 300);
  save(); setTimeout(render, 400);
}
document.getElementById('openAll').onclick=()=>openList(ITEMS);
document.getElementById('openAI').onclick=()=>openList(ITEMS.filter(x=>x.grp==='aiSearch'||x.grp==='aiChat'));
document.getElementById('openSocial').onclick=()=>openList(ITEMS.filter(x=>x.grp==='social'||x.grp==='video'));
document.getElementById('openMaps').onclick=()=>openList(ITEMS.filter(x=>x.grp==='maps'));
document.getElementById('reset').onclick=()=>{ if(confirm('清空你在本机的所有标记？')){ mark={}; save(); render(); } };
document.querySelectorAll('.open-grp').forEach(b=>b.onclick=()=>openList(ITEMS.filter(x=>x.grp===b.dataset.grp)));
document.querySelectorAll('.card').forEach(el=>{
  const id=el.dataset.id; const it=ITEMS.find(x=>x.id===id);
  el.querySelector('.go').onclick=(e)=>{ e.preventDefault(); onGo(it); };
  el.querySelector('.yes').onclick=()=>setMark(id,'hit');
  el.querySelector('.no').onclick=()=>setMark(id,'miss');
});
render();
</script>
</body>
</html>`;
}

function card(it) {
  return `<div class="card" data-id="${esc(it.id)}" data-grp="${esc(it.category)}">
    <div class="top">
      <img src="${esc(it.favicon)}" alt="" loading="lazy" onerror="this.style.visibility='hidden'">
      <span class="nm">${esc(it.name)}</span>
      <span class="chip">未查</span>
    </div>
    <div class="acts">
      <a class="go" href="${esc(it.url)}" target="_blank" rel="noopener">去查 ↗</a>
      <button class="yes" type="button">✓ 有</button>
      <button class="no" type="button">✗ 没有</button>
    </div>
  </div>`;
}
