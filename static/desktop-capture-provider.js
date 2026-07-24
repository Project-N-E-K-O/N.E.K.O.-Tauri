/**
 * Unified desktop capture boundary.
 *
 * Electron installs window.electronDesktopCapturer. Tauri injects
 * window.tauriDesktopCapturer after the local backend page finishes loading.
 * Consumers must resolve the provider at call time because the Tauri bridge is
 * intentionally installed after navigation to http://127.0.0.1:48911.
 */
(function installDesktopCaptureProviderResolver() {
    'use strict';

    window.getDesktopCaptureProvider = function () {
        if (window.tauriDesktopCapturer) return window.tauriDesktopCapturer;
        if (window.electronDesktopCapturer) return window.electronDesktopCapturer;
        return null;
    };
})();
