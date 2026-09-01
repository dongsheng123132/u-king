// origin-kernel · 五原语最小内核：Signal/Capability/Lease/Receipt/Realm
// 北极星挑战：离线可撤销授权 —— 断网也能干活，撤权一定生效，谁都能离线验证。
// 零依赖纯 Node。Realm 首版退化＝一台设备一个域；网络用事件数组模拟（消息可延迟、乱序、重复）。
//
// P0 修复（2026-08-26 双模型终审后）：
// - F9：每节点维护自己的签名事件链（prevHash+seq 进哈希与签名），sealLedger() 签发账本头；
//       审计方凭 heads 校验「无缺号无截断」——抽掉任何一条（含链尾 revoke）即检出。
// - F6：replay 入口按事件哈希去重；epoch=去重后不同 revoke 的基数；
//       totalOrder 对相同 eventId 返回 0（满足反对称性）。重复投递不再改变判决内容。
import { HLC, tsCmp } from './clock.mjs';
import { keygen, sha256, sign, verify } from './sig.mjs';

export const DENY_REASONS = ['UNKNOWN_LEASE', 'REVOKED_EPOCH', 'LEASE_EXPIRED', 'STALE_VERSION', 'CAP_NOT_ALLOWED', 'IDEM_CONFLICT', 'NOT_GRANTEE', 'NOT_CURRENT_GRANTEE'];

let seq = 0;

export class Node {
  // peers 是第二参；为兼容已有测试/调用，第二参传函数时仍视为 nowMs。
  // 同时接受 Node(id, nowMs, peers) 这个旧式测试友好的扩展写法。
  constructor(id, peersOrNow = {}, maybeNow = () => Date.now()) {
    const nowMs = typeof peersOrNow === 'function'
      ? peersOrNow
      : (typeof maybeNow === 'function' ? maybeNow : () => Date.now());
    const peers = typeof peersOrNow === 'function'
      ? (typeof maybeNow === 'object' && maybeNow !== null ? maybeNow : {})
      : peersOrNow;
    this.id = String(id);
    this.keys = keygen(this.id); // 生产环境私钥不入账本；demo 内联便于自证
    this.hlc = new HLC(this.id, nowMs);
    this.ledger = [];            // 本节点已知事件（含远端同步来的）
    this._ownSeq = 0;            // 本节点自签事件的序号（F9 链连续性锚点）
    this._lastOwnHash = null;    // 本节点上一条自签事件的哈希
    this.peers = peers;
  }
  get pub() { return this.keys.pub; }

  _emit(type, body) {
    const ts = this.hlc.tick();
    const e = {
      eventId: `${this.id}:${ts.phy}:${ts.l}:${++seq}`,
      type, realm: this.id, actor: this.id, ts,
      seq: this._ownSeq,                       // F9：本节点链内序号
      prevHash: this._lastOwnHash ?? 'GENESIS',// F9：指向上一条【自签】事件
      ...body,
    };
    e.hash = sha256(coreFields(e));            // prevHash/seq 已进哈希与签名
    e.sig = sign(this.keys.priv, { type: e.type, ts: e.ts, bodyHash: e.hash });
    this._ownSeq += 1;
    this._lastOwnHash = e.hash;
    this.ledger.push(e);
    return e;
  }

  // Capability→Lease：授权方签发临时租约（notAfter 由授权方时钟决定）
  grant({ to, caps, ttlMs = 3600_000, resource }) {
    if (!Array.isArray(caps) || caps.length === 0) throw new Error('caps required');
    if (!resource) throw new Error('resource required');
    const ts = this.hlc.tick();
    return this._emit('lease.grant', {
      leaseId: `l-${this.id}-${++seq}`, grantee: to, caps: [...caps], resource,
      notAfter: { l: 0, phy: ts.phy + ttlMs, node: ts.node }, grantTs: ts,
      depth: 0,
    });
  }

