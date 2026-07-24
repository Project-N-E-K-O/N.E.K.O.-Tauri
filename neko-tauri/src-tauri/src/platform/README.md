# Platform boundary

`lib.rs`, `backend_runtime.rs` and `radar.rs` contain shared application
orchestration. Native window behavior lives in exactly one implementation
selected by `platform/mod.rs` at compile time:

- `linux.rs`: GTK/X11 overlay and ShapeInput regions.
- `macos.rs`: AppKit window level, primary-screen geometry and mouse ignoring.
- `windows.rs`: Windows virtual-desktop sizing and native click-through.

Every platform module implements the same small contract: process setup,
application/window configuration, click-through, cursor sampling, navigation
handoff and settings-window creation.

Only one front-end hit-test owner may run on a platform. Linux uses the detailed
`static/live2d-core.js` payload required by XShape. macOS and Windows use the
injected `click_through.js` bridge.
