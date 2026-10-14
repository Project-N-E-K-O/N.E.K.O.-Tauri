# Platform boundary

`lib.rs`, `backend_runtime.rs` and `radar.rs` contain shared application
orchestration. Native window behavior lives in exactly one implementation
selected by `platform/mod.rs` at compile time:

- `linux.rs`: GTK overlay integration. GNOME/KDE use X11 ShapeInput;
  niri uses a Wayland layer-shell carrier plus native `wl_surface` input
  regions. Linux builds require the native development package that provides
  `gtk-layer-shell-0.pc` (for example `libgtk-layer-shell-dev` on
  Debian/Ubuntu).
- `macos.rs`: AppKit window level, primary-screen geometry and mouse ignoring.
- `windows.rs`: Windows virtual-desktop sizing and native click-through.

Every platform module implements the same small contract: process setup,
application/window configuration, click-through, cursor sampling, navigation
handoff. The desktop module owns child/satellite window creation; the platform
module configures their native behavior without sharing Pet input state.

Only one front-end hit-test owner runs on a platform. Each platform selects its
injected `CLICK_THROUGH_BRIDGE`: Linux uses `linux_click_through.js` to submit
persistent model/UI rectangles to the native input-region implementation;
macOS and Windows retain `click_through.js` and whole-window mouse ignoring.
The shared upstream page does not need to implement a Tauri radar listener.
Linux samples layout twice per second and submits only changed rectangles,
collecting whole surfaces before controls and skipping already covered areas.
Each active document permits one IPC request at a time; completions from a
previous cached-page lifecycle cannot alter the resumed bridge's state.
Pointer interactions retain input until release. Page hide removes the timer
and input listeners; cached-page restore resumes exactly one bridge.
