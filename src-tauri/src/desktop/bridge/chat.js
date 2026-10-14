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
  let heldFrames = [];
  let contentReady = false;
  let replayReady = false;
  let pendingSnapshotSequence = 0;
  let restoredSnapshotSequence = 0;
  let restoreGeneration = 0;
  let changingPresentation = false;

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
    const responseState = host.responseTransfer?.capture();
    const state = window.reactChatWindowHost?.getState?.();
    const attachments = host.composerTransfer?.capture();
    if (!state?.mounted || restoring || pendingSnapshot || !attachments || !responseState) return null;
    return { messages: state.messages || [], composerAttachments: attachments,
      responseState,
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
  async function restoreSnapshot() {
    const page = window.reactChatWindowHost;
    if (restoring || !pendingSnapshot || !page?.getState?.().mounted || !host.composerTransfer?.available() ||
        !host.responseTransfer?.available()) return;
    restoring = true;
    const generation = restoreGeneration;
    const data = pendingSnapshot;
    pendingSnapshot = null;
    const sequence = pendingSnapshotSequence;
    if (typeof data.draft === 'string') draft = data.draft;
    const restoreResponse = (data.throughSequence || 0) >= (host.characterSequence || 0);
    if (restoreResponse) {
      page.setMessages?.(data.messages || []);
      host.characterSequence = data.throughSequence || 0;
    }
    host.composerTransfer.restore(data.composerAttachments || []);
    page.setComposerHidden?.(!!data.composerHidden);
    try {
      await host.responseTransfer.restore(data.responseState, data.draft, restoreResponse);
    } catch (error) {
      if (generation === restoreGeneration) {
        restoring = false;
        if (!pendingSnapshot) { pendingSnapshot = data; pendingSnapshotSequence = sequence; }
      }
      throw error;
    }
    if (generation !== restoreGeneration || stopped) return;
    restoring = false;
    restoredSnapshotSequence = sequence;
    if (pendingSnapshot) { fire(restoreSnapshot()); return; }
    const frames = heldFrames;
    heldFrames = [];
    for (const [message, deliver] of frames) {
      if (message.sequence > (data.throughSequence || 0)) deliver();
    }
    resizeToContent();
  }
  host.deferChatFrame = (message, deliver) => {
    if (!pendingSnapshot && !restoring) { deliver(); return; }
    heldFrames.push([message, deliver]);
    if (heldFrames.length > 256) heldFrames.shift();
  };
  on('chat-snapshot', message => {
    contentReady = false;
    pendingSnapshotSequence = message.sequence || 0;
    pendingSnapshot = message.data;
    fire(restoreSnapshot());
  });
  on('surface-visibility', message => { active = !!message.data.visible && !message.data.minimized; });
  on('document-ready', message => {
    active = !!message.data.visible;
    contentReady = false;
    replayReady = false;
    pendingSnapshot = null;
    pendingSnapshotSequence = 0;
    restoredSnapshotSequence = 0;
    restoreGeneration += 1;
    restoring = false;
    // Transport queues its replay after document-ready consumers return.
    queueMicrotask(() => queueMicrotask(() => { replayReady = true; resizeToContent(); }));
  });

  let surfaceTransition = Promise.resolve();
  function changeSurface(mode) {
    if (restoring || changingPresentation || stopped || !active) return;
    const generation = restoreGeneration;
    surfaceTransition = surfaceTransition.catch(() => {}).then(async () => {
      if (generation !== restoreGeneration || stopped) return;
      await shareSnapshot();
      if (generation !== restoreGeneration || stopped) return;
      if (mode === 'minimized') return request('collapse', { bounds: null });
      if (mode !== ownSurface && ['compact', 'full'].includes(mode)) return request('select-surface', { surface: mode });
    });
    fire(surfaceTransition);
  }
  window.addEventListener('react-chat-window:chat-surface-mode-change', event => changeSurface(event.detail?.mode));
  on('chat-restored', () => {
    changingPresentation = true;
    contentReady = false;
    const page = window.reactChatWindowHost;
    page?.setChatSurfaceMode?.(ownSurface);
    page?.openWindow?.();
    changingPresentation = false;
    if (!pendingSnapshot && !restoring) {
      pendingSnapshot = snapshot();
      pendingSnapshotSequence = restoredSnapshotSequence;
    }
    fire(restoreSnapshot());
  });

  window.addEventListener('react-chat-window:composer-rollback', event => {
    if (typeof event.detail?.text === 'string') draft = event.detail.text;
    if (active) fire(shareSnapshot());
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
    } catch (_) {}
    if (active && userInput) queueMicrotask(() => fire(shareSnapshot()));
    return result;
  };

  function resizeToContent() {
    if (stopped) return;
    fire(restoreSnapshot());
    if (replayReady && !restoring && !contentReady && !pendingSnapshot && host.composerTransfer?.available() &&
        host.responseTransfer?.available() &&
        window.reactChatWindowHost?.getState?.().mounted &&
        document.querySelector('#react-chat-window-root .chat-window')) {
      contentReady = true;
      const sequence = restoredSnapshotSequence;
      fire(request('content-ready', { throughSnapshot: sequence })
        .then(result => { if (result?.accepted === false && sequence === restoredSnapshotSequence) contentReady = false; })
        .catch(error => { contentReady = false; throw error; }));
    }
    if (active && !restoring) fire(shareSnapshot());
  }

  function initializeDocument() {
    document.body.dataset.chatHostKind = ownSurface;
    fire(publish('config-request', {}));
    fire(publish('avatar-request', {}));
    window.addEventListener('input', event => {
      if (event.target !== composer()) return;
      draft = event.target.value;
      if (active && !restoring) fire(shareSnapshot());
    }, true);
    observer = new MutationObserver(resizeToContent);
    observer.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['class', 'style'] });
    layoutTimer = setInterval(resizeToContent, 500);
    window.addEventListener('pagehide', () => {
      stopped = true; restoreGeneration += 1; heldFrames = [];
      observer.disconnect(); clearInterval(layoutTimer);
    });
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
