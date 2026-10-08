const game = document.body.dataset.game;
const { t, setText } = window.arcadeI18n;
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
  setText(scaleToggle, originalSize ? 'fitWindow' : 'originalSize');
  scaleToggle.setAttribute('aria-pressed', String(originalSize));
  fit();
});
document.querySelector('#fullscreen').addEventListener('click', async () => {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else await document.querySelector('.game-main').requestFullscreen();
  } catch { setText(detail, 'fullscreenFailed'); }
});
retry.addEventListener('click', () => location.reload());
canvas.addEventListener('pointerdown', () => canvas.focus());

function fail(error) {
  console.error(error);
  document.body.dataset.status = 'error';
  loading.hidden = false;
  setText(document.querySelector('#loading-title'), 'loadingFailed');
  setText(detail, 'loadingFailedDetail', {error: () => error?.translationKey
    ? t(error.translationKey, error.translationValues) : String(error?.message || error)});
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
  if (!response.ok) throw localizedError('resourceFailed', {status: response.status});
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
    setText(detail, 'downloading', {mb: (received / 1048576).toFixed(1), percent: percent === null ? '' : ` / ${percent}%`});
  }
  const bytes = new Uint8Array(received);
  let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
  return bytes;
}

function localizedError(key, values = {}) {
  const error = new Error(t(key, values));
  error.translationKey = key;
  error.translationValues = values;
  return error;
}

try {
  const probe = document.createElement('canvas');
  const gl = probe.getContext('webgl2');
  if (!gl) throw localizedError('webglUnavailable');
  gl.getExtension('WEBGL_lose_context')?.loseContext();
  const [{ default: init }, bytes] = await Promise.all([
    import(`./${game}/game.js`), download(new URL(`./${game}/game_bg.wasm`, import.meta.url)),
  ]);
  setText(detail, 'starting');
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
