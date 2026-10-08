(() => {
  const messages = {
    en: {
      description: 'Play RustScript-powered Shooter, Gomoku, and Xiangqi in your browser and edit their scripts live.',
      tagline: 'PLAY · EDIT · REPEAT', rulesYours: 'THE RULES ARE YOURS', language: 'Language', auto: 'Auto', heroCopy: 'Three games. One scripting engine.', heroCopySecond: 'Jump in and play, or rewrite the rules.',
      startShooter: 'Play Shooter', chooseGame: 'Choose a game ↓', gamesTitle: 'Choose your next game', gamesCount: '03 GAMES / IN YOUR BROWSER', play: 'Play now ↗',
      shooterAlt: 'A spaceship facing enemy waves in a starfield', shooterCategory: 'ARCADE / SHOOTER', shooterDescription: 'Dodge, fire automatically, and face enemy waves. Tune projectiles, enemies, and rewards live.', shooterControls: 'WASD / Arrow keys to move',
      gomokuAlt: 'A Gomoku board with the live script editor', gomokuCategory: 'STRATEGY / GOMOKU', gomokuDescription: 'Connect five stones against a scripted AI. Adjust its strategy and explore every move.', gomokuControls: 'Click an intersection to play · Undo and save supported',
      xiangqiAlt: 'A Xiangqi board with the live script editor', xiangqiCategory: 'STRATEGY / XIANGQI', xiangqiDescription: 'Cross the river and challenge the AI. Rules and decisions live in the scripts.', xiangqiControls: 'Click a piece, then its destination · Undo and save supported',
      aboutTitle: 'Play. Edit. Experiment.', aboutCopy: 'Edit the running RustScript in the panel on the right. Update rules, AI, and parameters and see the results of each experiment immediately.', openGomoku: 'Open the Gomoku playground ↗', footer: 'Desktop browser recommended · WebGL2',
      fullscreen: 'Fullscreen ↗', originalSize: 'Original size', fitWindow: 'Fit to window', gameNavigation: 'Choose a game', gameCanvas: '{game} game canvas', gamePageTitle: '{game} · RustScript Arcade',
      loading: 'Loading game', initialLoading: 'The first load downloads the game assets.', downloadProgress: 'Download progress', retry: 'Reload', loadingFailed: 'Game failed to load', loadingFailedDetail: '{error}. Please retry or use a desktop browser with WebGL2 support.', downloading: 'Downloading · {mb} MB{percent}', starting: 'Starting game…', resourceFailed: 'Asset request failed ({status})', webglUnavailable: 'WebGL2 is unavailable', fullscreenFailed: 'Fullscreen is unavailable in this browser. Try original size mode.',
      'shooter.title': 'Shooter', 'gomoku.title': 'Gomoku', 'xiangqi.title': 'Xiangqi / Chinese chess',
      'shooter.controls': 'WASD / Arrow keys to move · Automatic fire', 'gomoku.controls': 'Click the board to play · Challenge the AI', 'xiangqi.controls': 'Click a piece, then its destination',
      'shooter.note': 'Click the game canvas, then use the keyboard to move.', 'gomoku.note': 'Save / Load stores and restores the board and scripts in this browser.', 'xiangqi.note': 'Save / Load stores and restores the board and scripts in this browser.',
      scriptNote: 'Live editing and debugging are available. Use Debug, Step, Next, Out, Continue, and Locals in the script panel.'
    },
    zh: {
      description: '在浏览器里体验 RustScript 驱动的飞行射击、五子棋与中国象棋，并实时修改游戏脚本。',
      tagline: '游玩 · 编辑 · 实验', rulesYours: '游戏规则，由你定义', language: '语言', auto: '自动', heroCopy: '三个游戏，一个脚本引擎。', heroCopySecond: '直接开玩，也可以改写规则。',
      startShooter: '开始飞行射击', chooseGame: '选择游戏 ↓', gamesTitle: '选择你的下一局', gamesCount: '03 个游戏 / 在浏览器中体验', play: '开始游戏 ↗',
      shooterAlt: '飞船在星空中迎战敌方机群', shooterCategory: 'ARCADE / 飞行射击', shooterDescription: '闪避、自动开火、迎战机群。实时调整弹幕、敌人和奖励。', shooterControls: 'WASD / 方向键移动',
      gomokuAlt: '五子棋棋盘与实时脚本编辑器', gomokuCategory: 'STRATEGY / 五子棋', gomokuDescription: '与脚本 AI 对弈，连接五枚棋子。调整 AI，研究每一步。', gomokuControls: '点击交点落子 · 支持悔棋与保存',
      xiangqiAlt: '中国象棋棋盘与实时脚本编辑器', xiangqiCategory: 'STRATEGY / 中国象棋', xiangqiDescription: '跨越楚河汉界，与 AI 较量。规则与决策都在脚本中。', xiangqiControls: '点击棋子与目标位置 · 支持悔棋与保存',
      aboutTitle: '边玩，边改。', aboutCopy: '右侧编辑器直接修改正在运行的 RustScript。游戏规则、AI 和参数随脚本更新，让每一次实验都可以立即看到结果。', openGomoku: '打开五子棋实验台 ↗', footer: '建议使用桌面浏览器 · WebGL2',
      fullscreen: '全屏 ↗', originalSize: '原始大小', fitWindow: '适应窗口', gameNavigation: '选择游戏', gameCanvas: '{game} 游戏画面', gamePageTitle: '{game} · RustScript Arcade',
      loading: '正在加载游戏', initialLoading: '首次加载需要下载游戏资源。', downloadProgress: '下载进度', retry: '重新加载', loadingFailed: '游戏加载失败', loadingFailedDetail: '{error}。请重试，或使用支持 WebGL2 的桌面浏览器。', downloading: '下载中 · {mb} MB{percent}', starting: '正在启动游戏…', resourceFailed: '资源请求失败 ({status})', webglUnavailable: 'WebGL2 不可用', fullscreenFailed: '此浏览器无法进入全屏，可以使用原始大小模式。',
      'shooter.title': 'Shooter / 飞行射击', 'gomoku.title': 'Gomoku / 五子棋', 'xiangqi.title': 'Xiangqi / 中国象棋',
      'shooter.controls': 'WASD / 方向键移动 · 自动开火', 'gomoku.controls': '点击棋盘落子 · 与 AI 对弈', 'xiangqi.controls': '点击棋子，再点击目标位置',
      'shooter.note': '点击游戏画面后使用键盘移动。', 'gomoku.note': 'Save / Load 在当前浏览器保存、恢复棋局与脚本。', 'xiangqi.note': 'Save / Load 在当前浏览器保存、恢复棋局与脚本。',
      scriptNote: '支持实时脚本编辑与调试。在脚本面板中使用 Debug、Step、Next、Out、Continue 和 Locals。'
    }
  };
  const storageKey = 'rustscript-arcade-language';
  const parameters = new WeakMap();
  let preference = 'auto';
  try {
    const saved = localStorage.getItem(storageKey);
    if (saved === 'en' || saved === 'zh') preference = saved;
  } catch { /* Automatic selection also works when browser storage is disabled. */ }
  function detectLanguage() {
    const preferred = navigator.languages?.[0] || navigator.language || 'en';
    return /^zh(?:-|$)/i.test(preferred) ? 'zh' : 'en';
  }
  let language = preference === 'auto' ? detectLanguage() : preference;
  function t(key, values = {}) {
    const text = messages[language][key] ?? messages.en[key] ?? key;
    return text.replace(/\{(\w+)\}/g, (_, name) => {
      const value = values[name];
      return typeof value === 'function' ? value() : value ?? `{${name}}`;
    });
  }
  function render(element) {
    const values = parameters.get(element) || {};
    for (const attr of ['text', 'alt', 'aria-label', 'content']) {
      const key = element.getAttribute(attr === 'text' ? 'data-i18n' : `data-i18n-${attr}`);
      if (!key) continue;
      if (attr === 'text') element.textContent = t(key, values);
      else element.setAttribute(attr, t(key, values));
    }
  }
  function setText(element, key, values = {}) {
    element.setAttribute('data-i18n', key);
    parameters.set(element, values);
    render(element);
  }
  function apply() {
    document.documentElement.lang = language === 'zh' ? 'zh-CN' : 'en';
    const game = document.body?.dataset.game;
    if (game) {
      document.title = t('gamePageTitle', {game: t(`${game}.title`)});
      const canvas = document.querySelector('#game-canvas');
      if (canvas) parameters.set(canvas, {game: () => t(`${game}.title`)});
    }
    document.querySelectorAll('[data-i18n], [data-i18n-alt], [data-i18n-aria-label], [data-i18n-content]').forEach(render);
    document.querySelectorAll('[data-language-select]').forEach(select => { select.value = preference; });
  }
  function setLanguage(value) {
    if (!['auto', 'en', 'zh'].includes(value)) return;
    preference = value;
    language = value === 'auto' ? detectLanguage() : value;
    try {
      if (value === 'auto') localStorage.removeItem(storageKey);
      else localStorage.setItem(storageKey, value);
    } catch { /* Keep the current choice for this page without persistent storage. */ }
    apply();
  }
  document.documentElement.lang = language === 'zh' ? 'zh-CN' : 'en';
  window.arcadeI18n = {t, setText, setLanguage, apply};
  document.addEventListener('DOMContentLoaded', () => {
    apply();
    document.querySelectorAll('[data-language-select]').forEach(select => {
      select.addEventListener('change', () => setLanguage(select.value));
    });
  });
  window.addEventListener('languagechange', () => {
    if (preference === 'auto') { language = detectLanguage(); apply(); }
  });
})();
