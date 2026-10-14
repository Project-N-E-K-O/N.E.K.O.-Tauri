import { readFileSync } from 'node:fs';
import { JSDOM } from 'jsdom';
import { afterEach, describe, expect, it, vi } from 'vitest';

const source = readFileSync('src-tauri/src/desktop/bridge/websocket.js', 'utf8');
const documents: JSDOM[] = [];

function page(role: string) {
  const dom = new JSDOM('', { url: 'http://127.0.0.1:48911/chat', runScripts: 'outside-only' });
  documents.push(dom);
  const w = dom.window;
  const connections: any[] = [];
  class NativeSocket extends w.EventTarget {
    static CONNECTING = 0;
    static OPEN = 1;
    static CLOSING = 2;
    static CLOSED = 3;
    readyState = 0;
    send = vi.fn();
    constructor(public url: string) { super(); connections.push(this); }
    open() { this.readyState = 1; this.dispatchEvent(new w.Event('open')); }
    message(data: unknown) { this.dispatchEvent(new w.MessageEvent('message', { data })); }
    close() { this.readyState = 3; this.dispatchEvent(new w.CloseEvent('close', { code: 1000 })); }
  }
  const handlers = new Map<string, Array<(message: any) => void>>();
  const publish = vi.fn(() => Promise.resolve());
  Object.assign(w, {
    WebSocket: NativeSocket,
    __NEKO_DESKTOP__: {
      profile: { role }, publish,
      fire: (result: Promise<unknown>) => result.catch(() => {}),
      on(kind: string, handler: (message: any) => void) {
        if (!handlers.has(kind)) handlers.set(kind, []);
        handlers.get(kind)!.push(handler);
      },
    },
  });
  w.eval(source);
  return {
    w, publish, connections,
    deliver(kind: string, data: unknown, generation = 1, epoch = 1) {
      for (const handler of handlers.get(kind) || []) handler({ kind, data, generation, epoch });
    },
  };
}

afterEach(() => { for (const document of documents.splice(0)) document.window.close(); });

describe('desktop character socket ownership', () => {
  it('opens one real Pet socket while chat input uses the proxy', async () => {
    const pet = page('pet');
    const chat = page('compact-chat');
    const owner = new pet.w.WebSocket('ws://127.0.0.1:48911/ws/cat') as any;
    owner.open();
    const proxy = new chat.w.WebSocket('ws://127.0.0.1:48911/ws/cat');
    chat.deliver('session-ready', { url: owner.url });
    proxy.send('{"action":"stream_data","input_type":"text","data":"hello"}');
    const [kind, data, generation] = chat.publish.mock.calls[0] as unknown as [string, string, number];
    pet.deliver(kind, data, generation);
    expect(pet.connections).toHaveLength(1);
    expect(chat.connections).toHaveLength(0);
    expect(owner.send).toHaveBeenCalledExactlyOnceWith(data);
    proxy.close();
    expect(owner.readyState).toBe(1);
  });

  it('delivers open before messages that arrived before chat constructed its socket', async () => {
    const chat = page('full-chat');
    chat.deliver('session-ready', {});
    chat.deliver('socket-message', '{"type":"gemini_response","text":"hello"}');
    const events: string[] = [];
    const proxy = new chat.w.WebSocket('ws://127.0.0.1:48911/ws/cat');
    proxy.onopen = () => events.push('open');
    proxy.onmessage = event => events.push(String(event.data));
    await new Promise(resolve => chat.w.setTimeout(resolve, 10));
    expect(events).toEqual(['open', '{"type":"gemini_response","text":"hello"}']);
  });

  it('suppresses replaced socket output and keeps audio and capture requests on Pet', () => {
    const pet = page('pet');
    const first = new pet.w.WebSocket('ws://127.0.0.1:48911/ws/first') as any;
    first.open();
    const second = new pet.w.WebSocket('ws://127.0.0.1:48911/ws/second') as any;
    second.open();
    pet.publish.mockClear();
    first.message('{"type":"gemini_response","text":"old"}');
    first.close();
    second.message('{"type":"audio_chunk","speech_id":"s"}');
    second.message(new ArrayBuffer(16));
    second.message('{"type":"capture_bridge_request","request_id":"capture"}');
    expect(pet.publish).not.toHaveBeenCalled();
    second.message('{"type":"gemini_response","text":"current"}');
    expect(pet.publish).toHaveBeenCalledExactlyOnceWith('socket-message', '{"type":"gemini_response","text":"current"}', 2);
  });

  it('keeps unrelated websocket protocols native in both surfaces', () => {
    for (const role of ['pet', 'compact-chat']) {
      const document = page(role);
      new document.w.WebSocket('ws://127.0.0.1:48911/notifications');
      expect(document.connections).toHaveLength(1);
      expect(document.publish).not.toHaveBeenCalled();
    }
  });

  it('does not replay a held frame before the pending chat snapshot is restored', async () => {
    const chat = page('full-chat');
    const held: Array<() => void> = [];
    (chat.w as any).__NEKO_DESKTOP__.deferChatFrame = (_message: unknown, deliver: () => void) => held.push(deliver);
    chat.deliver('session-ready', {});
    chat.deliver('socket-message', '{"type":"gemini_response","text":"after snapshot"}');
    const proxy = new chat.w.WebSocket('ws://127.0.0.1:48911/ws/cat');
    const frames: string[] = [];
    proxy.onmessage = event => frames.push(String(event.data));
    await new Promise(resolve => chat.w.setTimeout(resolve, 10));
    expect(frames).toEqual([]);
    held[0]();
    expect(frames).toEqual(['{"type":"gemini_response","text":"after snapshot"}']);
  });

  it('ignores a close from the previous Pet document epoch', () => {
    const chat = page('compact-chat');
    const socket = new chat.w.WebSocket('ws://127.0.0.1:48911/ws/cat');
    chat.deliver('session-ready', {}, 2, 2);
    chat.deliver('session-closed', { code: 1006 }, 10, 1);
    expect(socket.readyState).toBe(1);
    socket.send('{"action":"ping"}');
    expect(chat.publish).toHaveBeenCalledExactlyOnceWith('socket-send', '{"action":"ping"}', 2, 2);
  });
});
