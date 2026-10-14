(function installMinimizedChatBall() {
  'use strict';
  const host = window.__NEKO_DESKTOP__;
  if (!host || host.profile.role !== 'chat-ball') return;
  let acknowledgeRestore = null;
  let restoring = false;
  let visible = false;
  let restoreTimer = null;
  host.on('restore-ack', () => { if (acknowledgeRestore) acknowledgeRestore(); });
  function image(url) {
    return new Promise((resolve, reject) => {
      const image = new Image();
      image.onload = resolve;
      image.onerror = () => reject(new Error('Yarn ball asset could not load'));
      image.src = url;
    });
  }
  async function initialize() {
    const state = await host.ready;
    const button = document.querySelector('button');
    const assets = ['bounce', 'appear'].map(kind => [1, 2].map(scale =>
      new URL(`/static/assets/neko-idle/chat-minimized-yarn-ball-window-${kind}-${scale}x.png`, state.backendUrl).href));
    for (const [kind, urls] of [['bounce', assets[0]], ['appear', assets[1]]]) {
      document.documentElement.style.setProperty('--ball-' + kind, `image-set(url("${urls[0]}") 1x,url("${urls[1]}") 2x)`);
    }
    await Promise.all(assets.map(urls => image(urls[devicePixelRatio > 1 ? 1 : 0])));
    button.classList.add('ready');
    host.on('surface-visibility', message => {
      const next = !!message.data.visible;
      if (next && !visible) {
        button.classList.remove('bouncing');
        button.classList.add('appearing');
      }
      visible = next;
    });
    button.addEventListener('animationend', event => { if (event.animationName === 'appear') button.classList.remove('appearing'); });
    let origin = null;
    let dragging = false;
    let longPress = null;
    button.addEventListener('pointerdown', event => {
      if (event.button !== 0 || restoring) return;
      origin = { x: event.screenX, y: event.screenY };
      dragging = false;
      if (host.profile.platform === 'linux') longPress = setTimeout(() => {
        dragging = true;
        host.fire(host.request('drag'));
      }, 260);
    });
    button.addEventListener('pointermove', event => {
      if (!origin || dragging || host.profile.platform === 'linux') return;
      if (Math.hypot(event.screenX - origin.x, event.screenY - origin.y) >= 4) {
        dragging = true;
        host.fire(host.request('drag'));
      }
    });
    button.addEventListener('pointerup', () => {
      clearTimeout(longPress);
      if (!origin) return;
      origin = null;
      if (!dragging) button.click();
    });
    button.addEventListener('click', async event => {
      if (restoring || dragging || event.detail > 0) return;
      restoring = true;
      button.classList.remove('appearing');
      button.classList.add('bouncing');
      const restored = new Promise(resolve => { acknowledgeRestore = resolve; });
      try {
        await host.request('expand', { keepBall: true });
        const timeout = new Promise((_, reject) => {
          restoreTimer = setTimeout(() => reject(new Error('Chat surface did not confirm restoration')), 15000);
        });
        await Promise.all([Promise.race([restored, timeout]), new Promise(resolve => setTimeout(resolve, 760))]);
        await host.request('hide', { role: null });
      } catch (error) {
        console.error('[Desktop] Chat restore failed:', error);
        button.classList.remove('bouncing');
      } finally {
        acknowledgeRestore = null;
        clearTimeout(restoreTimer);
        restoring = false;
      }
    });
    for (const event of ['pointercancel', 'blur', 'pagehide']) window.addEventListener(event, () => {
      clearTimeout(longPress);
      origin = null;
      dragging = false;
    });
    // Hidden WebViews can suspend animation frames. Loaded sprite assets and
    // the ready class are sufficient to reveal; animate after native show.
    await host.request('painted');
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', () => host.fire(initialize()), { once: true });
  else host.fire(initialize());
})();
