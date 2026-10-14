(function installPetSurface() {
  'use strict';
  const host = window.__NEKO_DESKTOP__;
  if (!host || host.profile.role !== 'pet') return;
  const { request, publish, fire, on } = host;
  let surface = 'compact';
  host.ready.then(state => { surface = state.surface; });

  // The shared controller remains the owner of chat data; its presentation
  // methods cross the native host seam instead of revealing a duplicate DOM UI.
  function nativePresentation(object) {
    const methods = {
      openWindow: () => request('expand'),
      closeWindow: () => request('hide', { role: surface === 'full' ? 'full-chat' : 'compact-chat' }),
      ensureChatSurfaceVisible: () => request('expand'),
      getChatSurfaceMode: () => surface,
      setChatSurfaceMode(mode) {
        if (mode === 'minimized') return request('collapse', { bounds: null });
        if (mode !== 'full' && mode !== 'compact') return surface;
        surface = mode;
        fire(request('select-surface', { surface }));
        return surface;
      },
      cycleChatSurfaceMode() {
        return this.setChatSurfaceMode(surface === 'compact' ? 'full' : 'compact');
      }
    };
    for (const [name, method] of Object.entries(methods)) {
      Object.defineProperty(object, name, { configurable: true, enumerable: true,
        get: () => method, set: () => {} });
    }
    return object;
  }
  let chatHost = nativePresentation(window.reactChatWindowHost || {});
  Object.defineProperty(window, 'reactChatWindowHost', { configurable: true,
    get: () => chatHost, set: value => { chatHost = nativePresentation(value || {}); } });

  window.nekoSubtitleWindow = {
    show: () => request('show', { role: 'subtitle' }),
    hide: () => request('hide', { role: 'subtitle' }),
  };
  window.nekoAgentHud = {
    show: () => request('show', { role: 'agent-hud' }),
    hide: () => request('hide', { role: 'agent-hud' }),
  };

  async function config() {
    if (window.pageConfigReady) await window.pageConfigReady;
    const cfg = window.lanlan_config || {};
    return { ...cfg, master_name: window.master_name || cfg.master_name || '',
      master_profile_name: window.master_profile_name || cfg.master_profile_name || '',
      master_nickname: window.master_nickname || cfg.master_nickname || '',
      master_display_name: window.master_display_name || cfg.master_display_name || '' };
  }
  on('config-request', () => fire(config().then(data => publish('config-result', data))));

  function portrait(kind, options) {
    if (!window.avatarPortrait?.capture) return publish(kind, { error: 'Avatar capture is unavailable' });
    return window.avatarPortrait.capture({ ...options, includeDataUrl: true }).then(result => publish(kind, {
      dataUrl: result?.dataUrl || null,
      modelType: result?.modelType || window.lanlan_config?.model_type || 'live2d'
    })).catch(error => publish(kind, { error: error.message }));
  }
  on('avatar-request', () => fire(portrait('avatar-result', {
    width: 320, height: 320, padding: 0.035, shape: 'rounded', radius: 40,
    background: 'rgba(255,255,255,0.96)'
  })));
  on('character-reference-request', () => fire(portrait('character-reference-result', {
    width: 768, height: 1024, padding: 0.08, shape: 'square', cropMode: 'portrait', background: 'transparent'
  })));
  on('screenshot-request', () => fire((async () => {
    try {
      const result = typeof window.captureScreenshotDataUrl === 'function'
        ? await window.captureScreenshotDataUrl()
        : { dataUrl: await window.captureProactiveChatScreenshot?.() };
      return publish('screenshot-result', result?.dataUrl ? result : { dataUrl: null, canceled: true });
    } catch (error) {
      return publish('screenshot-result', { dataUrl: null, error: error.message });
    }
  })()));
  on('agent-toggle', message => fire(fetch('/api/agent/toggle_flag', {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ flag: message.data.capability, enabled: !!message.data.enabled })
  }).then(response => { if (!response.ok) throw new Error('Agent capability update failed'); })));

  function subtitleState(detail) {
    const state = detail?.state || detail;
    if (!state) return;
    const enabled = !!state.subtitleEnabled;
    const visible = enabled && !!state.visible;
    fire(request(visible ? 'show' : 'hide', { role: 'subtitle' }));
    fire(publish('transcript', { transcript: state.text || '', is_final: true, translated: true }));
    fire(publish('subtitle-state', {
      enabled, visible, language: state.userLanguage || localStorage.getItem('userLanguage') || 'en',
      locale: state.uiLocale || window.i18next?.language || document.documentElement.lang || 'zh-CN',
      opacity: state.subtitleOpacity ?? 95,
      bounds: state.subtitlePanelBounds || { width: 600, height: 68 },
      locked: !!state.subtitlePanelLocked || !!state.subtitleInteractionPassthrough,
      interactionPassthrough: !!state.subtitlePanelLocked || !!state.subtitleInteractionPassthrough,
      danmakuMode: !!state.subtitleDanmakuMode,
      fontSize: state.subtitleFontSize || 26, colorScheme: state.subtitleColorScheme || 'default'
    }));
  }
  window.__forwardSubtitleRenderStateToWindow = subtitleState;
  window.addEventListener('neko-subtitle-render-state', event => subtitleState(event.detail));
  on('subtitle-change', message => {
    const change = message.data;
    const keys = { toggle: 'subtitleEnabled', opacity: 'subtitleOpacity', bounds: 'subtitlePanelBounds',
      lock: 'subtitlePanelLocked', interactionPassthrough: 'subtitleInteractionPassthrough',
      danmakuMode: 'subtitleDanmakuMode', fontSize: 'subtitleFontSize', colorScheme: 'subtitleColorScheme' };
    if (change.type === 'language') window.subtitleBridge?.setUserLanguage?.(change.value);
    else if (keys[change.type]) {
      const patch = { [keys[change.type]]: change.value };
      if (change.type === 'lock' || change.type === 'interactionPassthrough') {
        patch.subtitlePanelLocked = patch.subtitleInteractionPassthrough = !!change.value;
      }
      if (change.type === 'toggle' && window.subtitleBridge?.setSubtitleEnabled) {
        window.subtitleBridge.setSubtitleEnabled(!!change.value, { source: 'subtitle-window-toggle' });
      } else window.nekoSubtitleShared?.updateSettings?.(patch, { source: 'subtitle-window', persist: !change.transient });
    }
    window.appSettings?.saveSettings?.();
    subtitleState(window.nekoSubtitleShared?.getRenderState?.());
  });

  function initializeDocument() {
    const style = document.createElement('style');
    style.textContent = '#react-chat-window-overlay,#chat-container,#subtitle-display,#agent-task-hud{visibility:hidden!important;pointer-events:none!important}';
    document.head.appendChild(style);
    subtitleState(window.nekoSubtitleShared?.getRenderState?.());
    let hudVisible = false;
    const observer = new MutationObserver(() => {
      const hud = document.getElementById('agent-task-hud');
      const visible = !!hud && hud.style.display !== 'none' && hud.style.opacity !== '0';
      if (visible !== hudVisible) {
        hudVisible = visible;
        fire(request(visible ? 'show' : 'hide', { role: 'agent-hud' }));
      }
    });
    observer.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['style', 'class'] });
    window.addEventListener('pagehide', () => observer.disconnect());
    window.addEventListener('pageshow', event => {
      if (event.persisted) {
        observer.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['style', 'class'] });
        subtitleState(window.nekoSubtitleShared?.getRenderState?.());
      }
    });
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', initializeDocument, { once: true });
  else initializeDocument();
})();
