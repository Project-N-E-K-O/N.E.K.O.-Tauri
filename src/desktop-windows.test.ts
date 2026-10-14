import { readFileSync } from 'node:fs';
import { JSDOM } from 'jsdom';
import { describe, expect, it, vi } from 'vitest';

const source = readFileSync('src-tauri/src/desktop/bridge/windows.js', 'utf8');
function page() {
  const dom = new JSDOM('', { url: 'http://127.0.0.1:48911/chat', runScripts: 'outside-only' });
  const handlers = new Map<string, (message: any) => void>();
  const request = vi.fn(async (action: string) => action === 'open-child' ? { label: 'child-test' } : null);
  Object.assign(dom.window, { __NEKO_DESKTOP__: { profile: { role: 'full-chat', parent: 'main' },
    request, fire: (operation: Promise<unknown>) => operation.catch(() => {}),
    on: (kind: string, handler: (message: any) => void) => handlers.set(kind, handler) } });
  dom.window.eval(source);
  return { dom, request, handlers };
}

describe('native feature window interface', () => {
  it('honors feature geometry, reuses a named window, and explicitly navigates on reuse', async () => {
    const { dom, request } = page();
    const window = dom.window as any;
    const first = window.open('/settings', 'settings', 'width=800,height=600,left=-40,top=24');
    await first.ready;
    expect(request).toHaveBeenCalledWith('open-child', { url: 'http://127.0.0.1:48911/settings',
      name: 'settings', width: 800, height: 600, x: -40, y: 24 });
    expect(window.openOrFocusWindow('/next', 'settings', '', { navigateOnReuse: true })).toBe(first);
    await Promise.resolve();
    expect(request).toHaveBeenCalledWith('open-child', { url: 'http://127.0.0.1:48911/next', name: 'settings', navigate: true });
    dom.window.close();
  });

  it('tracks a native close rather than retaining a stale live handle', async () => {
    const { dom, handlers } = page();
    const window = dom.window as any;
    const first = window.open('/settings', 'settings');
    await first.ready;
    handlers.get('child-closed')!({ data: 'child-test' });
    expect(first.closed).toBe(true);
    expect(window.open('/settings', 'settings')).not.toBe(first);
    dom.window.close();
  });

  it('closes a window requested while its native creation was still pending', async () => {
    const { dom, request } = page();
    const window = dom.window as any;
    const child = window.open('/settings', '_blank');
    child.close();
    await child.ready;
    expect(request).toHaveBeenCalledWith('close-child', { label: 'child-test' });
    dom.window.close();
  });
});
