(function installSharedSettings() {
  'use strict';
  const host = window.__NEKO_DESKTOP__;
  if (!host || host.profile.role === 'chat-ball') return;
  const keys = [
    'proactiveChatEnabled', 'proactiveVisionEnabled', 'proactiveVisionChatEnabled',
    'proactiveNewsChatEnabled', 'proactiveVideoChatEnabled', 'proactivePersonalChatEnabled',
    'proactiveMusicEnabled', 'proactiveMemeEnabled', 'proactiveMiniGameInviteEnabled',
    'proactiveChatInterval', 'proactiveVisionInterval', 'mergeMessagesEnabled',
    'focusModeEnabled', 'textGuardMaxLength', 'avatarReactionBubbleEnabled',
    'subtitleEnabled', 'userLanguage', 'speakerVolume'
  ];
  const pet = host.profile.role === 'pet';
  let applying = false;
  let queued = false;
  let pending = null;
  let hookTimer = null;
  const hooked = new WeakSet();

  function theme(dark) {
    document.documentElement.classList.toggle('dark', dark);
    if (dark) document.documentElement.setAttribute('data-theme', 'dark');
    else document.documentElement.removeAttribute('data-theme');
    localStorage.setItem('neko-dark-mode', String(dark));
    window.dispatchEvent(new CustomEvent('neko-theme-changed', { detail: { darkMode: dark } }));
  }
  function collect() {
    const state = window.appState;
    const snapshot = { darkMode: localStorage.getItem('neko-dark-mode') === 'true' };
    for (const key of keys) {
      const value = state && key in state ? state[key] : window[key];
      if (['boolean', 'string', 'number'].includes(typeof value)) snapshot[key] = value;
    }
    return snapshot;
  }
  function broadcast() {
    if (applying || queued) return;
    queued = true;
    queueMicrotask(() => {
      queued = false;
      host.fire(host.publish(pet ? 'settings-state' : 'settings-change', collect()));
    });
  }
  function apply(snapshot) {
    if (!snapshot) return;
    applying = true;
    try {
      if (typeof snapshot.darkMode === 'boolean') theme(snapshot.darkMode);
      const state = window.appState;
      if (!state) { pending = snapshot; return; }
      const changed = [];
      for (const key of keys) {
        if (!Object.prototype.hasOwnProperty.call(snapshot, key)) continue;
        if (state[key] !== snapshot[key]) { state[key] = snapshot[key]; changed.push(key); }
      }
      for (const key of ['subtitleEnabled', 'userLanguage', 'speakerVolume', 'textGuardMaxLength']) {
        if (key in snapshot) window[key] = snapshot[key];
      }
      if ('speakerVolume' in snapshot && state.speakerGainNode) {
        state.speakerGainNode.gain.setTargetAtTime(snapshot.speakerVolume / 100,
          state.speakerGainNode.context.currentTime, 0.05);
      }
      if ('subtitleEnabled' in snapshot) window.subtitleBridge?.setSubtitleEnabled?.(snapshot.subtitleEnabled);
      if ('userLanguage' in snapshot) window.subtitleBridge?.setUserLanguage?.(snapshot.userLanguage);
      if (pet && changed.some(key => key.startsWith('proactive'))) {
        if (state.proactiveChatEnabled) window.appProactive?.scheduleProactiveChat?.();
        else window.appProactive?.stopProactiveChatSchedule?.();
      }
      pending = null;
    } finally { applying = false; }
  }
  host.on('settings-state', message => apply(message.data));
  if (pet) {
    host.on('settings-request', () => host.fire(host.publish('settings-state', collect())));
    host.on('settings-change', message => { apply(message.data); host.fire(host.publish('settings-state', collect())); });
  }
  window.nekoDarkMode = {
    get: () => Promise.resolve(localStorage.getItem('neko-dark-mode') === 'true'),
    set: dark => { theme(!!dark); broadcast(); return Promise.resolve(!!dark); },
    toggle: () => window.nekoDarkMode.set(localStorage.getItem('neko-dark-mode') !== 'true')
  };
  function hook(object, key) {
    const original = object?.[key];
    if (typeof original !== 'function' || hooked.has(original)) return;
    function save(...args) {
      const result = original.apply(this, args);
      Promise.resolve(result).then(broadcast, () => {});
      return result;
    }
    hooked.add(save);
    object[key] = save;
  }
  function synchronize() {
    hook(window, 'saveNEKOSettings');
    hook(window.appSettings, 'saveSettings');
    if (pending && window.appState) apply(pending);
  }
  function start() {
    theme(localStorage.getItem('neko-dark-mode') === 'true');
    synchronize();
    if (hookTimer === null) hookTimer = setInterval(synchronize, 1000);
    if (pet) broadcast(); else host.fire(host.publish('settings-request', {}));
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start, { once: true });
  else start();
  window.addEventListener('pagehide', () => { clearInterval(hookTimer); hookTimer = null; });
  window.addEventListener('pageshow', event => { if (event.persisted) start(); });
})();
