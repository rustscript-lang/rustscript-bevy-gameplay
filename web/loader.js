const game = document.body.dataset.game;
const canvas = document.querySelector('#game-canvas');
const stage = document.querySelector('#stage');
const surface = document.querySelector('#game-surface');
const loading = document.querySelector('#loading');
const detail = document.querySelector('#loading-detail');
const progress = document.querySelector('#progress');
const retry = document.querySelector('#retry');
const scaleToggle = document.querySelector('#scale-toggle');
const width = game === 'shooter' ? 1150 : 1320;
const height = game === 'shooter' ? 1000 : 980;
let originalSize = false;

document.querySelector(`[data-link="${game}"]`).setAttribute('aria-current', 'page');
surface.style.width = `${width}px`;
surface.style.height = `${height}px`;
function fit() {
  const scale = originalSize ? 1 : Math.min(stage.clientWidth / width, stage.clientHeight / height, 1);
  surface.style.transform = `scale(${scale})`;
  surface.style.left = `${Math.max(0, (stage.clientWidth - width * scale) / 2)}px`;
  surface.style.top = `${Math.max(0, (stage.clientHeight - height * scale) / 2)}px`;
}
new ResizeObserver(() => requestAnimationFrame(fit)).observe(stage);
scaleToggle.addEventListener('click', () => {
  originalSize = !originalSize;
  stage.style.overflow = originalSize ? 'auto' : 'hidden';
  scaleToggle.textContent = originalSize ? '适应窗口' : '原始大小';
  scaleToggle.setAttribute('aria-pressed', String(originalSize));
  fit();
});
document.querySelector('#fullscreen').addEventListener('click', async () => {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else await document.querySelector('.game-main').requestFullscreen();
  } catch { detail.textContent = '此浏览器无法进入全屏，可以使用原始大小模式。'; }
});
retry.addEventListener('click', () => location.reload());
canvas.addEventListener('pointerdown', () => canvas.focus());

function fail(error) {
  console.error(error);
  document.body.dataset.status = 'error';
  loading.hidden = false;
  document.querySelector('#loading-title').textContent = '游戏加载失败';
  detail.textContent = `${error.message || error}。请重试，或使用支持 WebGL2 的桌面浏览器。`;
  progress.hidden = true;
  retry.hidden = false;
}
window.addEventListener('error', event => {
  // winit uses an exception to transfer control to its browser event loop.
  if (String(event.error || event.message).includes('Using exceptions for control flow')) {
    event.preventDefault();
    return;
  }
  fail(event.error || new Error(event.message));
});
window.addEventListener('unhandledrejection', event => fail(event.reason));

async function download(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`资源请求失败 (${response.status})`);
  const length = Number(response.headers.get('Content-Length'));
  if (!response.body) return new Uint8Array(await response.arrayBuffer());
  const reader = response.body.getReader();
  const chunks = [];
  let received = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    received += value.byteLength;
    const percent = length ? Math.min(100, Math.round(received / length * 100)) : null;
    if (percent === null) progress.removeAttribute('value');
    else progress.value = percent;
    detail.textContent = `下载中 · ${(received / 1048576).toFixed(1)} MB${percent === null ? '' : ` / ${percent}%`}`;
  }
  const bytes = new Uint8Array(received);
  let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
  return bytes;
}

try {
  const probe = document.createElement('canvas');
  const gl = probe.getContext('webgl2');
  if (!gl) throw new Error('WebGL2 不可用');
  gl.getExtension('WEBGL_lose_context')?.loseContext();
  const [{ default: init }, bytes] = await Promise.all([
    import(`./${game}/game.js`), download(new URL(`./${game}/game_bg.wasm`, import.meta.url)),
  ]);
  detail.textContent = '正在启动游戏…';
  progress.removeAttribute('value');
  try { await init({ module_or_path: bytes }); }
  catch (error) {
    if (!String(error).includes('Using exceptions for control flow')) throw error;
  }
  // Startup prepares the renderer asynchronously. Keep the canvas unobstructed.
  loading.hidden = true;
  document.body.dataset.status = 'running';
  canvas.focus();
} catch (error) { fail(error); }