  // 委托：同 leaseId@grantTs 键（根撤销天然级联）；能力只能收窄（本地 fail-fast）；
  // 期限继承父租约（不可延长）；depth 计数防审计链不可读。
  handoff({ leaseId, to, caps = null, meta = null }, { depthLimit = 4 } = {}) {
    const g = this._findGrant(leaseId);
    if (!g) throw new Error('unknown lease: ' + leaseId);
    if (this.id !== g.grantee) throw new Error('NOT_CURRENT_GRANTEE');
    let newCaps = g.caps;
    if (caps !== null) {
      const parent = new Set(g.caps);
      if (!caps.every((c) => parent.has(c))) throw new Error('CAPS_NOT_SUBSET'); // 收窄 fail-fast
      newCaps = [...caps];
    }
    const depth = (g.depth ?? 0) + 1;
    if (depth > depthLimit) throw new Error('DEPTH_LIMIT_EXCEEDED');
    return this._emit('lease.handoff', {
      leaseId, to, caps: newCaps, resource: g.resource,
      notAfter: g.notAfter,                    // 继承＝min(parent, requested)，不可延长
      grantTs: g.grantTs, grantee: to,
      depth,
      ...(meta ? { meta } : {}),               // 护照引用等元数据：被签名覆盖但不参与判决
    });
  }

  // Signal：撤销是一条事实。同租约第 N 条【不同】撤销 → epoch N（重复投递不计）。
  revoke({ leaseId }) {
    const g = this._findGrant(leaseId);
    if (!g) throw new Error('unknown lease: ' + leaseId);
    return this._emit('lease.revoke', { leaseId, resource: g.resource, grantTs: g.grantTs });
  }

  // 同步远端事件：合并 HLC 时钟并入账。在线签收的本质——收到即建立因果关系。
  // 远端事件保持其原链归属（owner 的 seq/prevHash 不重算）。
  receive(e) {
    if (sha256(coreFields(e)) !== e.hash) throw new Error('HASH_MISMATCH');
    const pub = this.peers[e.actor];
    if (pub && !verify(pub, { type: e.type, ts: e.ts, bodyHash: e.hash }, e.sig))
      throw new Error('BAD_SIGNATURE');
    this.hlc.recv(e.ts);
    this.ledger.push(e);
    return this;
  }

  // 授权方签收某次行使：签发 action.ack 凭证。撤销前被签收的操作不可再被撤销。
  ackExercise(ev) {
    return this._emit('action.ack', { forEvent: ev.eventId, leaseId: ev.leaseId, grantTs: ev.grantTs });
  }

  // 离线行使：本地先落 pending 回执（可用性：不 fail-closed）
  exercise({ leaseId, action, idem, expectVersion = null }) {
    const g = this._findGrant(leaseId);
    return this._emit('action.exercise', {
      leaseId, action, idem: String(idem), expectVersion,
      resource: g ? g.resource : null, grantTs: g ? g.grantTs : null,
    });
  }

  // F9：签发账本头——「我的账本共 N 条、头哈希是 X」。审计据此检出缺号与截断。
  sealLedger() {
    const ts = this.hlc.tick();
    const head = {
      kind: 'ledger.head', owner: this.id,
      seq: this._ownSeq, head: this._lastOwnHash ?? 'GENESIS', ts,
    };
    head.sig = sign(this.keys.priv, { owner: head.owner, seq: head.seq, head: head.head, ts: head.ts });
    return head;
  }

  _findGrant(leaseId) {
    const cands = this.ledger.filter((e) => (e.type === 'lease.grant' || e.type === 'lease.handoff') && e.leaseId === leaseId);
    if (!cands.length) return null;
    return cands.sort((a, b) => tsCmp(b.ts, a.ts))[0];
  }
}

function coreFields(e) {
  const { hash, sig, ...rest } = e; // prevHash/seq 保留在签名核心字段内（F9 关键）
  return rest;
}

