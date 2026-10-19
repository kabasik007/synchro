# Synchro Desktop UI — Reference fidelity update (v0.5)

## Source of truth

The approved reference is still docs/design/synchro-target-2026-10.png
and docs/design/README.md. Do not silently replace this reference with
Photoshop/Figma/other-app screenshots. This iteration uses standard
Rust/egui 0.37 and does NOT rewrite native transfer logic or adopt Tauri.

## Changes in desktop/src/design.rs

- Shared dark-navy color tokens, electric-blue accent, typography helpers,
  rounded connection status chips and a custom vector PC → network → phone illustration.
- Network status is always derived from the actual connection state:
  no fake "online" device, QR pairing, encryption or upload history.
- Mode label is derived from the configured address (HTTPS => Internet,
  private HTTP => LAN/Tailscale); it is NOT a security verification of TLS setup.

## Changes in desktop/src/ui.rs

- Reference-inspired branded sidebar, consistent navigation and current release label.
- A clearly grouped hero with network artwork and live status/transport indicators.
- Refined cards for recipient setup, file picker/drop area, transfer queue,
  progress, and action buttons.
- Responsive two-column content that collapses vertically as the window narrows.
- The onboarding panel describes either direct Wi-Fi/Tailscale or HTTPS relay
  according to the configured mode, instead of always assuming the phone LAN receiver.
- Current Windows native Internet uploader continues to send plaintext over HTTPS,
  **not E2EE**. An explicit warning remains visible for Internet Mode.

## Acceptance tests (must be run on Windows; source changes alone do not prove visuals)

1. Build with scripts/build-all.ps1 -NoPull or run:
   cd desktop && cargo test && cargo clippy --all-targets && cargo run --release
2. Compare against the approved 1672 × 941 PNG on a 1180 × 790 window.
   Check sidebar, blue accents, spacing, hero art, cards and primary send button.
3. Resize window down to the minimum 850 × 620 and check all controls remain
   reachable through the vertical scroll region; validate at Windows DPI 100, 150, 200%.
4. LAN: pair the actual Android receiver, verify offline/online status,
   drag a small APK, cancel/retry, and confirm Downloads + SHA-256.
5. Internet: configure HTTPS origin + valid device token + Android UUID,
   verify relay status, send a file, and check the remote inbox.
6. Use both light/dark Windows system modes, keyboard navigation, empty queue,
   active progress, malformed token, refused connection and very long file names.
7. Do not accept mock QR pairing, simulated transfer progress, or false
   claims that the Windows native Internet client offers E2EE.

## Future enhancements

- Visual review screenshots and controlled changes based on the same reference.
- Icons and font assets with explicit licenses; no hotlinked remote theme packs.
- Optional dockable workflow only if the product later gets multiple simultaneous
  inspectors/editors. Synchro is a file transfer utility, not a Photoshop canvas.
- Keyboard shortcuts and accessibility labels, more languages, and tests for text overflow.

**Status:** committed design source, **not** a claim that visuals or Rust builds
have passed on the user's Windows machine.
