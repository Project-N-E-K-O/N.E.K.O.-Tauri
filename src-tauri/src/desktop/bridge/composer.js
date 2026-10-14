(function installComposerTransfer() {
  'use strict';
  const host = window.__NEKO_DESKTOP__;
  if (!host || !['compact-chat', 'full-chat'].includes(host.profile.role)) return;

  function available() {
    const buttons = window.appButtons;
    const dom = window.appState?.dom;
    return !!(buttons?.addScreenshotToList && buttons?.getPendingComposerAttachments &&
      buttons?.syncPendingComposerAttachments && buttons?.updateScreenshotCount && dom?.screenshotsList &&
      dom?.screenshotThumbnailContainer && dom?.screenshotCount);
  }
  function capture() {
    if (!available()) return null;
    const items = Array.from(window.appState.dom.screenshotsList.children);
    return window.appButtons.getPendingComposerAttachments().map(attachment => {
      const item = items.find(item => String(item.dataset.attachmentId || item.dataset.index) === attachment.id);
      return { ...attachment, source: item?.dataset.source || '',
        avatarPosition: item?.dataset.avatarPosition || null,
        title: item?.querySelector('.screenshot-thumbnail')?.title || '' };
    });
  }
  function restore(attachments) {
    if (!available()) return false;
    const next = Array.isArray(attachments) ? attachments : [];
    if (JSON.stringify(capture()) === JSON.stringify(next)) return true;
    const dom = window.appState.dom;
    const buttons = window.appButtons;
    dom.screenshotsList.replaceChildren();
    for (const attachment of next) {
      let position = null;
      try { position = JSON.parse(attachment.avatarPosition); } catch (_) {}
      // Use the same upstream entry point as imports/capture. This installs
      // the original send/remove/view behavior instead of a preview-only copy.
      const item = buttons.addScreenshotToList(attachment.url, position, {
        source: attachment.source, alt: attachment.alt, title: attachment.title
      });
      item.dataset.attachmentId = String(attachment.id);
    }
    buttons.updateScreenshotCount();
    dom.screenshotThumbnailContainer.classList.toggle('show', next.length > 0);
    buttons.syncPendingComposerAttachments();
    return true;
  }
  host.composerTransfer = { available, capture, restore };
})();
