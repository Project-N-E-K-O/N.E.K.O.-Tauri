// @vitest-environment jsdom
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

// Load the script selected by the real Linux platform module, so these tests
// also catch a bridge that exists but is never injected on Linux.
const platformDir = resolve('src-tauri/src/platform');
const platformSource = readFileSync(resolve(platformDir, 'linux.rs'), 'utf8');
const scriptPath = platformSource.match(/CLICK_THROUGH_BRIDGE: &str = include_str!\("([^"]+)"\)/)?.[1];
const bridgeSource = scriptPath ? readFileSync(resolve(platformDir, scriptPath), 'utf8') : '';
const globals = window as unknown as Record<string, unknown>;

function element(id: string, bounds = { x: 100, y: 80, width: 200, height: 300 }, tag = 'div') {
  const node = document.createElement(tag);
  node.id = id;
  node.getBoundingClientRect = () => ({ ...bounds, left: bounds.x, top: bounds.y,
    right: bounds.x + bounds.width, bottom: bounds.y + bounds.height, toJSON() {} });
  document.body.appendChild(node);
  return node;
}

function install(invoke = vi.fn(() => Promise.resolve())) {
  globals.__TAURI__ = { core: { invoke } };
  window.eval(bridgeSource);
  return invoke;
}

function rectangles(invoke: ReturnType<typeof install>) {
  const args = invoke.mock.calls.at(-1)?.[1] as { details: string } | undefined;
  expect(args, 'Linux must submit a region without upstream radar or mouse events').toBeDefined();
  return JSON.parse(args!.details).persistentRects as number[][];
}