// ── 权威裁决：确定性重放。结论与到达顺序、重复次数、哪端先上线无关。──
export function replay(allEvents, opts = {}) {
  // F6：入口按哈希去重——重复投递不产生第二次状态迁移
  const seen = new Set();
  const uniq = [];
  for (const e of allEvents) {
    if (seen.has(e.hash)) continue;
    seen.add(e.hash);
    uniq.push(e);
  }
  const ordered = causalOrder(uniq);
  const orderedPos = new Map(ordered.map((e, i) => [e.eventId, i]));
  const unauthorizedRevokes = [];
  const unauthorizedIds = new Set();
  const deniedEvents = [];
  const deniedIds = new Set();
  const unverifiedAcks = [];
  const unverifiedAckIds = new Set();
  let overflow = 0;
  const noteCapped = (list, ids, eventId, value) => {
    if (ids.has(eventId)) return;
    ids.add(eventId);
    if (list.length < 500) {
      list.push(value);
      return;
    }
    overflow += 1;
    // 各扫描阶段可能不同；容量满时保留因果序最早的 500 条，而非先扫到的 500 条。
    let latest = 0;
    for (let i = 1; i < list.length; i++) {
      const at = typeof list[i] === 'string' ? list[i] : list[i].eventId;
      const latestAt = typeof list[latest] === 'string' ? list[latest] : list[latest].eventId;
      if (orderedPos.get(at) > orderedPos.get(latestAt)) latest = i;
    }
    const latestId = typeof list[latest] === 'string' ? list[latest] : list[latest].eventId;
    if (orderedPos.get(eventId) < orderedPos.get(latestId)) list[latest] = value;
  };
  const noteUnauthorized = (e) => {
    noteCapped(unauthorizedRevokes, unauthorizedIds, e.eventId, e.eventId);
  };
  const noteDenied = (e, reason) => noteCapped(deniedEvents, deniedIds, e.eventId, { eventId: e.eventId, reason });
  const noteUnverifiedAck = (e) => noteCapped(unverifiedAcks, unverifiedAckIds, e.eventId, e.eventId);
  const isPermissionEvent = (e) => e.type === 'lease.grant' || e.type === 'lease.handoff' || e.type === 'lease.revoke';
  const invalidPermissionKeys = new Set();
  // 有已知公钥时，坏签名的权限事件不参与任何状态迁移；未知公钥仍保留离线可用性。
  const authorized = ordered.filter((e) => {
    const pub = opts.pubs?.[e.actor];
    if (isPermissionEvent(e) && pub && (
      sha256(coreFields(e)) !== e.hash ||
      !verify(pub, { type: e.type, ts: e.ts, bodyHash: e.hash }, e.sig)
    )) {
      noteUnauthorized(e);
      invalidPermissionKeys.add(`${e.leaseId}@${JSON.stringify(e.grantTs)}`);
      return false;
    }
    return true;
  });
  // 伪造 grant/handoff 被滤除后重建依赖，不能让它残留在 defIdx 中阻塞合法 revoke。
  const evs = causalOrder(authorized);
  // 因果全序位次表:跨节点事件先后一律查此表(HLC 的 l 跨节点不可直接比较)
  const pos = new Map();
  evs.forEach((e, i) => pos.set(e.eventId, i));
  const posOf = (eventId) => pos.get(eventId) ?? -1;

  const leases = new Map();      // `${leaseId}@${grantTs}` → 状态
  const verdicts = new Map();    // eventId → {ok, reason}
  const idemFirst = new Map();   // lease+grant+actor+idem → { fp, verdict }
  const versions = new Map();    // resource → 版本号
  const effects = [];
  const acks = new Map();        // `${forEvent}|${leaseId}@${grantTs}` → action.ack 候选数组
  const roots = new Map();       // `${leaseId}@${grantTs}` → 根签发者

  const kOf = (e) => `${e.leaseId}@${JSON.stringify(e.grantTs)}`;
  const ackKOf = (e) => `${e.forEvent}|${kOf(e)}`;
  const granteeAt = (st, ts) => {
    // exercise 已按 lease key 因果依赖根 grant；根受托人由该依赖确立，不能被离线端
    // 未 merge HLC 时恰好相同的逻辑时间反向否掉。之后的 handoff 才严格按 sinceTs 分界。
    let grantee = st.granteeTimeline[0]?.grantee ?? null;
    for (const point of st.granteeTimeline.slice(1)) {
      if (tsCmp(point.sinceTs, ts) <= 0) grantee = point.grantee;
      else break;
    }
    return grantee;
  };
  const addGrantee = (st, sinceTs, grantee) => {
    st.granteeTimeline.push({ sinceTs, grantee });
    st.granteeTimeline.sort((a, b) => tsCmp(a.sinceTs, b.sinceTs));
  };

  // 第一遍：先独立建立根签发者索引；ack 即便在输入中排到 grant 前，也不会误判为无根。
  for (const e of evs) {
    if (e.type === 'lease.grant' && !roots.has(kOf(e)))
      roots.set(kOf(e), e.actor);
  }

  // 第二遍：收授权/委托/签收/撤销，建立租约状态与签收候选。
  for (const e of evs) {
    if (e.type === 'action.ack') {
      const k = kOf(e);
      const root = roots.get(k);
      if (!root || e.actor !== root) {
        noteUnauthorized(e);
        noteDenied(e, 'UNAUTHORIZED_ACK');
        continue;
      }
      const pub = opts.pubs?.[e.actor];
      if (pub && (
        sha256(coreFields(e)) !== e.hash ||
        !verify(pub, { type: e.type, ts: e.ts, bodyHash: e.hash }, e.sig)
      )) continue;
      if (!pub) noteUnverifiedAck(e);
      const ak = ackKOf(e);
      if (!acks.has(ak)) acks.set(ak, []);
      acks.get(ak).push(e);
      continue;
    }
    if (e.type === 'lease.grant') {
      const k = kOf(e);
      const prev = leases.get(k);
      if (!prev) {
        leases.set(k, {
          caps: e.caps, resource: e.resource, notAfter: e.notAfter,
          revokePosList: [],
          // grantTs 是租约的生效时点；e.ts 是封装该 grant 事件时的链时间戳。
          // 兼容离线方先持有 grant 再落 exercise 的既有因果模型，判受托人以 grantTs 起算。
          granteeTimeline: [{ sinceTs: e.grantTs, grantee: e.grantee }],
        });
      }
    } else if (e.type === 'lease.handoff') {
      const k = kOf(e);
      const st = leases.get(k);
      if (!st || e.actor !== granteeAt(st, e.ts)) {
        noteUnauthorized(e);
        noteDenied(e, 'NOT_CURRENT_GRANTEE');
        continue;
      }
      st.caps = intersect(st.caps, e.caps); // 单调：只减不增
      addGrantee(st, e.ts, e.to ?? e.grantee);
    } else if (e.type === 'lease.revoke') {
      const k = kOf(e);
      const st = leases.get(k);
      const root = roots.get(k);
      if (st && (!root || e.actor !== root)) {
        noteUnauthorized(e);
        noteDenied(e, 'UNAUTHORIZED_REVOKE');
      } else if (st) st.revokePosList.push(posOf(e.eventId));
    }
  }

  const hasEffectiveAck = (e, st) => {
    const candidates = acks.get(`${e.eventId}|${kOf(e)}`) ?? [];
    // 见证成立的判据(两条同时满足,全部用【因果位次】而非自报 ts):
    // ① pos(ack) > pos(exercise):签收见证的是「已发生」的操作。HLC 的 l 各节点独立
    //    计数,跨节点裸比无时序真相;位次来自确定性 causalOrder,与输入顺序无关。
    // ② pos(ack) < pos(该租约全部授权撤销)(E3 终局性):挡「撤销后补签/伪造时钟翻案」。
    //    假时钟把 ack 的自报 phy 改早没用——位次由事件在 trace 中的因果关系决定。
    return candidates.some((ack) =>
      posOf(ack.eventId) > posOf(e.eventId) &&
      !st.revokePosList.some((rvPos) => posOf(ack.eventId) >= rvPos));
  };
  // 第三遍：判操作。安全优先语义——撤销前未被授权方签收的操作一律拒绝；
  // 被签收过的操作视为「已见证」，即使 trace 中操作事件与撤销并发也可生效。
  for (const e of evs) {
    if (e.type !== 'action.exercise') continue;
    const st = e.grantTs ? leases.get(`${e.leaseId}@${JSON.stringify(e.grantTs)}`) : null;
    const idemK = `${e.leaseId}@${JSON.stringify(e.grantTs)}|${e.actor}|${e.idem}`;
    const fp = sha256({ action: e.action, expectVersion: e.expectVersion, resource: e.resource });
    let v;
    let skipIdem = false;
    const acked = st && hasEffectiveAck(e, st);
    // 撤销判定用【存在性】而非时点比较(G4终版):exercise 与 revoke 无因果路径时二者并发,
    // 按 revoke-wins 语义一律拒——若按时点比,pc 的 HLC 从未 merge 过 revoke,
    // 先后完全取决于两台设备的墙钟对齐,判决随毫秒抖动翻转(实测 demo flaky),
    // 违反判据3「判定不信任墙钟」。存在性判定确定且只严不松;
    // 「不能被后续追加的撤销放宽历史」由 hasEffectiveAck 的 ack-early-than-all-revokes 窗口保证。
    const authRevoked = st ? st.revokePosList.length > 0 : false;
    if (!st) { v = { ok: false, reason: 'UNKNOWN_LEASE' }; skipIdem = true; }
    else if (e.actor !== granteeAt(st, e.ts)) { v = { ok: false, reason: 'NOT_GRANTEE' }; skipIdem = true; }
    // 已知公钥揭出伪造权限事件时，不能靠“丢掉该事件”反而放宽同一租约的裁决。
    else if (invalidPermissionKeys.has(kOf(e))) { v = { ok: false, reason: 'REVOKED_EPOCH' }; skipIdem = true; }
    // epoch 报告值=因果位次上先于本次 exercise 的授权撤销数（时点口径），
    // 拒绝判据本身仍是存在性（revoke-wins），两者不矛盾：epoch 只做展示口径。
    else if (authRevoked && !acked) {
      const exPos = posOf(e.eventId);
      const priorRevokes = st.revokePosList.filter((rvPos) => rvPos < exPos).length;
      v = { ok: false, reason: 'REVOKED_EPOCH', epoch: Math.max(priorRevokes, 1) };
      skipIdem = true;
    }
    // 过期判定保留时点比较:notAfter 由授权方签租约时一次性写死(自己时钟自洽),
    // 与行使端墙钟无关,确定。
    else if (tsCmp(e.ts, st.notAfter) >= 0 && !acked) { v = { ok: false, reason: 'LEASE_EXPIRED' }; skipIdem = true; }
    else if (!st.caps.includes(e.action)) { v = { ok: false, reason: 'CAP_NOT_ALLOWED' }; skipIdem = true; }
    if (skipIdem) {
      noteDenied(e, v.reason);
      verdicts.set(e.eventId, v);
      continue;
    }
    {
      const cur = versions.get(st.resource) ?? 0;
      if (e.expectVersion !== null && e.expectVersion !== cur) v = { ok: false, reason: 'STALE_VERSION', currentVersion: cur };
      else v = { ok: true, reason: null };
    }
    // 幂等：同键恒等回放首次判定；效果只记一次
    const seenIdem = idemFirst.get(idemK);
    if (seenIdem !== undefined) {
      v = seenIdem.fp === fp
        ? { ...seenIdem.verdict, duplicate: true }
        : { ok: false, reason: 'IDEM_CONFLICT' };
    } else {
      idemFirst.set(idemK, { fp, verdict: v });
      if (v.ok && st) { versions.set(st.resource, (versions.get(st.resource) ?? 0) + 1); effects.push(e.eventId); }
    }
    if (!v.ok) noteDenied(e, v.reason);
    verdicts.set(e.eventId, v);
  }
  // 三类审计数组均按同一条因果全序输出，绝不泄漏 Map/分遍扫描的偶然顺序。
  const byTrace = (a, b) => orderedPos.get(typeof a === 'string' ? a : a.eventId) - orderedPos.get(typeof b === 'string' ? b : b.eventId);
  unauthorizedRevokes.sort(byTrace);
  deniedEvents.sort(byTrace);
  unverifiedAcks.sort(byTrace);
  return { verdicts, effects, versions: Object.fromEntries(versions), unauthorizedRevokes, deniedEvents, unverifiedAcks, overflow };
}

