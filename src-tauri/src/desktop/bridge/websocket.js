(function installCharacterSocketBridge() {
  'use strict';
  const host = window.__NEKO_DESKTOP__;
  if (!host) return;
  const NativeSocket = window.WebSocket;
  const role = host.profile.role;
  const chat = role === 'compact-chat' || role === 'full-chat';
  if (role !== 'pet' && !chat) return;

  function characterSocket(url) {
    try {
      const parsed = new URL(url, location.href);
      return ['ws:', 'wss:'].includes(parsed.protocol) && parsed.host === location.host &&
        /^\/ws\/[^/]+$/.test(parsed.pathname);
    } catch (_) { return false; }
  }

  const petOnlyFrames = new Set([
    'audio_chunk', 'cozy_audio', 'user_activity', 'capture_bridge_request',
    'capture_bridge_computer_use_request', 'capture_bridge_region_request'
  ]);
  const petOnlyActions = new Set([
    'capture_bridge_status', 'capture_bridge_response',
    'capture_bridge_computer_use_response', 'capture_bridge_region_response'
  ]);

  if (role === 'pet') {
    let active = null;
    let generation = 0;
    let agentTimer = null;
    function publish(kind, data, socketGeneration) {
      host.fire(host.publish(kind, data, socketGeneration));
    }
    function announce() {
      if (!active) return;
      publish('session-connecting', { url: active.url }, generation);
      if (active.readyState === NativeSocket.OPEN) publish('session-ready', { url: active.url }, generation);
    }
    function OwnedSocket(url, protocols) {
      const socket = protocols === undefined ? new NativeSocket(url) : new NativeSocket(url, protocols);
      if (!characterSocket(url)) return socket;
      active = socket;
      const socketGeneration = ++generation;
      publish('session-connecting', { url: socket.url }, socketGeneration);
      socket.addEventListener('open', () => {
        if (active === socket) publish('session-ready', { url: socket.url }, socketGeneration);
      });
      socket.addEventListener('close', event => {
        if (active !== socket) return;
        active = null;
        publish('session-closed', { code: event.code, reason: event.reason }, socketGeneration);
      });
      socket.addEventListener('message', event => {
        if (active !== socket || typeof event.data !== 'string') return;
        let frame;
        try { frame = JSON.parse(event.data); } catch (_) {}
        if (!frame || !petOnlyFrames.has(frame.type)) publish('socket-message', event.data, socketGeneration);
        if (frame?.type === 'user_transcript') {
          publish('transcript', { transcript: frame.text || frame.transcript || '', is_final: frame.is_final }, socketGeneration);
        }
        if (['agent_status_update', 'agent_task_update', 'agent_notification'].includes(frame?.type)) {
          if (agentTimer === null) agentTimer = setTimeout(() => {
            agentTimer = null;
            if (active !== socket) return;
            const tasks = window._agentTaskMap ? Array.from(window._agentTaskMap.values()) : [];
            publish('agent-update', { tasks,
              running_count: tasks.filter(task => task.status === 'running').length,
              queued_count: tasks.filter(task => task.status === 'queued').length
            }, socketGeneration);
          }, 50);
        }
      });
      return socket;
    }
    OwnedSocket.prototype = NativeSocket.prototype;
    Object.setPrototypeOf(OwnedSocket, NativeSocket);
    window.WebSocket = OwnedSocket;
    host.on('socket-send', message => {
      if (!active || active.readyState !== NativeSocket.OPEN || message.generation !== generation) return;
      if (typeof message.data !== 'string') return;
      try {
        const frame = JSON.parse(message.data);
        if (petOnlyActions.has(frame.action)) return;
      } catch (_) {}
      active.send(message.data);
    });
    // Cache restoration re-registers the document while retaining its native
    // socket. Republish its current lifecycle into the new host epoch.
    let firstDocument = true;
    host.on('document-ready', () => {
      if (firstDocument) { firstDocument = false; return; }
      announce();
    });
    return;
  }

  let lifecycle = null;
  let frames = [];
  const proxies = new Set();
  function stale(message) {
    return lifecycle && (message.epoch < lifecycle.epoch ||
      (message.epoch === lifecycle.epoch && message.generation < lifecycle.generation));
  }
  function adopt(message) {
    if (stale(message)) return false;
    if (!lifecycle || message.epoch !== lifecycle.epoch || message.generation !== lifecycle.generation) frames = [];
    lifecycle = message;
    return true;
  }
  for (const kind of ['session-connecting', 'session-ready', 'session-closed']) {
    host.on(kind, message => {
      if (!adopt(message)) return;
      if (kind !== 'session-ready') frames = [];
      for (const proxy of proxies) proxy.lifecycle(message);
    });
  }
  host.on('socket-message', message => {
    if (stale(message) || lifecycle?.kind !== 'session-ready') return;
    frames.push(message);
    if (frames.length > 256) frames.shift();
    const deliver = () => {
      host.characterSequence = message.sequence || host.characterSequence || 0;
      for (const proxy of proxies) proxy.message(message);
    };
    if (host.deferChatFrame) host.deferChatFrame(message, deliver); else deliver();
  });

  class SocketProxy extends EventTarget {
    constructor(url, protocols) {
      super();
      if (!characterSocket(url)) return protocols === undefined ? new NativeSocket(url) : new NativeSocket(url, protocols);
      this.url = new URL(url, location.href).href;
      this.protocol = '';
      this.extensions = '';
      this.binaryType = 'blob';
      this.bufferedAmount = 0;
      this.readyState = NativeSocket.CONNECTING;
      this.epoch = 0;
      this.generation = 0;
      this.pending = [];
      this.openScheduled = false;
      proxies.add(this);
      if (lifecycle) {
        const initial = lifecycle;
        this.pending = frames.slice();
        this.openScheduled = true;
        setTimeout(() => { this.openScheduled = false; this.lifecycle(initial); }, 0);
      }
    }
    emit(type, event) {
      const handler = this['on' + type];
      if (typeof handler === 'function') handler.call(this, event);
      this.dispatchEvent(event);
    }
    lifecycle(message) {
      if (stale(message) || this.readyState === NativeSocket.CLOSED) return;
      this.epoch = message.epoch;
      this.generation = message.generation;
      if (message.kind === 'session-ready') {
        if (this.readyState !== NativeSocket.OPEN) {
          this.readyState = NativeSocket.OPEN;
          this.emit('open', new Event('open'));
        }
        const pending = this.pending;
        this.pending = [];
        for (const frame of pending) this.message(frame);
      } else if (message.kind === 'session-closed') {
        this.close(message.data?.code || 1006, message.data?.reason || '');
      }
    }
    message(message) {
      if (stale(message)) return;
      if (this.readyState === NativeSocket.OPEN) this.emit('message', new MessageEvent('message', { data: message.data }));
      else if (this.openScheduled) this.pending.push(message);
    }
    send(data) {
      if (this.readyState !== NativeSocket.OPEN) throw new DOMException('Character socket is not open', 'InvalidStateError');
      if (typeof data !== 'string') throw new TypeError('Character proxy only accepts the shared JSON frame contract');
      host.fire(host.publish('socket-send', data, this.generation, this.epoch));
    }
    close(code = 1000, reason = '') {
      if (this.readyState === NativeSocket.CLOSED) return;
      this.readyState = NativeSocket.CLOSED;
      this.pending = [];
      proxies.delete(this);
      this.emit('close', new CloseEvent('close', { code, reason }));
    }
  }
  for (const key of ['CONNECTING', 'OPEN', 'CLOSING', 'CLOSED']) {
    Object.defineProperty(SocketProxy, key, { value: NativeSocket[key] });
    Object.defineProperty(SocketProxy.prototype, key, { value: NativeSocket[key] });
  }
  window.WebSocket = SocketProxy;
})();
