import '@testing-library/jest-dom/vitest';
import { vi } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => undefined),
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => undefined),
}));

Object.defineProperty(HTMLMediaElement.prototype, 'play', {
  configurable: true,
  value: vi.fn(async () => undefined),
});

if (!HTMLElement.prototype.scrollTo) {
  HTMLElement.prototype.scrollTo = function scrollTo(options?: ScrollToOptions | number, _y?: number) {
    if (typeof options === 'number') {
      this.scrollLeft = options;
      if (typeof _y === 'number') {
        this.scrollTop = _y;
      }
      return;
    }
    if (options && typeof options === 'object' && typeof options.top === 'number') {
      this.scrollTop = options.top;
    }
    if (options && typeof options === 'object' && typeof options.left === 'number') {
      this.scrollLeft = options.left;
    }
  };
}

if (typeof window.ResizeObserver === 'undefined') {
  class ResizeObserverStub {
    observe() {}
    unobserve() {}
    disconnect() {}
  }

  window.ResizeObserver = ResizeObserverStub as typeof ResizeObserver;
  globalThis.ResizeObserver = ResizeObserverStub as typeof ResizeObserver;
}
