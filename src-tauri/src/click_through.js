(function installNekoTauriClickThroughBridge() {
  'use strict';

  if (window.__NEKO_TAURI_CLICK_THROUGH_INSTALLED__) return;

  var tauri = window.__TAURI__;
  var listen = tauri && tauri.event && tauri.event.listen;
  var invoke = tauri && tauri.core && tauri.core.invoke;
  if (typeof listen !== 'function' || typeof invoke !== 'function') return;

  window.__NEKO_TAURI_CLICK_THROUGH_INSTALLED__ = true;

  var BACKGROUND_IDS = {
    app: true,
    root: true,
    'live2d-container': true,
    'live2d-canvas': true,
    'vrm-container': true,
    'vrm-canvas': true,
    'mmd-container': true,
    'mmd-canvas': true,
    'pngtuber-container': true,
    'react-chat-window-overlay': true,
    'react-chat-window-backdrop': true
  };
  var BACKGROUND_CLASSES = [
    'app-root',
    'transparent-overlay',
    'model-overlay',
    'pet-overlay',
    'live2d-container',
    'vrm-container',
    'mmd-container',
    'pngtuber-container'
  ];
  var MODEL_SOURCES = [
    { containerId: 'live2d-container', managerName: 'live2dManager' },
    { containerId: 'vrm-container', managerName: 'vrmManager' },
    { containerId: 'mmd-container', managerName: 'mmdManager' },
    { containerId: 'pngtuber-container', managerName: 'pngtuberManager' }
  ];
  var MODEL_EXIT_MARGIN = 10;

  var isIgnoring = false;
  var desiredIgnore = false;
  var ignoreTransition = null;
  var wasOverModel = false;
  var pointerIsDown = false;

  function isFiniteNumber(value) {
    return typeof value === 'number' && Number.isFinite(value);
  }

  function isElementVisible(element) {
    if (!element || !element.isConnected) return false;
    if (element.hidden) return false;

    var style = window.getComputedStyle(element);
    if (!style) return true;
    if (style.display === 'none' || style.visibility === 'hidden') return false;
    return Number(style.opacity || '1') > 0.01;
  }

  function isBackgroundElement(element) {
    if (!element || element.nodeType !== 1) return true;
    if (element === document.body || element === document.documentElement) return true;
    if (element.tagName === 'HTML' || BACKGROUND_IDS[element.id]) return true;

    for (var index = 0; index < BACKGROUND_CLASSES.length; index += 1) {
      if (element.classList && element.classList.contains(BACKGROUND_CLASSES[index])) {
        return true;
      }
    }
    return false;
  }

  function isInteractiveDomPoint(x, y) {
    var elements = [];
    if (typeof document.elementsFromPoint === 'function') {
      elements = document.elementsFromPoint(x, y);
    } else if (typeof document.elementFromPoint === 'function') {
      var element = document.elementFromPoint(x, y);
      if (element) elements = [element];
    }

    for (var index = 0; index < elements.length; index += 1) {
      var candidate = elements[index];
      if (!isBackgroundElement(candidate) && isElementVisible(candidate)) {
        return true;
      }
    }
    return false;
  }

  function normalizeBounds(rawBounds) {
    if (!rawBounds || typeof rawBounds !== 'object') return null;

    var left = Number(rawBounds.left != null ? rawBounds.left : rawBounds.x);
    var top = Number(rawBounds.top != null ? rawBounds.top : rawBounds.y);
    var right = Number(
      rawBounds.right != null
        ? rawBounds.right
        : left + Number(rawBounds.width)
    );
    var bottom = Number(
      rawBounds.bottom != null
        ? rawBounds.bottom
        : top + Number(rawBounds.height)
    );

    if (
      !Number.isFinite(left) ||
      !Number.isFinite(top) ||
      !Number.isFinite(right) ||
      !Number.isFinite(bottom) ||
      right <= left ||
      bottom <= top
    ) {
      return null;
    }
    return { left: left, top: top, right: right, bottom: bottom };
  }

  function pointIsInsideBounds(bounds, x, y, margin) {
    return (
      bounds &&
      x >= bounds.left - margin &&
      x <= bounds.right + margin &&
      y >= bounds.top - margin &&
      y <= bounds.bottom + margin
    );
  }

  function getManagerModelBounds(source) {
    var container = document.getElementById(source.containerId);
    if (!isElementVisible(container)) return null;

    var manager = window[source.managerName];
    if (!manager) return null;

    if (typeof manager.getModelScreenBounds === 'function') {
      try {
        var managerBounds = normalizeBounds(manager.getModelScreenBounds());
        if (managerBounds) return managerBounds;
      } catch (_) {}
    }

    if (source.managerName === 'pngtuberManager') {
      var image = container.querySelector('.pngtuber-image, img');
      if (isElementVisible(image)) return normalizeBounds(image.getBoundingClientRect());
    }

    return null;
  }

  function getManagerCanvas(source, manager, container) {
    if (source.managerName === 'live2dManager') {
      var pixi = manager && manager.pixi_app;
      if (pixi && (pixi.view || pixi.canvas)) return pixi.view || pixi.canvas;
    }

    if (manager && manager.renderer && manager.renderer.domElement) {
      return manager.renderer.domElement;
    }

    return container && container.querySelector
      ? container.querySelector('canvas')
      : null;
  }

  function getLive2DRendererPoint(manager, container, x, y) {
    var canvas = getManagerCanvas(
      { managerName: 'live2dManager' },
      manager,
      container,
    );
    if (!canvas || typeof canvas.getBoundingClientRect !== 'function') return null;

    var rect = canvas.getBoundingClientRect();
    if (!rect || !(rect.width > 0) || !(rect.height > 0)) return null;
    if (x < rect.left || x > rect.right || y < rect.top || y > rect.bottom) return null;

    var renderer = manager && manager.pixi_app && manager.pixi_app.renderer;
    var screen = renderer && renderer.screen;
    var rendererWidth = Number(screen && screen.width);
    var rendererHeight = Number(screen && screen.height);
    var scaleX = rendererWidth > 0 ? rendererWidth / rect.width : 1;
    var scaleY = rendererHeight > 0 ? rendererHeight / rect.height : 1;

    return {
      x: (x - rect.left) * scaleX,
      y: (y - rect.top) * scaleY,
    };
  }

  function getLive2DInputRegionRects(manager) {
    if (typeof manager.getModelInputRegionRects !== 'function') return null;

    try {
      var rects = manager.getModelInputRegionRects({ padding: 8 });
      if (!Array.isArray(rects)) return null;
      return rects.map(normalizeBounds).filter(Boolean);
    } catch (_) {
      return null;
    }
  }

  // A precise result is authoritative. Do not widen a precise miss back to
  // the coarse model bounds, because the native window covers the desktop.
  function getPreciseManagerHit(source, x, y) {
    var container = document.getElementById(source.containerId);
    if (!isElementVisible(container)) return false;

    var manager = window[source.managerName];
    if (!manager) return null;

    if (source.managerName === 'live2dManager') {
      var inputRegionRects = getLive2DInputRegionRects(manager);
      if (inputRegionRects !== null) {
        return inputRegionRects.some(function(rect) {
          return pointIsInsideBounds(rect, x, y, 0);
        });
      }
    }

    if (
      source.managerName === 'live2dManager' &&
      manager.currentModel &&
      typeof manager.currentModel.hitTest === 'function'
    ) {

      var point = getLive2DRendererPoint(manager, container, x, y);
      if (!point) return false;

      try {
        var hitAreas = manager.currentModel.hitTest(point.x, point.y);
        return Array.isArray(hitAreas) ? hitAreas.length > 0 : Boolean(hitAreas);
      } catch (_) {
        return false;
      }
    }

    if (typeof manager.hitTest === 'function') {
      try {
        return Boolean(manager.hitTest(x, y));
      } catch (_) {
        return false;
      }
    }

    var interaction = manager.interaction;
    if (interaction && typeof interaction._hitTestModel === 'function') {
      try {
        return Boolean(interaction._hitTestModel(x, y));
      } catch (_) {
        return false;
      }
    }

    return null;
  }

  function isPointOverModel(x, y) {
    var margin = wasOverModel ? MODEL_EXIT_MARGIN : 0;
    for (var index = 0; index < MODEL_SOURCES.length; index += 1) {
      var preciseHit = getPreciseManagerHit(MODEL_SOURCES[index], x, y);
      if (preciseHit !== null) {
        if (preciseHit) return true;
        continue;
      }

      var bounds = getManagerModelBounds(MODEL_SOURCES[index]);
      if (pointIsInsideBounds(bounds, x, y, margin)) return true;
    }
    return false;
  }

  function applyIgnoreState(shouldIgnore) {
    desiredIgnore = Boolean(shouldIgnore);
    if (ignoreTransition || desiredIgnore === isIgnoring) return;

    ignoreTransition = (async function() {
      while (isIgnoring !== desiredIgnore) {
        var nextIgnore = desiredIgnore;
        try {
          await invoke('set_click_through', { ignore: nextIgnore });
          isIgnoring = nextIgnore;
        } catch (error) {
          console.error('切换 Tauri 鼠标穿透状态失败:', error);
          desiredIgnore = isIgnoring;
          break;
        }
      }
    })().finally(function() {
      ignoreTransition = null;
      if (isIgnoring !== desiredIgnore) applyIgnoreState(desiredIgnore);
    });
  }

  function handleRadarPoint(x, y) {
    if (!isFiniteNumber(x) || !isFiniteNumber(y)) return;

    // Once an interaction starts in this window, keep receiving input until
    // the button is released. Otherwise crossing a model edge mid-drag would
    // make the native window ignore the matching mouseup event.
    if (pointerIsDown) {
      applyIgnoreState(false);
      return;
    }

    var overModel = isPointOverModel(x, y);
    wasOverModel = overModel;
    var overUi = isInteractiveDomPoint(x, y);
    applyIgnoreState(!(overModel || overUi));
  }

  window.addEventListener('pointerdown', function() {
    pointerIsDown = true;
  }, true);
  window.addEventListener('mousedown', function() {
    pointerIsDown = true;
  }, true);
  window.addEventListener('pointerup', function() {
    pointerIsDown = false;
  }, true);
  window.addEventListener('mouseup', function() {
    pointerIsDown = false;
  }, true);
  window.addEventListener('blur', function() {
    pointerIsDown = false;
  });

  listen('neko_radar_tick', function(event) {
    var payload = event && event.payload;
    if (!Array.isArray(payload) || payload.length < 2) return;
    handleRadarPoint(Number(payload[0]), Number(payload[1]));
  }).catch(function(error) {
    console.error('监听 Tauri 鼠标雷达失败:', error);
  });

  // Kept intentionally small so the bridge can be verified in jsdom without
  // duplicating the production hit-test rules in a separate test-only module.
  window.__NEKO_TAURI_CLICK_THROUGH_TEST__ = {
    handleRadarPoint: handleRadarPoint,
    isInteractiveDomPoint: isInteractiveDomPoint,
    isPointOverModel: isPointOverModel
  };
})();
