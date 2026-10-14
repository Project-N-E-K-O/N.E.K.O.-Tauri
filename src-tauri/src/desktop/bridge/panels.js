(function installPanelSurfaces() {
  'use strict';
  const host = window.__NEKO_DESKTOP__;
  if (!host) return;
  const role = host.profile.role;
  const dispatch = (name, detail) => window.dispatchEvent(new CustomEvent(name, { detail }));
  if (role === 'subtitle' || role === 'subtitle-settings') {
    host.on('transcript', message => {
      window.__nekoSubtitleLatestTranscript = message.data;
      dispatch('neko-ws-transcript', message.data);
    });
    host.on('subtitle-state', message => {
      window.__nekoSubtitleLatestState = message.data;
      dispatch('neko-subtitle-state-sync', message.data);
    });
  }
  if (role === 'subtitle-settings') {
    host.on('subtitle-settings-state', message => {
      window.__nekoSubtitleLatestState = message.data;
      dispatch('neko-subtitle-state-sync', message.data);
    });
    function openSettings() {
      const style = document.createElement('style');
      style.textContent = '#subtitle-scroll,#subtitle-panel-controls{display:none!important}#subtitle-display{position:relative!important;inset:auto!important;width:100%!important;max-width:none!important;height:auto!important;transform:none!important;margin:0!important}#subtitle-settings-panel{position:static!important;display:block!important;max-height:none!important;margin:0!important}';
      document.head.appendChild(style);
      queueMicrotask(() => document.getElementById('subtitle-settings-btn')?.click());
    }
    if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', openSettings, { once: true });
    else openSettings();
  }
  if (role === 'agent-hud') {
    host.on('agent-update', message => dispatch('neko-ws-agent-update', message.data));
  }
})();