describe('Linux host input-region bridge', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    document.body.innerHTML = '';
    for (const key of ['__TAURI__', '__NEKO_TAURI_CLICK_THROUGH_INSTALLED__',
      'live2dManager', 'vrmManager', 'mmdManager', 'pngtuberManager']) delete globals[key];
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1920 });
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 1080 });
  });
  afterEach(() => {
    window.dispatchEvent(new Event('pagehide'));
    vi.clearAllTimers();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('opens model and chat regions immediately without upstream Tauri support', () => {
    // The real upstream full-screen model container opts out of DOM input;
    // its canvas opts in. Container pointer-events must not hide the model.
    element('live2d-container').style.pointerEvents = 'none';
    element('react-chat-window-shell', { x: 400, y: 300, width: 500, height: 400 });
    globals.live2dManager = { getModelScreenBounds: () => ({ left: 100, top: 80, right: 300, bottom: 380 }) };
    const invoke = install();
    expect(rectangles(invoke)).toEqual([[100, 80, 200, 300], [400, 300, 500, 400]]);
    expect(invoke.mock.calls[0][1]).toMatchObject({ ignore: true });
  });

  it('updates late-loading and moving models while the pointer is stationary', async () => {
    element('live2d-container');
    const invoke = install();
    expect(rectangles(invoke)).toEqual([]);
    let left = 100;
    globals.live2dManager = { getModelScreenBounds: () => ({ left, top: 80, width: 200, height: 300 }) };
    await vi.advanceTimersByTimeAsync(500);
    expect(rectangles(invoke)).toEqual([[100, 80, 200, 300]]);
    left = 500;
    await vi.advanceTimersByTimeAsync(500);
    expect(rectangles(invoke)).toEqual([[500, 80, 200, 300]]);
  });

  it('keeps backgrounds, hidden ancestors and locked models out of the region', async () => {
    element('live2d-container');
    globals.live2dManager = { isLocked: true, getModelScreenBounds: () => ({ x: 100, y: 80, width: 200, height: 300 }) };
    const chat = element('react-chat-window-shell');
    const wrapper = document.createElement('section');
    wrapper.style.opacity = '0';
    document.body.appendChild(wrapper);
    wrapper.appendChild(chat);
    element('live2d-canvas', { x: 0, y: 0, width: 1920, height: 1080 });
    const invoke = install();
    expect(rectangles(invoke)).toEqual([]);
    wrapper.style.opacity = '1';
    await vi.advanceTimersByTimeAsync(500);
    expect(rectangles(invoke)).toEqual([[100, 80, 200, 300]]);
  });

  it('holds input across a drag outside the model and releases it on pointerup', async () => {
    const chat = element('react-chat-window-shell');
    const invoke = install();
    window.dispatchEvent(new Event('pointerdown'));
    await vi.advanceTimersByTimeAsync(500);
    expect(rectangles(invoke)).toEqual([[0, 0, 1920, 1080]]);
    chat.remove();
    window.dispatchEvent(new Event('pointerup'));
    await vi.advanceTimersByTimeAsync(500);
    expect(rectangles(invoke)).toEqual([]);
  });

  it('includes the Agent HUD body/header, not only its internal buttons', async () => {
    const hud = element('agent-task-hud', { x: 400, y: 200, width: 320, height: 400 });
    const header = element('agent-task-hud-header', { x: 400, y: 200, width: 320, height: 48 });
    hud.appendChild(header); // upstream header is a div, not a button/dialog
    const invoke = install();
    expect(rectangles(invoke)).toEqual([[400, 200, 320, 400]]);
    header.dispatchEvent(new Event('pointerdown', { bubbles: true }));
    await vi.advanceTimersByTimeAsync(500);
    expect(rectangles(invoke)).toEqual([[0, 0, 1920, 1080]]);
    window.dispatchEvent(new Event('pointerup'));
    await vi.advanceTimersByTimeAsync(500);
    expect(rectangles(invoke)).toEqual([[400, 200, 320, 400]]);
    hud.style.display = 'none';
    await vi.advanceTimersByTimeAsync(500);
    expect(rectangles(invoke)).toEqual([]);
  });

  it('deduplicates unchanged regions, retries failures and installs only once', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    element('react-chat-window-shell');
    const invoke = vi.fn().mockRejectedValueOnce(new Error('IPC unavailable')).mockResolvedValue(undefined);
    install(invoke);
    window.eval(bridgeSource);
    await vi.advanceTimersByTimeAsync(500);
    expect(invoke).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(500);
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it('bounds idle work and releases its timer/listeners on pagehide', async () => {
    element('live2d-container').style.pointerEvents = 'none';
    const bounds = vi.fn(() => ({ x: 100, y: 80, width: 200, height: 300 }));
    globals.live2dManager = { getModelScreenBounds: bounds };
    const invoke = install();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(bounds).toHaveBeenCalledTimes(21); // initial + two samples/second
    expect(invoke).toHaveBeenCalledTimes(1); // unchanged regions never enqueue IPC
    window.dispatchEvent(new Event('pagehide'));
    expect(vi.getTimerCount()).toBe(0);
    window.dispatchEvent(new Event('pointerdown'));
    await vi.advanceTimersByTimeAsync(10_000);
    expect(bounds).toHaveBeenCalledTimes(21);
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it('does not accumulate IPC requests when the native command is pending', async () => {
    element('react-chat-window-shell');
    const invoke = vi.fn(() => new Promise<void>(() => {}));
    install(invoke);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it('covers model popups outside floating buttons, including div menu items', () => {
    for (const [index, type] of ['live2d', 'vrm', 'mmd', 'pngtuber'].entries()) {
      const popup = element(type + '-popup-settings', { x: 400 + index * 300, y: 100, width: 240, height: 200 });
      popup.className = type + '-popup';
      const item = element(type + '-menu-api', { x: 410 + index * 300, y: 110, width: 200, height: 30 });
      item.setAttribute('role', 'button');
      popup.appendChild(item);
    }
    const invoke = install();
    expect(rectangles(invoke)).toEqual([
      [400, 100, 240, 200], [700, 100, 240, 200],
      [1000, 100, 240, 200], [1300, 100, 240, 200],
    ]);
  });

  it('covers a visible modal backdrop and releases it when the modal closes', async () => {
    const overlay = element('confirmation-overlay', { x: 0, y: 0, width: 1920, height: 1080 });
    overlay.className = 'modal-overlay';
    const dialog = element('confirmation-dialog', { x: 400, y: 300, width: 400, height: 300 });
    dialog.setAttribute('role', 'dialog');
    overlay.appendChild(dialog);
    const invoke = install();
    expect(rectangles(invoke)).toEqual([[0, 0, 1920, 1080]]);
    overlay.style.display = 'none';
    await vi.advanceTimersByTimeAsync(500);
    expect(rectangles(invoke)).toEqual([]);
  });

  it('includes standalone ARIA buttons and switches outside known surfaces', () => {
    element('menu-action', { x: 400, y: 100, width: 200, height: 40 }).setAttribute('role', 'button');
    element('toggle-action', { x: 700, y: 100, width: 200, height: 40 }).setAttribute('role', 'switch');
    const invoke = install();
    expect(rectangles(invoke)).toEqual([[400, 100, 200, 40], [700, 100, 200, 40]]);
  });

  it('reserves surface regions before a long chat and avoids redundant style scans', () => {
    const chat = element('react-chat-window-shell', { x: 0, y: 0, width: 400, height: 600 });
    for (let i = 0; i < 140; i++) {
      const link = element('chat-link-' + i, { x: 10, y: 10, width: 20, height: 10 }, 'a');
      link.setAttribute('href', '/');
      chat.appendChild(link);
    }
    element('agent-task-hud', { x: 600, y: 100, width: 320, height: 400 });
    const styles = vi.spyOn(window, 'getComputedStyle');
    const invoke = install();
    expect(rectangles(invoke)).toEqual([[0, 0, 400, 600], [600, 100, 320, 400]]);
    expect(styles.mock.calls.filter(([node]) => node.tagName === 'A')).toHaveLength(0);
  });

  it('stops inspecting controls once the native rectangle budget is exhausted', () => {
    let lastButton: HTMLElement | undefined;
    for (let i = 0; i < 150; i++) {
      lastButton = element('control-' + i, { x: (i % 20) * 30, y: Math.floor(i / 20) * 30, width: 10, height: 10 }, 'button');
    }
    const bounds = vi.spyOn(lastButton!, 'getBoundingClientRect');
    const styles = vi.spyOn(window, 'getComputedStyle');
    const invoke = install();
    expect(rectangles(invoke)).toHaveLength(128);
    expect(bounds).not.toHaveBeenCalled();
    expect(styles.mock.calls.some(([node]) => node === lastButton)).toBe(false);
  });

  it('resumes exactly one bridge when a cached document is restored', async () => {
    const chat = element('react-chat-window-shell');
    const invoke = install();
    await vi.advanceTimersByTimeAsync(500);
    window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
    expect(vi.getTimerCount()).toBe(0);
    chat.getBoundingClientRect = () => ({ x: 500, y: 100, left: 500, top: 100, right: 800, bottom: 400, width: 300, height: 300, toJSON() {} });
    window.eval(bridgeSource); // Tauri may reinject before pageshow on a history navigation.
    window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
    window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
    expect(vi.getTimerCount()).toBe(1);
    expect(rectangles(invoke)).toEqual([[500, 100, 300, 300]]);
    await vi.advanceTimersByTimeAsync(1000);
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it('ignores pre-cache IPC completions without releasing a newer pending request', async () => {
    const chat = element('react-chat-window-shell');
    const complete: Array<() => void> = [];
    const invoke = vi.fn(() => new Promise<void>((resolve) => complete.push(resolve)));
    install(invoke);
    window.dispatchEvent(new PageTransitionEvent('pagehide', { persisted: true }));
    chat.getBoundingClientRect = () => ({ x: 500, y: 100, left: 500, top: 100, right: 800, bottom: 400, width: 300, height: 300, toJSON() {} });
    window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: true }));
    expect(invoke).toHaveBeenCalledTimes(2);
    complete[0]();
    await vi.advanceTimersByTimeAsync(1000);
    expect(invoke).toHaveBeenCalledTimes(2);
    complete[1]();
    await vi.advanceTimersByTimeAsync(1000);
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(rectangles(invoke)).toEqual([[500, 100, 300, 300]]);
  });

  it('clips CSS rectangles and preserves DPR for the native conversion', () => {
    Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value: 1.75 });
    element('react-chat-window-shell', { x: -10, y: -20, width: 100, height: 80 });
    const invoke = install();
    expect(rectangles(invoke)).toEqual([[0, 0, 90, 60]]);
    expect(JSON.parse(invoke.mock.calls[0][1].details).dpr).toBe(1.75);
  });
});
