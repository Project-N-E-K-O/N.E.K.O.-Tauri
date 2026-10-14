# Native multi-window implementation

## Scope and acceptance

The Tauri host must implement the Electron desktop window experience while
keeping conversation, avatar rendering and feature UI in N.E.K.O. This work is
complete only after the following behavior is implemented and verified:

1. Pet is the only owner of the character WebSocket, audio playback and model.
   Compact/full chat use a WebSocket proxy. Unrelated sockets remain native.
   Reconnect, character change and Pet reload invalidate old generations.
2. Compact and full chat are independent native surfaces, reuse the upstream
   `/chat` and `/chat_full` pages, and preserve drafts/history when switching.
   Minimize/restore retains the last real surface and the ball's moved anchor.
3. Subtitle and Agent HUD use their upstream pages in independent windows.
   Transcript/settings/task updates, show/hide, drag, resize and input policy
   work through the host. Subtitle remains non-focus-stealing by default.
4. Same-origin feature pages open in managed native child windows. Named opens
   reuse/focus a window; close and return-to-parent behave coherently. External
   URLs open in the system browser.
5. Window bounds are stored as logical coordinates, restored onto available
   work areas, and clamped after display changes. Temporary/minimized bounds
   do not overwrite expanded bounds. Full and compact keep separate geometry.
6. Bridge installation happens before upstream scripts on every managed page.
   Reload and cached-page restoration have symmetric cleanup. Native commands
   derive the sender's identity from Tauri, not renderer-supplied window labels.
7. Windows, macOS, Linux X11/XWayland and niri each have explicit window/input
   adapters. Existing Pet overlay behavior is preserved. Child windows never
   reuse Pet-only global input state or layer-shell configuration.
8. Integration tests exercise the actual upstream-facing bridge interfaces and
   native routing/state transitions. Native smoke checks cover creation,
   readiness, interaction, window changes, persistence and teardown. Platform
   behavior without runtime evidence is reported as unverified.

Tray, autostart, application updating, cloud account management and backend
ownership redesign are separate modules, outside the window implementation.
Their existing upstream UI must not be presented as available host capabilities.

## Electron reference

Baseline: N.E.K.O.-PC `3e2350371d7d51baed4a67489656c6ec48abc8d9` and
N.E.K.O `35902fbb2a6cfdcd7e5bae1790c2acb29342b885` (2026-10-14 inspection).

Relevant Electron modules: `window-manager.js`, `ipc-router.js`,
`preload/entries/{pet,compact-chat,full-chat,subtitle,agent-hud,child-window}.js`,
and the window/WebSocket/subtitle bridges under `preload/bridges/`.

## Modules and ownership

- `desktop/`: native window roles, logical geometry, persistent layout,
  lifecycle coordination and sender-checked message routing.
- `desktop/bridge/`: document-start transport and upstream interface adapters.
  Each surface installs only the interfaces it actually implements.
- `platform/`: native window placement, focus, input regions and compositor
  behavior. Platform variation stays behind this module's interface.
- N.E.K.O remains the owner of page assets, React chat state, subtitles, HUD,
  avatar rendering and backend protocols. No upstream source snapshot is added.

Native role identity, desired visibility and actual document readiness are
separate state. A finished page must not reveal a surface that was hidden while
loading. Chat surface selection is independent from temporary minimization.
Messages are ordered and bounded; sticky state is replayed after a receiver
registers, and old document/session generations cannot update current state.

## Verification record

Record the exact upstream revisions, commands, covered requirements and any
unverified platform behavior here as implementation progresses. Test success
alone does not establish desktop behavior on an untested platform.

### Implementation in progress — 2026-10-14

The native role registry, document identity, visibility intent, logical layout,
message router, document-start bridge, Pet/Chat socket ownership, chat/panel/child
interfaces and local ball page are implemented. The work is not complete.

Evidence so far:

- Rust geometry/router/document/visibility checks: 9 passed, including queued
  input from an obsolete Pet document epoch.
- Character WebSocket bridge: 5 passed, covering one real owner, proxy input,
  initial replay ordering, old socket/epoch isolation, and Pet-only audio/capture.
- Vite production build creates both startup and ball pages.
- A real Tauri/WebKit run under Xvfb + KWin created compact/full/subtitle/HUD/
  child windows and exercised switch, named reuse, parent-child messages,
  child close and application exit. The first run exposed default Tauri event
  broadcast semantics. Both native deliveries and renderer subscriptions now
  explicitly target a window. A fresh native run passed all seven fixture
  checks, including exactly one child recipient, with no page errors and a
  successful application exit. This fixture proves the native host seam only;
  the actual upstream UI and all platform-specific behaviors still need tests.

Outstanding acceptance work (retain the full scope above):

- Real upstream pages and compiled React UI integration: visual compact/full
  layout, accessible mode selection, dragging/resizing, drafts, attachments,
  history hydration and switching during a live stream.
- Native ball assets/appear/bounce, moved anchor, reload during restoration,
  and confirmed destination readiness before the restore control disappears.
- Complete subtitle settings, panel geometry/lock behavior, HUD controls and
  shared settings/theme/config synchronization.
- Child opener lifetime, named navigation/reparenting, window.open features,
  per-window permissions and native layering/focus.
- Symmetric cached-page restoration across every bridge, replay sequencing
  relative to chat snapshots, and bounded failure/timeout behavior.
- Native platform adapters, especially niri floating geometry/input, mixed-DPI
  monitor restoration, monitor topology changes and Windows/macOS verification.
- Full project checks, cross-platform CI/build checks, Standards + Spec review,
  final documentation and commit on `codex/native-multi-window`.

