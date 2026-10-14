import { readFileSync } from 'node:fs';
import { JSDOM } from 'jsdom';
import { describe, expect, it, vi } from 'vitest';

const source = readFileSync('src-tauri/src/desktop/bridge/transport.js', 'utf8');

describe('desktop document activation', () => {
  it('re-registers a cached page with a fresh activation and exactly one set of listeners', async () => {
    const dom = new JSDOM('', { url: 'http://127.0.0.1:48911/chat', runScripts: 'outside-only' });
    const w = dom.window as any;
    const listeners = new Map<string, Set<(event: any) => void>>();
    let activation = 1;
    let document = 0;
    Object.defineProperty(w.performance, 'timeOrigin', { value: 100 });
    Object.defineProperty(w.performance, 'now', { value: () => activation });
    const invoke = vi.fn(async (name: string) => {
      if (name === 'desktop_ready') return { document: ++document, replay: [] };
      return null;
    });
    Object.assign(w, {
      __NEKO_DESKTOP_PROFILE__: { role: 'compact-chat', label: 'chat-compact' },
      __TAURI__: { core: { invoke }, event: {
        listen: async (name: string, handler: (event: any) => void, options: any) => {
          expect(options).toEqual({ target: 'chat-compact' });
          if (!listeners.has(name)) listeners.set(name, new Set());
          listeners.get(name)!.add(handler);
          return () => listeners.get(name)!.delete(handler);
        },
      } },
    });
    try {
      w.eval(source);
      const host = w.__NEKO_DESKTOP__;
      await host.ready;
      const receive = vi.fn();
      host.on('socket-message', receive);
      w.dispatchEvent(new w.PageTransitionEvent('pagehide', { persisted: true }));
      expect([...listeners.values()].every(set => set.size === 0)).toBe(true);
      activation = 3;
      w.dispatchEvent(new w.PageTransitionEvent('pageshow', { persisted: true }));
      await host.ready;
      await host.request('bounds');
      expect(invoke).toHaveBeenCalledWith('desktop_ready', { startedAt: 100, activatedAt: 101 });
      expect(invoke).toHaveBeenCalledWith('desktop_ready', { startedAt: 100, activatedAt: 103 });
      expect(invoke).toHaveBeenLastCalledWith('desktop_window', { request: { action: 'bounds' }, document: 2 });
      expect([...listeners.values()].every(set => set.size === 1)).toBe(true);
      for (const listener of listeners.get('neko-desktop-message')!) {
        listener({ payload: { kind: 'socket-message', data: 'frame', sequence: 1 } });
      }
      expect(receive).toHaveBeenCalledOnce();
    } finally { dom.window.close(); }
  });
});
