(function installSatelliteInput() {
  'use strict';
  const host = window.__NEKO_DESKTOP__;
  if (!host || ['pet', 'child', 'subtitle-settings'].includes(host.profile.role)) return;
  const selector = {
    'compact-chat': '#react-chat-window-shell,.live2d-popup,.vrm-popup,[role="dialog"]',
    'full-chat': '#react-chat-window-shell,[role="dialog"]',
    'subtitle': '#subtitle-display,#subtitle-panel-controls button,[role="dialog"]',
    'agent-hud': '#agent-task-hud,[role="dialog"]',
    'chat-ball': 'button'
  }[host.profile.role];
  let pressed = false;
  let last = '';
  let pending = false;
  let timer = null;
  let stopped = false;

  function visible(node) {
    for (let current = node; current && current.nodeType === 1; current = current.parentElement) {
      const style = getComputedStyle(current);
      if (current.hidden || style.display === 'none' || style.visibility === 'hidden' || Number(style.opacity || 1) <= 0.01) return false;
    }
    return true;
  }
  function regions() {
    if (pressed) return [{ x: 0, y: 0, width: innerWidth, height: innerHeight }];
    const compact = host.profile.role === 'compact-chat' ? window.__nekoGetCompactInteractionGeometry?.() : null;
    if (compact?.mode === 'compact' && compact.surfaceNativeRects?.length) {
      return compact.surfaceNativeRects.map(rect => ({
        x: Number(rect.left ?? rect.x), y: Number(rect.top ?? rect.y),
        width: Number(rect.width), height: Number(rect.height)
      })).filter(rect => Object.values(rect).every(Number.isFinite) && rect.width >= 1 && rect.height >= 1).slice(0, 128);
    }
    return Array.from(document.querySelectorAll(selector)).filter(node => {
      if (host.profile.role === 'subtitle' && node.id === 'subtitle-display' &&
          (window.__nekoSubtitleLatestState?.locked || window.__nekoSubtitleLatestState?.interactionPassthrough)) return false;
      return visible(node);
    }).map(node => {
      const rect = node.getBoundingClientRect();
      return { x: Math.max(0, rect.x), y: Math.max(0, rect.y),
        width: Math.min(innerWidth, rect.right) - Math.max(0, rect.x),
        height: Math.min(innerHeight, rect.bottom) - Math.max(0, rect.y) };
    }).filter(rect => rect.width >= 1 && rect.height >= 1).slice(0, 128);
  }
  function apply(ignore, rectangles) {
    if (stopped || pending) return;
    const details = JSON.stringify({ ignore, rectangles, dpr: devicePixelRatio || 1 });
    if (details === last) return;
    pending = true;
    host.request('input-region', JSON.parse(details)).then(() => { last = details; })
      .catch(error => { if (!stopped) console.error('[Desktop] Input region failed:', error); })
      .finally(() => { pending = false; });
  }
  function refresh() {
    if (host.profile.platform === 'linux') apply(true, regions());
  }
  host.on('radar-point', event => {
    if (host.profile.platform === 'linux') return;
    const [x, y] = event.data || [];
    if (!Number.isFinite(x) || !Number.isFinite(y)) return;
    const hit = pressed || regions().some(rect => x >= rect.x && y >= rect.y &&
      x <= rect.x + rect.width && y <= rect.y + rect.height);
    apply(!hit, []);
  });
  window.addEventListener('pointerdown', () => { pressed = true; refresh(); }, true);
  for (const event of ['pointerup', 'pointercancel', 'blur']) {
    window.addEventListener(event, () => { pressed = false; refresh(); }, true);
  }
  window.addEventListener('resize', refresh);
  function start() {
    stopped = false;
    last = '';
    if (host.profile.platform === 'linux') timer = setInterval(refresh, 250);
    refresh();
  }
  window.addEventListener('pagehide', () => { stopped = true; clearInterval(timer); timer = null; });
  window.addEventListener('pageshow', event => { if (event.persisted) start(); });
  host.ready.then(start);
})();