// ── 第三方离线验证：trace + 各 actor 公钥 + 各节点账本头，复现链完整性、签名与裁决 ──
// 无 heads ⇒ 无法担保完整性 ⇒ FAIL（被审方必须交付 sealLedger 凭证）。
export function verifyTrace(trace, pubs, heads = null) {
  const problems = [];

  // 按 owner 分组验链：序号连续 + prevHash 链接 + 哈希复算 + 逐条验签
  const groups = new Map();
  for (const e of trace) {
    if (!groups.has(e.actor)) groups.set(e.actor, []);
    groups.get(e.actor).push(e);
  }
  for (const [actor, evs] of groups) {
    evs.sort((a, b) => (a.seq ?? 0) - (b.seq ?? 0));
    let expectHash = 'GENESIS';
    let expectSeq = 0;
    for (const e of evs) {
      if ((e.seq ?? 0) !== expectSeq) problems.push({ eventId: e.eventId, problem: 'SEQ_GAP', expected: expectSeq, actual: e.seq });
      if ((e.prevHash ?? 'GENESIS') !== expectHash) problems.push({ eventId: e.eventId, problem: 'CHAIN_BREAK' });
      const pub = pubs[actor];
      if (!pub) problems.push({ eventId: e.eventId, problem: 'NO_PUBKEY' });
      else {
        if (!verify(pub, { type: e.type, ts: e.ts, bodyHash: e.hash }, e.sig))
          problems.push({ eventId: e.eventId, problem: 'BAD_SIGNATURE' });
        if (sha256(coreFields(e)) !== e.hash)
          problems.push({ eventId: e.eventId, problem: 'HASH_MISMATCH' });
      }
      expectHash = e.hash;
      expectSeq = (e.seq ?? 0) + 1;
    }
  }

  // heads：缺交/错主/数量对不上/头对不上/有 actor 没交头 —— 全部检出（含链尾截断）
  if (!Array.isArray(heads) || heads.length === 0) {
    problems.push({ problem: 'NO_HEADS', hint: '审计需要各节点 sealLedger() 签发的账本头' });
  } else {
    for (const h of heads) {
      const pub = pubs[h.owner];
      if (!pub || !verify(pub, { owner: h.owner, seq: h.seq, head: h.head, ts: h.ts }, h.sig)) {
        problems.push({ problem: 'BAD_HEAD_SIG', owner: h.owner });
        continue;
      }
      const g = groups.get(h.owner) ?? [];
      if (g.length !== h.seq || (g.length ? g[g.length - 1].hash : 'GENESIS') !== h.head)
        problems.push({ problem: 'HEAD_MISMATCH', owner: h.owner, declared: h.seq, actual: g.length });
    }
    for (const actor of groups.keys())
      if (!heads.some((h) => h.owner === actor)) problems.push({ problem: 'ACTOR_WITHOUT_HEAD', actor });
  }

  const r = replay(trace, { pubs });
  return {
    ok: problems.length === 0,
    problems,
    verdicts: Object.fromEntries(r.verdicts),
    versions: r.versions,
    unauthorizedRevokes: r.unauthorizedRevokes,
    deniedEvents: r.deniedEvents,
    unverifiedAcks: r.unverifiedAcks,
    overflow: r.overflow,
  };
}