### Upstream UI integration progress

The pinned upstream React bundle was built in the external reference checkout.
`scripts/upstream-window-fixture.py` serves actual upstream chat/subtitle/HUD
templates and assets with isolated test configuration and one fixture character
WebSocket. No upstream source or user data is copied into this repository.

Native WebKit checks now establish that compact/full React surfaces mount,
the compact carrier uses the upstream published interaction geometry (its
outer shell intentionally has zero height), and history plus a non-empty draft
survive a compact-to-full handoff. This required preventing a newly mounted
mirror from publishing empty initialization snapshots, retaining draft text
when the compact composer unmounts, and using the shared renderer's public
rollback-draft props instead of relying solely on a DOM value assignment.

Settings/theme synchronization, a native chat-layout context menu, a dedicated
subtitle settings role, native drag gestures and cached-document lifecycle
cleanup have been added. These additions are not all accepted yet. Empty draft
replacement, attachments/live-frame replay, subtitle/HUD/native menu behavior,
ball recovery, feature-window navigation, platform adapters and the remaining
acceptance list above still need completion and verification. Latest native
fixture evidence is in `.artifacts/native-upstream-windows/latest-report.json`.

### Latest lifecycle and native geometry checks

`DesktopState` now owns restore transitions and visibility planning. Cached
documents register a new activation identity independently of their original
time origin; the previous activation's document remains invalid. The new Rust
and transport checks cover cached-page registration, old input rejection,
scoped listener cleanup/reinstallation, destination readiness, and keeping a
restore control available while its replacement loads. These checks do not
establish React restoration readiness; that remains acceptance work.

A fresh isolated Xvfb/KWin native run confirmed an actual 88 × 88 ball window.
An XTest mouse click hid the ball and restored the compact window at 488 × 102.
This verifies native carrier size and the click/restore path. Moved anchors,
reload during restoration, and full-surface restoration remain unverified.

The completed Standards/Spec source review confirmed the visibility ownership
refactor. Remaining integration work includes sendable composer attachments,
live response adapter state, blank feature-window/navigation contracts,
React-ready restore acknowledgement, upstream settings ownership, and moving
compositor placement policy fully behind the platform interface. These are
retained as unfinished requirements; passing host tests does not close them.

### Chat handoff and moved-anchor verification

The shared conversation transfer contract is now implemented in N.E.K.O on
`codex/desktop-chat-transfer`, now commit
`4969b5b3c29a1ad13cf8de114a233c9f4c76f773`, rebased onto upstream
`8a655ac` (the intervening changes did not alter these chat modules). Draft PR:
[N.E.K.O #3436](https://github.com/Project-N-E-K-O/N.E.K.O/pull/3436). The Tauri
provider requires that contract: `appChat.captureDesktopState` /
`restoreDesktopState` and the shared composer's committed-restore callback.
This dependency is not yet merged or present in the original upstream baseline.
The provider transports opaque runtime data; queue/ref ownership and pending
rollback state stay upstream. It no longer wraps React mounting, rebuilds
virtual references, or remounts the composer to clear a draft.

Readiness now belongs to a specific snapshot sequence. A previously mounted
sibling is invalidated for each new handoff; an obsolete acknowledgement cannot
reveal it. Socket frames wait while the shared interface restores and commits
the composer. Cached-document generations invalidate obsolete completions.

The actual upstream browser probe passed compact/full rendering, non-empty and
empty draft replacement, restored attachment removal and sending with metadata,
same-bubble merge-stream continuation, and compact Enter submission followed by
`RESPONSE_TOO_LONG` rollback in full. Shared React checks cover unchanged empty
and unchanged non-empty drafts as well as key deduplication and ordinary
rollback on both surfaces. All 287 App tests passed; the shared bundle builds.

The fresh isolated native probe passed five checks in Tauri/WebKit with KWin:

1. Draft, attachment, history and live merge response survived compact → full.
2. Both chat windows used exactly one real character WebSocket owned by Pet.
3. The restored image was sent with a fresh request ID, exact payload, source
   and avatar-position metadata; historical fixture frames cannot satisfy it.
4. An actual 88 × 88 ball moved to `(250, 800)` restored the selected full window
   at `(250, 248)` with size `640 × 640`.
5. Native exit succeeded and the fixture observed character-socket teardown.

Evidence: `.artifacts/upstream-window-probe/report.json` and
`.artifacts/native-transfer-probe/report.json` (`passed: true`). Browser fixture
control is disabled during browser probes so it cannot consume native commands.
The fixture bounds its trace history and reports live connection counts plus
document timestamps. The native probe owns an isolated process/X11 display and
returns nonzero on failure.

Latest checks: `npm run check` (36 tests, typecheck/build), `cargo test` (29 tests),
`cargo clippy --all-targets -- -D warnings`, and formatting/diff checks passed.
Both source-review axes found no remaining issue in this focused handoff delta.
This does not close the full acceptance list: overlapping restores, reload
during restoration, realistic/structured stream variants, complete panels,
feature-window contracts, settings ownership and platform behavior remain open.

To reproduce, serve a built N.E.K.O checkout containing the transfer contract
with `scripts/upstream-window-fixture.py --upstream /path/to/N.E.K.O`, run Vite and
build the Tauri executable. Then run `scripts/upstream-window-probe.py --browser
/path/to/chromium` and `scripts/native-upstream-probe.py --binary
/path/to/neko-tauri`. Python dependencies are aiohttp, Jinja2, Playwright and
Python Xlib; the native Linux probe also needs Xvfb and KWin. Run against the
isolated fixture, whose health identity must be verified before starting it.
