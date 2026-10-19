# Synchro roadmap

## Delivered in source

- **v0.1:** Rust egui drag-and-drop sender, Kotlin Android HTTP receiver, token, SHA-256, MediaStore Downloads, and system APK installer.
- **v0.2:** Dark desktop UI, file queue, native picker, authenticated Android health checking.
- **v0.3:** Light Android card layout based on the [approved mockup](design/synchro-target-2026-10.png), device name and SDK health metadata, per-file progress, recent send list, non-recursive stable folder watcher, emulator ADB-forward helper, branded Android icon, safer service shutdown, private address validation.

**Validation gate:** v0.3 source requires successful cargo test/check and assembleDebug on Windows, emulator-to-Windows integration tests and a real-phone APK installation test. See [release checklist](RELEASE-0.3.md).

- **v0.3.1 (current source, verification pending):** persisted pending queue, queued auto-drain, safer APK watcher, bounded receiver clients, deadlines, Downloads capacity checks, pending MediaStore cleanup, and installer lifecycle handling. See [the stability fix log](RELEASE-0.3.1.md).

## Next milestones

1. **v0.3.x stabilization:** fix compile and device-only findings, reduce UI overflow at 100–200% DPI, manage foreground service lifecycle correctly across recent Android versions, add on-device test logs.
2. **v0.4:** approved QR exchange (real scan and confirmation), multiple Android profiles, Windows persistent encrypted credential store, persisting send history, clear transfer retry controls.
3. **v0.5:** USB / wireless debugging workflows to directly invoke `adb install -r` on authorized development devices, optional Gradle output watchers and signed APK metadata detection.
4. **v0.6:** resumable encrypted transfers, end-to-end pairing keys, optional aaPanel relay for remote operation (not a required server).

No release should invent a connected device, fake a transfer progress value, or silently install an APK.
