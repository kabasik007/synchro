# Synchro v0.3.1 — Stability Update

**Baseline:** [source audit of v0.3](AUDIT-0.3-2026-10-19.md).  
**State:** fixes committed to GitHub, **local Windows/Android builds and end-to-end transfer not yet executed in this environment**. Treat as a release candidate until tests pass on the actual devices.

## Changes applied

### Windows sender (Rust)

- **S-01 / S-10:** After a successful batch, auto-send continues with new files added while it was busy. On failure, pending files remain for retry; no infinite automatic retry loop.
- **S-02:** Pending queue is loaded from and saved to `Synchro/pending-queue.json` under the user configuration directory. A `.bak` file is kept as a recovery fallback. Files still need to exist at their original paths.
- **S-12:** Progress events carry a full `PathBuf`, rather than just the `app-debug.apk` basename, so two builds with the same filename are distinct.
- Hashing/uploading checks for modifications to the build output. If Android Studio overwrites an APK during transfer, the new build is left queued for another attempt.
- **S-07:** The automatic folder watcher now accepts only `.apk` (case-insensitive), rejects symlink files, and waits at least eight seconds after the last file modification plus stable observations. Manual file selection and drag/drop still accept other file types.
- **S-11:** A failed transfer invalidates cached connection status and schedules a new health check.
- Watcher configuration is applied after saving settings rather than whenever a checkbox is toggled.

### Android receiver (Kotlin)

- **S-03:** Three bounded client slots allow authenticated health checks while an upload runs; a separate single-upload semaphore protects file writes. Excess clients return HTTP 503 rather than being allowed unlimited workers.
- Absolute 12-second request-header and 30-minute upload deadlines; interrupted/invalid transfers get explicit error responses when the socket is still open.
- **S-05:** Disk capacity is checked against shared Downloads, not the app-private `filesDir`. ENOSPC is separately handled (507).
- **S-06:** On receiver startup the app attempts to reconcile/delete its own orphaned pending MediaStore items from `Download/Synchro/Debug/`. A killed process can still leave a pending item until the next start or platform expiration.
- **S-09:** Synchro only sends HTTP 201 after `IS_PENDING=0` has been updated successfully; otherwise the pending item is deleted where possible.
- **S-13:** The pending APK URI and filename survive activity recreation while returning from Android's "Install unknown apps" settings.
- **S-14:** Receiver startup and last transfer error are visible in the Android UI.
- **S-04 (partial):** Android 15+ dataSync timeout is explicitly reported, but the platform's six-hour background budget is **not bypassed**.

## Known limitations / not yet fixed

- **S-08:** Pairing token is still saved in a local Windows JSON configuration file. HTTP on a trusted LAN is authenticated but **not encrypted**. Protect the user account, never port-forward port 8765, and use Tailscale when outside a trusted LAN. Secure OS credential storage and cryptographic transport are future work.
- Stable mtime and file size are helpful but cannot *prove* a Gradle build has finished. Paused compilation for over eight seconds can still produce a premature watcher event; a Gradle post-build hook would be stronger.
- Android foreground-service limitations remain. Reopen Synchro and start receiving again if Android stops the service.
- No upload resume/cancel UI, no persisted transfer-history database, no automatic retry with exponential backoff.
- GitHub Actions CI is not added/enabled automatically. The project provides a local verification script so CI minutes are not spent without opt-in.
- Storage and protocol edge behavior on the user's exact Android devices must be tested before declaring stable.

## Verify after git pull

Run these on Windows with Rust, Android SDK and a compatible Gradle JDK configured:

```powershell
cd M:\projects\synchro
git pull --ff-only
.\scripts\verify-local.ps1 -Android
```

Alternatively run the parts separately:

```powershell
cd M:\projects\synchro\desktop
cargo test
cargo check
cargo run --release

cd M:\projects\synchro
.\scripts\build-android.ps1
```

### Manual regression test cases

1. Start an upload of A and drop B while A is busy. With auto-send enabled B must start when A completes.
2. Queue files, close and reopen Synchro Desktop. Pending entries must be restored.
3. Add `project-a/app-debug.apk` and `project-b/app-debug.apk`. Only the currently transmitting path shows progress.
4. Enable folder watch on a Gradle output folder. A new APK is queued after settling; `.txt` and `.tmp` files are ignored.
5. Interrupt Wi-Fi mid-transfer; the unacknowledged file remains queued and a partial Android item is cleaned or later reconciled.
6. Reboot/crash Android during an upload; restart receiver and verify stale pending items are cleaned.
7. Reconnect while another file transfers; `GET /health` must respond instead of waiting for the whole APK.
8. Fill destination storage until insufficient; Android must reject upload without HTTP 201.
9. On Android, open Install Unknown Apps permission page, rotate/recreate Activity, return and continue installing if permission granted.
10. For Emulator, run `scripts/connect-emulator.ps1` and use `http://127.0.0.1:8765`; for a phone use its local Wi-Fi or Tailscale address.
11. On Android 15+, confirm receiver timeout and restart behavior.

## Acceptance bar

Do not tag a production-ready GitHub Release until `cargo test`, `cargo check`, Android `assembleDebug`, and a real APK transfer-install cycle pass. No tests are claimed passed until executed.
