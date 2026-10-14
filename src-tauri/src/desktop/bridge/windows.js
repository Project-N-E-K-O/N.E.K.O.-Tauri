(function installNativeWindowInterfaces() {
  'use strict';
  const host = window.__NEKO_DESKTOP__;
  if (!host) return;
  const { request, fire } = host;
  const role = host.profile.role;
  const chat = role === 'compact-chat' || role === 'full-chat';
  const named = new Map();
  const labels = new Map();

  class ChildWindow {
    constructor(url, name, options = {}) {
      this.closed = false;
      this.url = url;
      this.label = null;
      this.ready = request('open-child', { url, name: name || null, ...options }).then(result => {
        if (!result) { this.closed = true; return null; }
        this.label = result.label;
        labels.set(this.label, this);
        if (this.closed) return request('close-child', { label: this.label }).then(() => null);
        return this.label;
      }).catch(error => { this.closed = true; throw error; });
      fire(this.ready);
      const handle = this;
      this.location = {
        get href() { return handle.url; },
        set href(url) {
          handle.url = new URL(url, location.href).href;
          fire(request('open-child', { url: handle.url, name: name || null, navigate: true }));
        }
      };
    }
    focus() { fire(this.ready.then(label => label && request('focus-child', { label }))); }
    close() {
      if (this.closed) return;
      this.closed = true;
      fire(this.ready.then(label => label && request('close-child', { label })));
    }
    postMessage(data, origin) {
      if (origin !== '*' && origin !== location.origin) return;
      fire(this.ready.then(label => label && request('post-message', { target: label, data })));
    }
  }

  function open(url = 'about:blank', name = '_blank', features = '', options = {}) {
    const absolute = new URL(url, location.href);
    if (absolute.origin !== location.origin) {
      fire(request('open-child', { url: absolute.href, name: null }));
      return null;
    }
    if (name && name !== '_blank') {
      const existing = named.get(name);
      if (existing && !existing.closed) {
        if (options.navigateOnReuse) existing.location.href = absolute.href;
        existing.focus(); return existing;
      }
    }
    const geometry = {};
    for (const [feature, property] of [['width','width'],['height','height'],['left','x'],['top','y']]) {
      const match = String(features).match(new RegExp('(?:^|,)\\s*'+feature+'\\s*=\\s*(-?[0-9]+(?:\\.[0-9]+)?)','i'));
      if (match) geometry[property] = Number(match[1]);
    }
    const child = new ChildWindow(absolute.href, name, geometry);
    if (name && name !== '_blank') named.set(name, child);
    return child;
  }

  window.open = open;
  window.openOrFocusWindow = open;
  if (role === 'child') window.close = () => fire(request('close'));
  window.electronShell = {
    openExternal: url => request('open-child', { url: new URL(url, location.href).href, name: null })
  };
  if (host.profile.parent && role === 'child') {
    Object.defineProperty(window, 'opener', { configurable: true, value: {
      closed: false,
      focus: () => fire(request('focus-parent')),
      postMessage(data, origin) {
        if (origin !== '*' && origin !== location.origin) return;
        fire(request('post-message', { target: host.profile.parent, data }));
      }
    } });
  }
  host.on('parent-changed', message => { host.profile.parent = message.data; });
  host.on('document-ready', message => { if (role === 'child') host.profile.parent = message.data.parent; });
  host.on('child-closed', event => {
    const child = labels.get(event.data);
    if (child) child.closed = true;
    labels.delete(event.data);
  });
  host.on('post-message', event => {
    const message = event.data;
    const source = message.source === host.profile.parent ? window.opener : labels.get(message.source);
    const forwarded = new MessageEvent('message', { data: message.data, origin: location.origin });
    Object.defineProperty(forwarded, 'source', { value: source || null });
    window.dispatchEvent(forwarded);
  });

  const windowControl = {
    getBounds: () => request('bounds'),
    getWorkArea: () => request('work-area'),
    setBounds: (x, y, width, height, options = {}) => request('set-bounds', {
      bounds: { x, y, width, height }, transient: options.transient === true
    }),
    setPosition: (x, y) => request('bounds').then(bounds => request('set-bounds', { bounds: { ...bounds, x, y } })),
    setSize: (width, height) => request('bounds').then(bounds => request('set-bounds', { bounds: { ...bounds, width, height } })),
    setResizable: resizable => request('set-resizable', { resizable: !!resizable }),
    bringToFront: () => request('focus'),
    hide: () => request('hide', { role: null }),
    show: () => request('show', { role: null }),
    showConfirmed: () => request('show', { role: null }),
    dragStart: () => request('drag'),
    dragMove: () => request('bounds'),
    dragStop: () => request('bounds'),
    dragStopAndGetBounds: () => request('bounds'),
    resizeStart: direction => request('resize', { direction: direction || 'se' }),
    resizeStop: () => request('bounds'),
    close: () => request('close'),
    minimize: () => request('native-minimize'),
    maximize: () => request('toggle-maximize'),
    isMaximized: () => request('is-maximized'),
    getPinState: () => request('pin-state'),
    setPinned: pinned => request('set-pinned', { pinned: !!pinned }),
    togglePin: () => request('pin-state').then(state => request('set-pinned', { pinned: !state.pinned })),
    enableInteraction: () => window.__TAURI__.core.invoke('set_click_through', { ignore: false }),
    disableInteraction: () => window.__TAURI__.core.invoke('set_click_through', { ignore: true }),
  };
  window.nekoWindowControl = windowControl;
  if (chat) window.nekoChatWindow = {
    ...windowControl,
    collapse: bounds => request('collapse', { bounds: bounds || null }),
    expand: () => request('expand'),
    requestAutoCollapseAfterEnter: () => request('collapse', { bounds: null }),
  };
  if (role === 'subtitle') window.nekoSubtitle = {
    ...windowControl,
    getCursorPoint: () => request('cursor'),
    setBounds: (x, y, width, height, options = {}) => options.restore
      ? request('restore-bounds') : windowControl.setBounds(x, y, width, height, options),
    resizeMove: () => request('bounds'),
    changeSettings: data => fire(host.publish('subtitle-change', data)),
    openSettings: () => request('subtitle-settings', { visible: true }),
    closeSettings: () => request('subtitle-settings', { visible: false }),
    updateSettingsWindow: state => host.publish('subtitle-settings-state', state),
  };
  if (role === 'subtitle-settings') window.nekoSubtitle = {
    changeSettings: data => fire(host.publish('subtitle-change', data))
  };
  if (role === 'agent-hud') window.nekoAgent = {
    toggle: (capability, enabled) => fire(host.publish('agent-toggle', { capability, enabled: !!enabled }))
  };

  window.__NEKO_MULTI_WINDOW__ = role === 'pet' || chat;
  window.__LANLAN_IS_ELECTRON_PET__ = role === 'pet';
  // These class names are the existing shared page's native-window layout
  // contract. They do not claim Electron-only host capabilities.
  function markDocument() {
    document.body?.classList.add('neko-desktop-runtime', 'neko-electron-runtime');
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', markDocument, { once: true });
  else markDocument();
  window.nekoDesktopWindows = {
    selectChatSurface: async surface => {
      if (host.prepareChatSnapshot) await host.prepareChatSnapshot();
      return request('select-surface', { surface });
    },
    restoreChat: () => request('expand'),
  };
})();
