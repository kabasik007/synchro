# Synchro v0.3.0 — Windows + Android

**Baseline:** [approved target mockup](design/synchro-target-2026-10.png).

This release focuses on real functional UI, not simulated connected devices or fake QR pairing. The Windows sender remains Rust/egui; the Android receiver remains dependency-free native Kotlin.

## Implemented

### Windows (Rust)

- Dark sidebar, transfer dashboard and pairing card, now with real device model/Android SDK/app version from authenticated `GET /health`.
- Continuous health recheck approximately every 20 seconds while idle.
- System multi-file picker, drag & drop and transmission progress within the pending file row.
- In-session list of up to 15 successfully sent files; files not confirmed by Android remain queued after errors.
- **Optional folder watch:** specify an output folder in **Налаштування**, enable watching and save. Existing files are baselined; newly created or modified files join the queue after being stable across two checks. Auto-send still follows the existing toggle.
- Local IP-only HTTP receiver validation: Wi-Fi/LAN, private IPv6, Tailscale 100.64.0.0/10 or `127.0.0.1` for ADB emulator forwarding. Public HTTP targets and DNS names are rejected to avoid transmitting pairing credentials to unintended servers.

### Android (Kotlin)

- Rebuilt light UI with rounded status, addresses, token, action and received-file panels; safe handling of status/navigation bar insets.
- Real Wi-Fi, emulator and Tailscale IP addresses, per-field clipboard actions, copy all pairing information.
- Real receiver controls and detailed bind errors. Active upload socket closes when receiver is stopped.
- Byte counters surfaced during active transfer; up to 50 received files with name, size and timestamp, plus **Install** for APK.
- Android's official installer is always used; install-from-this-source permission is requested explicitly.
- Branded launcher icon and notification icon.
- Authenticated health response includes device model, version and Android API, with no secret token in the response.

### Developer workflow

- `scripts/connect-emulator.ps1`: map a running Android Studio emulator's port 8765 to the Windows host.
- `scripts/build-android.ps1`: detect compatible JDK and verify the APK output.
- Unit tests for Rust URL safety, health parsing and folder watcher eligibility.

## Installation and test checklist

Run on Windows, from repository root:

```powershell
git pull --ff-only
cd desktop
cargo test
cargo check
cargo run --release
```

Build the Android debug APK (close the previous app before installing an incompatible debug signing key):

```powershell
cd M:\projects\synchro
.\scripts\build-android.ps1
```

For Android Studio's emulator, **start the Android Synchro receiver**, then on Windows:

```powershell
.\scripts\connect-emulator.ps1
```

In Windows Synchro enter `http://127.0.0.1:8765` and the 32-character token shown by Android. Select **Перевірити**: device details must come from the emulator, not mock data. Drop a small text file, then an APK, and verify that the files appear under `Download/Synchro/Debug`. Tapping **Install** should open the system APK installer and ask for confirmation.

On a physical phone connected to the same Wi-Fi, use its actual private address. For private remote connections, use the Tailscale 100.x address.

## Remaining work / limitations

- **Not yet compiled in the assistant environment** (Windows toolchain and Android SDK are unavailable here). All new Rust and Kotlin changes require the above on-device validation.
- The approved mockup contains a QR workflow, but QR pairing/scanning is **not implemented**. The UI intentionally presents manual pairing instead.
- Folder watching is non-recursive and keeps state only while Synchro Desktop runs.
- Desktop transfer history is in memory, not persisted.
- No pause/resume or transfer cancellation inside the Windows application; Android stop closes the active socket.
- Plain HTTP must remain inside a trusted LAN or an encrypted tunnel such as Tailscale; do not port-forward the receiver to the public Internet. The pairing token is stored in the local Windows settings file.
- Foreground service restrictions on recent Android versions can stop long-running receivers. Test on your real Android version and restart the service if needed.
- For multiple devices and richer build automation, future versions will add a proper device manager, a reviewed QR exchange and resumable uploads.

## Acceptance criteria

- Desktop and Android launch without Gradle/Rust compilation errors.
- Actual connection probe returns Android name, SDK and version.
- Drag-drop and optional stable folder watch do not send incomplete files.
- Android verifies SHA-256 and makes the result visible in Downloads.
- Failed uploads stay in the Windows queue for retry.
- Android APK installation always has explicit system confirmation.
- Visually compare each screen with the approved PNG in `docs/design`; keep adapting spacing/typography without replacing the reference.
