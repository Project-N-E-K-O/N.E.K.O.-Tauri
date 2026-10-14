(function installNativeGestures() {
  'use strict';
  const host = window.__NEKO_DESKTOP__;
  if (!host || !['compact-chat', 'full-chat', 'agent-hud'].includes(host.profile.role)) return;
  let press = null;
  const controls = 'button,input,textarea,select,a,[role="button"],[contenteditable="true"],[data-compact-no-drag="true"]';
  const surfaces = '[data-compact-drag-surface="true"],.window-topbar,#agent-task-hud-header';
  if (host.profile.role !== 'agent-hud') document.addEventListener('contextmenu', event => {
    if (!event.target.closest(surfaces) || event.target.closest(controls)) return;
    event.preventDefault();
    host.fire((async () => {
      if (host.prepareChatSnapshot) await host.prepareChatSnapshot();
      return host.request('window-menu');
    })());
  });
  document.addEventListener('pointerdown', event => {
    if (event.button !== 0 || !event.target.closest(surfaces) || event.target.closest(controls)) return;
    press = { pointer: event.pointerId, x: event.screenX, y: event.screenY };
  }, true);
  document.addEventListener('pointermove', event => {
    if (!press || event.pointerId !== press.pointer || !event.buttons) return;
    if (Math.hypot(event.screenX - press.x, event.screenY - press.y) < 4) return;
    press = null;
    event.preventDefault();
    event.stopPropagation();
    host.fire(host.request('drag'));
  }, true);
  for (const event of ['pointerup', 'pointercancel', 'blur', 'pagehide']) {
    window.addEventListener(event, () => { press = null; }, true);
  }
})();
