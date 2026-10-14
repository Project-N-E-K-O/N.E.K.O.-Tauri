(function installCompactLayout() {
  'use strict';
  const host = window.__NEKO_DESKTOP__;
  if (!host || host.profile.role !== 'compact-chat') return;
  // Shared compact geometry clamps its anchor to a 20px viewport pad. Use the
  // same pad so native fitting cannot repeatedly fight that clamp and drift.
  const margin = 20;
  let bounds = null;
  let workArea = null;
  let surface = null;
  let active = false;
  let applying = false;
  let generation = 0;
  let timer = null;

  function rect(raw) {
    if (!raw) return null;
    const left = Number(raw.left ?? raw.x);
    const top = Number(raw.top ?? raw.y);
    const width = Number(raw.width);
    const height = Number(raw.height);
    return [left, top, width, height].every(Number.isFinite) && width > 0 && height > 0
      ? { left, top, width, height, right: left + width, bottom: top + height } : null;
  }
  function publishLayout() {
    if (!bounds || !surface || !workArea) return;
    window.__nekoDesktopCompactLayout = {
      surface, windowBounds: bounds, workArea,
      surfaceScreenRect: { ...surface, left: bounds.x + surface.left, top: bounds.y + surface.top },
      anchorVersion: ++generation
    };
    window.dispatchEvent(new CustomEvent('neko:desktop-compact-layout-change', {
      detail: window.__nekoDesktopCompactLayout
    }));
  }
  function fitContent() {
    if (applying || !bounds || !active) return;
    const geometry = window.__nekoGetCompactInteractionGeometry?.();
    const base = rect(geometry?.baseSurfaceRect);
    const rectangles = (geometry?.surfaceNativeRects || []).map(rect).filter(Boolean);
    if (!base || !rectangles.length || geometry.mode !== 'compact') return;
    const left = Math.min(...rectangles.map(rect => rect.left));
    const top = Math.min(...rectangles.map(rect => rect.top));
    const right = Math.max(...rectangles.map(rect => rect.right));
    const bottom = Math.max(...rectangles.map(rect => rect.bottom));
    const dx = margin - Math.floor(left);
    const dy = margin - Math.floor(top);
    const next = { x: bounds.x - dx, y: bounds.y - dy,
      width: Math.ceil(right - left) + margin * 2, height: Math.ceil(bottom - top) + margin * 2 };
    if (Math.abs(next.width - bounds.width) < 1 && Math.abs(next.height - bounds.height) < 1 && !dx && !dy) return;
    applying = true;
    const requestGeneration = generation;
    host.request('set-bounds', { bounds: next }).then(applied => {
      if (requestGeneration !== generation) return;
      bounds = applied;
      surface = { ...base, left: base.left + dx, top: base.top + dy };
      publishLayout();
    }).catch(error => console.error('[Desktop] Compact carrier layout failed:', error))
      .finally(() => { applying = false; });
  }
  host.on('surface-visibility', message => { active = !!message.data.visible; if (active) fitContent(); });
  window.addEventListener('neko:compact-interaction-geometry-change', fitContent);
  window.addEventListener('neko:compact-surface-resize-request', event => {
    const requested = rect(event.detail?.screenRect);
    if (!requested || !bounds) return;
    surface = { left: requested.left - bounds.x, top: requested.top - bounds.y,
      width: requested.width, height: requested.height };
    publishLayout();
    requestAnimationFrame(fitContent);
  });
  host.on('bounds-changed', message => { bounds = message.data; if (!applying) publishLayout(); });

  async function start() {
    const state = await host.ready;
    active = state.visible;
    bounds = state.bounds;
    workArea = await host.request('work-area');
    surface = { left: margin, top: margin, width: Math.max(320, bounds.width - margin * 2), height: 58 };
    document.documentElement.dataset.nekoNativeChat = 'compact';
    const style = document.createElement('style');
    style.textContent = 'html[data-neko-native-chat="compact"] body.neko-desktop-runtime.subtitle-web-host #react-chat-window-shell{left:0!important;top:0!important;right:auto!important;bottom:auto!important;width:100%!important;height:0!important;transform:none!important}';
    document.head.appendChild(style);
    publishLayout();
    if (timer === null) timer = setInterval(fitContent, 500);
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', () => host.fire(start()), { once: true });
  else host.fire(start());
  window.addEventListener('pagehide', () => { clearInterval(timer); timer = null; generation += 1; });
  window.addEventListener('pageshow', event => { if (event.persisted) host.fire(start()); });
})();
