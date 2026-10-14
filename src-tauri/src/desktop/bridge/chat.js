(function installChatSurface() {
  'use strict';
  const host = window.__NEKO_DESKTOP__;
  if (!host || !['compact-chat', 'full-chat'].includes(host.profile.role)) return;
  const { request, publish, fire, on } = host;
  const ownSurface = host.profile.role === 'full-chat' ? 'full' : 'compact';
  let active = false;
  let restoring = false;
  let pendingSnapshot = null;
  let layoutTimer = null;
  let stopped = false;
  let lastSnapshot = '';
  let observer = null;
  let draft = '';
  let draftOverride = null;
  let draftRevision = 0;
  let clearDraftRevision = 0;
  const renderers = new WeakSet();
  let heldFrames = [];

  function event(name, data) { window.dispatchEvent(new CustomEvent(name, { detail: data })); }
  function config(data) {
    window.__nekoInjectedConfig = data || {};
    event('neko:config-injected', data);
  }
  on('config-result', message => config(message.data));
  window.__nekoRequestConfigInjection = () => fire(publish('config-request', {}));
  window.__nekoRequestAvatarPreview = () => fire(publish('avatar-request', {}));
  window.__nekoRequestCharacterReference = () => fire(publish('character-reference-request', {}));
  on('avatar-result', message => {
    if (message.data?.dataUrl) window.appChatAvatar?.setExternalAvatar?.(message.data.dataUrl, message.data.modelType);
    event('neko:avatar-preview-ipc-result', message.data);
  });
  on('character-reference-result', message => event('neko:character-reference-ipc-result', message.data));
  on('screenshot-result', message => event('neko:screenshot-result', message.data));
  window.nekoScreenshotProxy = { request: () => fire(publish('screenshot-request', {})) };
  window.nekoSubtitleWindow = {
    setEnabled: enabled => fire(publish('subtitle-change', { type: 'toggle', value: !!enabled })),
    show: () => request('show', { role: 'subtitle' }),
    hide: () => request('hide', { role: 'subtitle' }),
  };

  function composer() {
    return document.querySelector('#react-chat-window-root textarea, #react-chat-window-root input[type="text"]');
  }
  function snapshot() {
    const state = window.reactChatWindowHost?.getState?.();
    if (!state?.mounted || pendingSnapshot) return null;
    return { messages: state.messages || [], composerAttachments: state.composerAttachments || [],
      composerHidden: state.composerHiddenRequested, draft, surface: ownSurface,
      throughSequence: host.characterSequence || 0 };
  }
  function shareSnapshot() {
    const data = snapshot();
    if (!data) return Promise.resolve();
    const serialized = JSON.stringify(data);
    if (serialized === lastSnapshot) return Promise.resolve();
    return publish('chat-snapshot', data).then(() => { lastSnapshot = serialized; });
  }
  host.prepareChatSnapshot = shareSnapshot;
  function restoreSnapshot() {
    const page = window.reactChatWindowHost;
    if (!pendingSnapshot || !page?.getState?.().mounted) return;
    restoring = true;
    const data = pendingSnapshot;
    pendingSnapshot = null;
    if (typeof data.draft === 'string') {
      if (draft && !data.draft) clearDraftRevision = draftRevision + 1;
      draft = data.draft;
      draftOverride = data.draft;
      draftRevision += 1;
    }
    installDraftAdapter();
    if ((data.throughSequence || 0) >= (host.characterSequence || 0)) page.setMessages?.(data.messages || []);
    page.setComposerAttachments?.(data.composerAttachments || []);
    page.setComposerHidden?.(!!data.composerHidden);
    const input = composer();
    if (input && typeof data.draft === 'string') {
      // Use the browser's value setter so React's value tracker observes the
      // input event exactly as it does for user typing.
      const prototype = input.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(prototype, 'value').set.call(input, data.draft);
      input.dispatchEvent(new Event('input', { bubbles: true }));
    }
    restoring = false;
    const frames = heldFrames;
    heldFrames = [];
    for (const [message, deliver] of frames) {
      if (message.sequence > (data.throughSequence || 0)) deliver();
    }
  }
  host.deferChatFrame = (message, deliver) => {
    if (!pendingSnapshot) { deliver(); return; }
    heldFrames.push([message, deliver]);
    if (heldFrames.length > 256) heldFrames.shift();
  };
  function installDraftAdapter() {
    const renderer = window.NekoChatWindow;
    if (!renderer) return;
    for (const name of ['mount', 'mountChatWindow']) {
      const original = renderer[name];
      if (typeof original !== 'function' || renderers.has(original)) continue;
      function mount(root, props) {
        // rollbackDraft restores text through the shared renderer contract.
        // Its empty-string case intentionally does nothing; reset that inactive
        // React surface once, then mount the complete host props again.
        if (clearDraftRevision && clearDraftRevision === draftRevision) {
          renderer.unmountChatWindow(root);
          clearDraftRevision = 0;
        }
        const next = draftOverride === null ? props : { ...props,
          rollbackDraft: draftOverride, _rollbackKey: 'tauri-draft-' + draftRevision };
        return original.call(this, root, next);
      }
      renderers.add(mount);
      renderer[name] = mount;
    }
  }
  on('chat-snapshot', message => { pendingSnapshot = message.data; restoreSnapshot(); });
  on('surface-visibility', message => { active = !!message.data.visible && !message.data.minimized; });
  on('document-ready', message => { active = !!message.data.visible; });

  let surfaceTransition = Promise.resolve();
  function changeSurface(mode) {
    if (restoring || stopped || !active) return;
    surfaceTransition = surfaceTransition.then(async () => {
      await shareSnapshot();
      if (mode === 'minimized') return request('collapse', { bounds: null });
      if (mode !== ownSurface && ['compact', 'full'].includes(mode)) return request('select-surface', { surface: mode });
    });
    fire(surfaceTransition);
  }
  window.addEventListener('react-chat-window:chat-surface-mode-change', event => changeSurface(event.detail?.mode));
  on('chat-restored', () => {
    restoring = true;
    const page = window.reactChatWindowHost;
    page?.setChatSurfaceMode?.(ownSurface);
    page?.openWindow?.();
    restoring = false;
  });

  const send = window.WebSocket.prototype.send;
  window.WebSocket.prototype.send = function(data) {
    const result = send.call(this, data);
    // The shared page adds the local user message in the same event task.
    // Preserve it for a lazily created sibling before the next surface switch.
    let userInput = false;
    try {
      const frame = JSON.parse(data);
      userInput = frame?.action === 'stream_data' && frame.input_type === 'text';
      if (active && userInput) draft = '';
      if (active && userInput) draftOverride = null;
    } catch (_) {}
    if (active && userInput) queueMicrotask(() => fire(shareSnapshot()));
    return result;
  };

  function resizeToContent() {
    if (stopped) return;
    restoreSnapshot();
    installDraftAdapter();
    if (active && !restoring) fire(shareSnapshot());
  }

  function initializeDocument() {
    document.body.dataset.chatHostKind = ownSurface;
    fire(publish('config-request', {}));
    fire(publish('avatar-request', {}));
    window.addEventListener('input', event => {
      if (event.target !== composer()) return;
      draft = event.target.value;
      if (!restoring) draftOverride = null;
      if (active && !restoring) fire(shareSnapshot());
    }, true);
    observer = new MutationObserver(resizeToContent);
    observer.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['class', 'style'] });
    layoutTimer = setInterval(resizeToContent, 500);
    window.addEventListener('pagehide', () => { stopped = true; observer.disconnect(); clearInterval(layoutTimer); });
    resizeToContent();
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', initializeDocument, { once: true });
  else initializeDocument();
  window.addEventListener('pageshow', event => {
    if (!event.persisted || !observer) return;
    stopped = false;
    observer.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['class', 'style'] });
    clearInterval(layoutTimer);
    layoutTimer = setInterval(resizeToContent, 500);
    resizeToContent();
  });
})();
