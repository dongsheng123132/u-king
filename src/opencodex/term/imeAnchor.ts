/**
 * 把输入法候选条钉回光标处。
 *
 * ## 症状
 * 在 U-CLI 里跑 Claude Code，打中文时候选条（和拼音预览）飘在屏幕上方、盖住别的输出行，
 * 而同一台机器的 Windows Terminal 里一切正常。
 *
 * ## 真因（`scripts/check-term-ime.mjs` 量得到，不是推测）
 * 候选条**不由我们画** —— 操作系统把它贴在当前聚焦元素的插入符处。xterm.js 用一个
 * `opacity:0` 的隐藏 `<textarea>` 收键盘，靠 `_syncTextArea()` 把它挪到光标格上，而那段算式是：
 *
 *     top = buffer.y * 行高
 *
 * `buffer.y` 是**相对缓冲区顶行 `ybase`** 的行号，可屏幕上显示的顶行是 `ydisp`。
 * 没滚动时两者相等所以看不出来；往回滚了 N 行，textarea 就比真光标高 N 行 —— 一比一。
 *
 * ## 为什么偏偏是我们中招
 * 原生终端里敲任何键都会先跳回底部，位置差自然消失。而输入法**组字期间不产生数据**，
 * xterm 的 `scrollOnUserInput` 是在数据写入时触发的，组字不触发 ——
 * 于是「往上翻着看输出，随手开始打中文」这个最常见的动作正好落进缺口。
 *
 * ## 修法
 * 组字一开始就跳回底部：`ydisp === ybase`，xterm 那套算式的前提成立，位置自然对。
 * 这同时也是原生终端的既有行为（敲字就回到提示符），不是为了绕 bug 硬造的规矩。
 *
 * 只用公开 API（`term.textarea` / `term.scrollToBottom`），不碰 xterm 私有方法 ——
 * 私有方法下次升级就没了，而这条链路一旦坏掉是**看不出来的**（候选条位置没人写测试）。
 */

/** 只要求这两样，方便跑道拿一个真 xterm 实例直接喂进来。 */
type ImeAnchorTarget = {
  textarea?: HTMLTextAreaElement;
  scrollToBottom(): void;
};

/**
 * 给一个 xterm 实例装上候选条锚定。返回卸载函数（拆终端时调，别泄漏监听）。
 *
 * `compositionupdate` 也要跟：组字期间上游还在刷输出，一次 `compositionstart` 之后
 * 视图可能又被推走。反复调 `scrollToBottom` 在已经贴底时是 no-op，不会打架。
 */
export function anchorImeToCursor(term: ImeAnchorTarget): () => void {
  const ta = term.textarea;
  if (!ta) return () => {};
  const pin = () => {
    try {
      term.scrollToBottom();
    } catch {
      /* 终端已拆 —— 组字中途关标签会走到这，不能让它把界面带走 */
    }
  };
  ta.addEventListener("compositionstart", pin);
  ta.addEventListener("compositionupdate", pin);
  return () => {
    ta.removeEventListener("compositionstart", pin);
    ta.removeEventListener("compositionupdate", pin);
  };
}
