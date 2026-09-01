// origin-kernel · HLC 混合逻辑时钟（零依赖）
// 判据3：判定不信任墙钟——拨快拨慢系统时钟不影响裁决。
// 语义：事件全序 = (logical, node) 字典序；physical 仅作展示与粗校。
export class HLC {
  constructor(nodeId, nowMs = () => Date.now()) {
    this.node = String(nodeId);
    this._now = nowMs;
    this.l = 0;          // 逻辑计数
    this.phy = this._now(); // 上次物理毫秒
  }
  // 本地发生一个事件
  tick() {
    const p = this._now();
    this.l = p > this.phy ? 0 : this.l + 1;
    this.phy = Math.max(this.phy, p);
    return { l: this.l, phy: this.phy, node: this.node };
  }
  // 收到远端时间戳后合并
  recv(remote) {
    const p = this._now();
    const maxPhy = Math.max(this.phy, remote.phy ?? 0, p);
    if (maxPhy === this.phy && maxPhy === remote.phy) this.l = Math.max(this.l, remote.l) + 1;
    else if (maxPhy === this.phy) this.l = this.l + 1;
    else if (maxPhy === remote.phy) this.l = remote.l + 1;
    else this.l = 0;
    this.phy = maxPhy;
    return { l: this.l, phy: this.phy, node: this.node };
  }
}

// 全序比较：先比物理毫秒，再比逻辑计数，最后比节点名
export function tsCmp(a, b) {
  if (a.phy !== b.phy) return a.phy - b.phy;
  if (a.l !== b.l) return a.l - b.l;
  return a.node < b.node ? -1 : a.node > b.node ? 1 : 0;
}
