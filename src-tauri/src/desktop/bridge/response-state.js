(function installResponseStateTransfer() {
  'use strict';
  const host = window.__NEKO_DESKTOP__;
  if (!host || !['compact-chat', 'full-chat'].includes(host.profile.role)) return;
  function available() {
    return !!(window.appChat?.captureDesktopState && window.appChat?.restoreDesktopState &&
      window.reactChatWindowHost?.captureComposerTransferState);
  }
  host.responseTransfer = {
    available,
    capture: () => available() ? window.appChat.captureDesktopState() : null,
    restore: (state, draft, restoreResponse) => window.appChat.restoreDesktopState(state, { draft, restoreResponse })
  };
})();
