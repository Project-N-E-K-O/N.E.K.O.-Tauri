(function installNekoLinuxInputRegionBridge() {
  'use strict';

  if (window.__NEKO_TAURI_CLICK_THROUGH_INSTALLED__) return;
  var invoke = window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
  if (typeof invoke !== 'function') return;
  window.__NEKO_TAURI_CLICK_THROUGH_INSTALLED__ = true;

  // Persistent regions work even when the window receives no pointer events
  // (including native Wayland, where global cursor coordinates are unavailable).
  var MODEL_SOURCES = ['live2d', 'vrm', 'mmd', 'pngtuber'];
  var SURFACE_SELECTOR = [
    '#react-chat-window-shell', '#agent-task-hud', '.modal-overlay',
    '[role="dialog"]', '[data-neko-sidepanel]',
    '.live2d-popup', '.vrm-popup', '.mmd-popup', '.pngtuber-popup',
    '[id$="-floating-buttons"]', '[id$="-return-button-container"]',
    '[id$="-lock-icon"]', '#subtitle-display.show', '#status-toast.show'
  ].join(',');
  var CONTROL_SELECTOR = [
    'button', 'input', 'textarea', 'select', 'a[href]',
    '[role="button"]', '[role="switch"]', '[contenteditable="true"]'
  ].join(',');
  var pointerDown = false;
  var inFlight = false;
  var lastDetails = null;
  var stopped = false;
  var generation = 0;
  var timer = null;

  function visible(element, visualOnly) {
    if (!element || !element.isConnected) return false;
    // Child controls may override a parent's pointer-events:none. Hidden and
    // transparent ancestors, however, make the whole subtree invisible.
    if (!visualOnly && window.getComputedStyle(element).pointerEvents === 'none') return false;
    for (var node = element; node && node.nodeType === 1; node = node.parentElement) {
      var style = window.getComputedStyle(node);
      if (node.hidden || style.display === 'none' || style.visibility === 'hidden' ||
          Number(style.opacity || '1') <= 0.01) return false;
    }
    return true;
  }

  function normalizeRect(bounds) {
    if (!bounds) return null;
    var left = Number(bounds.left != null ? bounds.left : bounds.x);
    var top = Number(bounds.top != null ? bounds.top : bounds.y);
    var right = Number(bounds.right != null ? bounds.right : left + Number(bounds.width));
    var bottom = Number(bounds.bottom != null ? bounds.bottom : top + Number(bounds.height));
    if (![left, top, right, bottom].every(Number.isFinite)) return null;
    left = Math.max(0, left);
    top = Math.max(0, top);
    right = Math.min(window.innerWidth, right);
    bottom = Math.min(window.innerHeight, bottom);
    if (right <= left || bottom <= top) return null;
    // Round outward, then deduplicate unchanged snapshots before invoking Rust.
    return [Math.floor(left), Math.floor(top),
      Math.ceil(right) - Math.floor(left), Math.ceil(bottom) - Math.floor(top)];
  }

  function covered(rectangles, rect) {
    return rectangles.some(function(existing) {
      return existing[0] <= rect[0] && existing[1] <= rect[1] &&
        existing[0] + existing[2] >= rect[0] + rect[2] &&
        existing[1] + existing[3] >= rect[1] + rect[3];
    });
  }

  function appendRect(rectangles, bounds) {
    if (rectangles.length >= 128) return;
    var rect = normalizeRect(bounds);
    if (rect && !covered(rectangles, rect)) rectangles.push(rect);
  }

  function collectElements(rectangles, selector) {
    var elements = document.querySelectorAll(selector);
    for (var index = 0; index < elements.length && rectangles.length < 128; index += 1) {
      var element = elements[index];
      var rect = normalizeRect(element.getBoundingClientRect());
      // Chat links/buttons already inside a surface need neither another
      // rectangle nor repeated ancestor style queries. Overflowing popups are
      // still inspected: DOM ancestry alone does not imply geometric coverage.
      if (rect && !covered(rectangles, rect) && visible(element)) rectangles.push(rect);
    }
  }

  function collectRegions() {
    var rectangles = [];
    if (pointerDown) {
      appendRect(rectangles, { x: 0, y: 0, width: window.innerWidth, height: window.innerHeight });
      return rectangles;
    }
    MODEL_SOURCES.forEach(function(type) {
      var container = document.getElementById(type + '-container');
      var manager = window[type + 'Manager'];
      // Upstream's full-screen model container intentionally has
      // pointer-events:none; the canvas opts in separately when unlocked.
      if (!visible(container, true) || !manager || manager.isLocked === true ||
          (manager.currentModel && manager.currentModel.visible === false)) return;
      try {
        if (typeof manager.getModelScreenBounds === 'function') {
          appendRect(rectangles, manager.getModelScreenBounds());
        } else if (type === 'pngtuber') {
          var image = container.querySelector('.pngtuber-image, img');
          if (visible(image)) appendRect(rectangles, image.getBoundingClientRect());
        }
      } catch (_) {
        // Model creation/disposal can briefly invalidate its geometry.
      }
    });
    // Reserve whole surfaces before their potentially numerous child controls.
    collectElements(rectangles, SURFACE_SELECTOR);
    collectElements(rectangles, CONTROL_SELECTOR);
    return rectangles;
  }

  function refresh() {
    if (stopped || inFlight) return;
    var details = JSON.stringify({
      dpr: window.devicePixelRatio || 1,
      persistentRects: collectRegions()
    });
    if (details === lastDetails) return;
    inFlight = true;
    var requestGeneration = generation;
    // Linux ignore=true preserves persistentRects and lets every other point
    // pass through. It is not the whole-window mouse-ignore used on Win/macOS.
    Promise.resolve(invoke('set_click_through', {
      ignore: true,
      reason: pointerDown ? 'drag-active-regions' : 'linux-persistent-regions',
      details: details
    })).then(function() {
      if (requestGeneration === generation) lastDetails = details;
    }).catch(function(error) {
      if (requestGeneration === generation) console.error('更新 Linux 输入区域失败:', error);
    }).finally(function() {
      if (requestGeneration === generation) inFlight = false;
    });
  }

  function press() { pointerDown = true; refresh(); }
  function release() { pointerDown = false; refresh(); }
  function move(event) { if (pointerDown && event.buttons === 0) release(); }
  function setInputListeners(enabled) {
    var method = enabled ? 'addEventListener' : 'removeEventListener';
    window[method]('pointerdown', press, true);
    window[method]('pointerup', release, true);
    window[method]('pointercancel', release, true);
    window[method]('pointermove', move, true);
    window[method]('blur', release);
    window[method]('resize', refresh);
  }

  // A small bounded poll also catches model animation and late loading without
  // maintaining observers/hooks for each upstream renderer. IPC only changes
  // when the region changes, and never includes DOM text or chat content.
  function start() {
    stopped = false;
    pointerDown = false;
    lastDetails = null;
    setInputListeners(true);
    timer = window.setInterval(refresh, 500);
    refresh();
  }

  function pagehide(event) {
    stopped = true;
    window.clearInterval(timer);
    timer = null;
    setInputListeners(false);
    generation += 1;
    inFlight = false;
    if (!event.persisted) {
      window.removeEventListener('pagehide', pagehide);
      window.removeEventListener('pageshow', pageshow);
      delete window.__NEKO_TAURI_CLICK_THROUGH_INSTALLED__;
    }
  }

  function pageshow(event) {
    if (event.persisted && stopped) start();
  }

  window.addEventListener('pagehide', pagehide);
  window.addEventListener('pageshow', pageshow);
  start();
})();
