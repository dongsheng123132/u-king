/* U-King 法务页面语言切换。
   默认语言按浏览器 language 判定（非 zh 一律给英文）—— 这些页面存在的主要理由
   就是让海外刷卡买家看得懂，所以宁可默认英文，也不要让他看到一屏中文就关掉。
   选择记在 localStorage，跨三个页面一致。 */
(function () {
  var KEY = 'uking_legal_lang';

  function pick() {
    try {
      var saved = localStorage.getItem(KEY);
      if (saved === 'zh' || saved === 'en') return saved;
    } catch (e) { /* 隐私模式下 localStorage 会抛，忽略即可 */ }
    var nav = (navigator.language || navigator.userLanguage || 'en').toLowerCase();
    return nav.indexOf('zh') === 0 ? 'zh' : 'en';
  }

  function apply(lang) {
    var nodes = document.querySelectorAll('[data-lang]');
    for (var i = 0; i < nodes.length; i++) {
      var n = nodes[i];
      if (n.getAttribute('data-lang') === lang) n.classList.add('show');
      else n.classList.remove('show');
    }
    var btns = document.querySelectorAll('.langsw button');
    for (var j = 0; j < btns.length; j++) {
      var b = btns[j];
      if (b.getAttribute('data-set') === lang) b.classList.add('on');
      else b.classList.remove('on');
    }
    document.documentElement.setAttribute('lang', lang === 'zh' ? 'zh-CN' : 'en');
  }

  function set(lang) {
    try { localStorage.setItem(KEY, lang); } catch (e) { /* 同上 */ }
    apply(lang);
  }

  document.addEventListener('DOMContentLoaded', function () {
    apply(pick());
    var btns = document.querySelectorAll('.langsw button');
    for (var i = 0; i < btns.length; i++) {
      btns[i].addEventListener('click', function () {
        set(this.getAttribute('data-set'));
      });
    }
  });
})();