// 多节点账本合并 → 因果保序全序输出。各事件保留自己 owner 链的 prevHash/seq（不再改写）。
export function chainTrace(...ledgers) {
  return causalOrder(ledgers.flat());
}

// 因果保序的全序：先按数据依赖拓扑分层（租约事件必须晚于其授权事件），
// 同层内按 HLC 全序排；无依赖事件照常全序。保证 grant 先于一切引用它的事件被重放。
function causalOrder(events) {
  // 合并=并集：同一事件经复制存在于多个节点账本是常态，按哈希去重
  const seenHash = new Set();
  const list = [];
  for (const e of events) {
    if (seenHash.has(e.hash)) continue;
    seenHash.add(e.hash);
    list.push(e);
  }
  const base = totalOrder(list);
  const defIdx = new Map();   // `${leaseId}@${grantTsJSON}` → 首条 grant/handoff 的 eventId
  for (const e of base) {
    if ((e.type === 'lease.grant' || e.type === 'lease.handoff') && e.grantTs) {
      const k = `${e.leaseId}@${JSON.stringify(e.grantTs)}`;
      if (!defIdx.has(k)) defIdx.set(k, e.eventId);
    }
  }
  // ack 的 forEvent 引用是协议层因果边:被签收的 exercise 因果先于 ack。
  // (HLC 各节点 l 独立计数,tsCmp 全序跨节点不代表因果先后——ack 必须排在它签收的操作之后。)
  const ackDep = new Map();   // forEvent eventId → 该 action.ack
  for (const e of base) {
    if (e.type === 'action.ack' && e.forEvent) {
      if (!ackDep.has(e.forEvent)) ackDep.set(e.forEvent, []);
      ackDep.get(e.forEvent).push(e);
    }
  }
  const placedIds = new Set();
  const placed = new Set();
  const out = [];
  const remaining = [...base];
  let progressed = true;
  while (remaining.length && progressed) {
    progressed = false;
    for (let i = 0; i < remaining.length; i++) {
      const e = remaining[i];
      if (e.type === 'action.exercise' || e.type === 'action.ack' || e.type === 'lease.handoff' || e.type === 'lease.revoke') {
        const depKey = `${e.leaseId}@${JSON.stringify(e.grantTs)}`;
        const depId = defIdx.get(depKey);
        if (depId && !placed.has(depId) && depId !== e.eventId) continue; // 依赖未就位，跳过
      }
      if (e.type === 'action.ack' && e.forEvent && !placed.has(e.forEvent)) {
        // 被签收事件在本 trace 里存在但还没入序 → ack 必须等它(见证因果边);
        // 若被签收事件根本不在 trace(截断/选择性出示),ack 不阻塞,正常入序。
        const src = list.find((c) => c.eventId === e.forEvent);
        if (src) continue;
      }
      out.push(e); placed.add(e.eventId); remaining.splice(i, 1);
      progressed = true;
      break;
    }
  }
  return out.concat(remaining); // 剩余为循环依赖（不应发生），兜底输出
}

function totalOrder(events) {
  return [...events].sort((a, b) => {
    const c = tsCmp(a.ts, b.ts);
    if (c !== 0) return c;
    if (a.eventId === b.eventId) return 0; // F6：相同事件比较器必须返回 0（反对称性）
    return a.eventId < b.eventId ? -1 : 1;
  });
}

function intersect(parent, child) {
  const p = new Set(parent);
  return child.filter((c) => p.has(c));
}

export { tsCmp };
