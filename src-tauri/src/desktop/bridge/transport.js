(function installDesktopTransport() {
  'use strict';
  const profile = window.__NEKO_DESKTOP_PROFILE__;
  const backendPage = location.protocol === 'http:' &&
    ['127.0.0.1', 'localhost'].includes(location.hostname) && location.port === '48911';
  const ballPage = profile?.role === 'chat-ball' && location.pathname.endsWith('/ball.html');
  if (window.__NEKO_DESKTOP__ || (!backendPage && !ballPage)) return;
  const tauri = window.__TAURI__;
  if (!profile || !tauri || !tauri.core || !tauri.event) return;
  const subscribers = new Map();
  const eventOptions = { target: profile.label };
  let documentId = 0;
  let disposed = false;
  let unlisten = null;
  let pending = [];
  let sendQueue = Promise.resolve();
  let ready;
  let seen = new Set();
  let lifecycleGeneration = 0;

  function receive(message) {
    if (disposed) return;
    if (!documentId) { pending.push(message); return; }
    if (message.sequence && seen.has(message.sequence)) return;
    if (message.sequence) {
      seen.add(message.sequence);
      if (seen.size > 1024) seen.delete(seen.values().next().value);
    }
    for (const listener of subscribers.get(message.kind) || []) {
      try { listener(message); } catch (error) { console.error('[Desktop] Message consumer failed:', error); }
    }
  }

  function start() {
    const generation = ++lifecycleGeneration;
    const activatedAt = performance.timeOrigin + performance.now();
    disposed = false;
    documentId = 0;
    seen = new Set();
    const stops = [];
    function listen(name, handler) {
      return tauri.event.listen(name, handler, eventOptions).then(stop => {
        if (disposed || generation !== lifecycleGeneration) stop();
        else stops.push(stop);
        return stop;
      });
    }
    ready = Promise.all([
      listen('neko-desktop-message', event => { if (generation === lifecycleGeneration) receive(event.payload); }),
      ...Object.entries({
        'neko-desktop-visibility': 'surface-visibility',
        'neko-desktop-bounds-changed': 'bounds-changed',
        'neko-desktop-child-closed': 'child-closed',
        'neko-desktop-parent-changed': 'parent-changed',
        'neko-desktop-post-message': 'post-message',
        'neko-desktop-chat-restored': 'chat-restored',
        'neko-desktop-restore-ack': 'restore-ack',
        'neko_radar_tick': 'radar-point'
      }).map(([event, kind]) => listen(event, event => {
        if (generation === lifecycleGeneration) receive({ kind, data: event.payload });
      }))
    ]).then(() => {
      const stop = () => { for (const stop of stops) stop(); };
      if (disposed || generation !== lifecycleGeneration) { stop(); throw new Error('Desktop document was disposed'); }
      unlisten = stop;
      return tauri.core.invoke('desktop_ready', { startedAt: performance.timeOrigin, activatedAt });
    }).then(result => {
      if (disposed || generation !== lifecycleGeneration) throw new Error('Desktop document was disposed');
      documentId = result.document;
      pending = result.replay.concat(pending);
      for (const listener of subscribers.get('document-ready') || []) listener({ data: result });
      queueMicrotask(() => {
        const replay = pending;
        pending = [];
        for (const message of replay) receive(message);
      });
      return result;
    }).catch(error => {
      for (const stop of stops) stop();
      throw error;
    });
    ready.catch(error => { if (!disposed) console.error('[Desktop] Bridge initialization failed:', error); });
  }

  function publish(kind, data, generation = 0, epoch = 0) {
    // Serialize publishes from the same document: OPEN must reach the host
    // before the first frame, even if the underlying IPC completes out of order.
    const issuedGeneration = lifecycleGeneration;
    const operation = sendQueue.then(() => ready).then(() => {
      if (disposed || issuedGeneration !== lifecycleGeneration) throw new Error('Desktop document was disposed');
      return tauri.core.invoke('desktop_publish', {
        message: { kind, data, generation, epoch, document: documentId }
      });
    });
    sendQueue = operation.catch(() => {});
    return operation;
  }

  function request(action, options = {}) {
    const issuedGeneration = lifecycleGeneration;
    return ready.then(() => {
      if (disposed || issuedGeneration !== lifecycleGeneration) throw new Error('Desktop document was disposed');
      return tauri.core.invoke('desktop_window', { request: { action, ...options }, document: documentId });
    });
  }

  function on(kind, handler) {
    if (!subscribers.has(kind)) subscribers.set(kind, new Set());
    subscribers.get(kind).add(handler);
    return () => subscribers.get(kind)?.delete(handler);
  }

  function dispose(event) {
    disposed = true;
    lifecycleGeneration += 1;
    if (unlisten) unlisten();
    pending = [];
    if (!event.persisted) subscribers.clear();
  }

  window.__NEKO_DESKTOP__ = {
    profile, get ready() { return ready; }, publish, request, on,
    fire(operation) { Promise.resolve(operation).catch(error => { if (!disposed) console.error('[Desktop] Operation failed:', error); }); }
  };
  window.addEventListener('pagehide', dispose);
  window.addEventListener('pageshow', event => { if (event.persisted && disposed) start(); });
  start();
})();
