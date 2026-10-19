> **Follow-up:** The stability fixes were implemented as [Synchro v0.3.1](RELEASE-0.3.1.md) after this audit. This document intentionally preserves the original v0.3 findings and line references; do not interpret them as unresolved in the updated source without checking the release notes.

# Synchro v0.3.0 — Bug, reliability and security audit

**Date:** 2026-10-19  
**Scope:** `kabasik007/synchro`, `main`, baseline commit `ede0e9197cb35c109a6de78109dcb6d1e19a3460`  
**Method:** static source review (Rust sender/UI/watcher, Kotlin receiver/UI, build scripts, protocol, manifest) plus relevant official Android and Gradle docs. **Not compiled or tested on Windows/Android in the audit environment.** No code changes made in this audit.

**Status (at audited v0.3 baseline):** MVP architecture is usable, but not release-ready. Findings below are either **confirmed by source code** (logic/architectural defects) or **needs runtime validation** (platform-specific behavior). They are not claims of reproduced crashes.

## P1 — high priority

### S-01: Auto-send gets stranded during another upload — CONFIRMED

- `desktop/src/main.rs:383-418` processes `WatchFile` and `FilesPicked` events by setting `new_files=true`, but calls `start()` only when `!busy`.
- `desktop/src/main.rs:365-369` finishes an upload without draining newly queued files.
- **Repro:** start sending a large APK; while `busy=true`, write a second APK to the watched folder. It is queued, but after the first transfer finishes the second does not automatically start.
- **Fix:** explicit pending/sending/confirmed/failed queue state machine. On completion, if auto-send is enabled, schedule the next item. Never silently discard pending work.

### S-02: Pending transfer queue lost on restart — CONFIRMED

- `desktop/src/main.rs:223-258`: queued `files` and `history` exist only in RAM. `load_settings()` / `save_settings()` persist pairing and toggles, not transfer state.
- **Repro:** add an APK to the queue, close Synchro, reopen. The entry is gone.
- **Fix:** persist queue safely (atomic JSON or SQLite), distinguish pending from confirmed, handle missing files upon recovery, offer explicit clear.

### S-03: One stalled HTTP client blocks all Android transfers — CONFIRMED

- `ReceiverService.kt:65-80` uses one executor thread and calls `handle(socket)` inline before the next `accept()`.
- `ReceiverService.kt:112-121,149-170` uses `soTimeout=60_000` per socket read, but has no total request-header deadline. A client can drip bytes or stall an upload, blocking all later connections.
- **Fix:** bounded separate accept/workers, concurrent-client cap (possibly one active upload plus separate authenticated health checks), absolute header/body deadlines, length/rate limits and teardown tests.

### S-04: Android 15+ receiver cannot run indefinitely — CONFIRMED PLATFORM LIMIT

- `AndroidManifest.xml:20-22` declares foreground service type `dataSync`, `build.gradle.kts:14` targets API 35, and `ReceiverService.kt:107-110` stops on timeout.
- Android 15+ allows `dataSync` foreground services **up to six hours total per 24 hours while backgrounded**. An always-on receiver will stop when the budget expires (by design, not a Kotlin crash).
- **Fix:** model availability explicitly; design supported restart/foreground flow rather than promising 24/7 server operation; evaluate alternative platform-appropriate architecture. Show why receiving stopped.
- **Official:** https://developer.android.com/develop/background-work/services/fgs/timeout

### S-05: Free-space test checks the wrong storage location — CONFIRMED

- `ReceiverService.kt:207-210` calls `StatFs(filesDir.absolutePath)`, which checks the app's private directory, but bytes are written via `MediaStore.Downloads.EXTERNAL_CONTENT_URI` (`ReceiverService.kt:226-241`).
- On devices where internal and shared/external storage differ, this can falsely reject a transfer or admit one that later runs out of disk space.
- **Fix:** query the actual destination volume/capacity; handle `ENOSPC` when writing, and ensure MediaStore partial-item cleanup.

### S-06: Pending MediaStore entries survive hard process termination — CONFIRMED CODE PATH, DEVICE VALIDATION REQUIRED

- `ReceiverService.kt:232-265` creates a pending entry, deletes it in `finally` on ordinary exceptions, and publishes it on success.
- If Android kills the process before `finally` executes, an orphan `IS_PENDING=1` item can remain hidden and consume storage until platform expiration/cleanup. Android's default expiration is **typically about seven days**, so do not claim permanent leakage.
- **Fix:** persist active upload URI/session ID, reconcile stale pending entries at next start, verify `resolver.update(...IS_PENDING=0)` affects exactly one row, report errors clearly.
- **Official:** https://developer.android.com/reference/android/provider/MediaStore.MediaColumns

### S-07: Folder watcher can enqueue a paused/incomplete build or unrelated file — CONFIRMED DESIGN LIMIT

- `desktop/src/watch.rs:19-38,60-73` accepts virtually any regular file. It only checks identical size+mtime on consecutive scans and mtime older than 3s.
- A build tool may pause writing longer than 3s, then resume; a file may be queued/sent too early, causing checksum mismatch or needless bandwidth.
- The watcher includes other files (possibly sensitive) from the selected directory; a filesystem symlink can point outside the watched folder.
- **Fix:** opt-in file extensions (at least `.apk` by default); reject symlinks; canonical-root check; longer debounce with exclusive-access where possible; enqueue only after build success via a dedicated Gradle hook.

## P2 — medium priority

### S-08: Secrets are stored in plaintext and LAN transfers lack transport encryption — CONFIRMED

- `desktop/src/main.rs:40-57` saves `token` as readable JSON in the user config directory. `AndroidManifest.xml:13` allows cleartext; `desktop/src/main.rs:59-88` uses HTTP private-address allowlist.
- A local account/process that can read settings, or a malicious actor on an untrusted LAN, can obtain or misuse the token. The private-IP restriction is not encryption.
- **Fix:** Windows Credential Manager/DPAPI; Android Keystore/Encrypted storage or an appropriately scoped solution; encrypted/authenticated transport. Until then: trusted LAN or encrypted Tailscale only, never port-forward.
- No evidence of remote code execution found; the threat is unauthorized file injection / credential compromise if the token leaks.

### S-09: False success possible if MediaStore publication fails — CONFIRMED CODE PATH

- `ReceiverService.kt:260-262` ignores the integer result of `contentResolver.update(uri, ready, ...)` and still sets `ok=true`, then the server sends HTTP 201.
- **Fix:** require `update()==1` and verify the item is published before returning success; differentiate 4xx client errors, 5xx receiver errors and insufficient storage. Preserve source file on Windows until acknowledged.

### S-10: Watcher/files arriving during busy state have no durable retry — CONFIRMED

- Consequence of S-01 and S-02. Auto-watched changes are appended into the in-memory queue during an upload and lost if Synchro closes before someone manually sends them.
- **Fix:** model queue as durable jobs rather than a bare `Vec<PathBuf>`, with retries and terminal states.

### S-11: Health state may remain stale during a long transfer — CONFIRMED

- `desktop/src/main.rs:420-426` skips periodic checks while `busy`, so an earlier Online status may be shown even after receiver connectivity is lost, until upload fails.
- **Fix:** separate heartbeat from upload worker, update state on transport errors and when settings change. Do not claim connected merely from cached check.

### S-12: Filename matching for per-file progress is ambiguous — CONFIRMED

- `desktop/src/main.rs:357-360` stores only a basename; `desktop/src/ui.rs:199-212` matches rows by filename. Two different `app-debug.apk` paths can show progress simultaneously.
- **Fix:** use unique job IDs or full canonical paths in progress events and queue rows.

### S-13: APK install permission flow loses context on lifecycle recreation — CONFIRMED

- `MainActivity.kt:318-348` keeps `pendingInstall` only in an Activity field. Rotation/process recreation while viewing the "Install unknown apps" settings page resets it.
- **Fix:** `SavedStateHandle`/saved-instance-state and an explicit stable install action when returning, or retain URI in ViewModel/persistence. Test Android 12–16.

### S-14: Transfer errors and diagnostics are not reliably observable — CONFIRMED

- `ReceiverService.kt:75-88` swallows per-connection exceptions and records only fatal server errors.
- `MainActivity.kt:256-284` polls content resolver on the main thread every two seconds; no transfer session history, error codes or structured logs.
- **Fix:** ring-buffer diagnostic log without tokens, receiver notification progress, background database queries, error categories, opt-in logs export.

### S-15: No automated Android/Windows CI or integration evidence — CONFIRMED

- Repository at baseline has no `.github/workflows/` and the baseline commit has no reported status checks.
- Rust tests cover URL validation and watcher eligibility only. Android has no automated tests; neither side has an end-to-end test exercising an actual upload and APK installer flow.
- **Fix:** GitHub Actions on PR/push for cargo fmt/test/clippy; Gradle lint/test/assembleDebug with pinned JDK/Gradle; emulator integration pipeline when runtime budget permits; malformed headers and interrupted transfers tests.
- Compilation of **v0.3** has not been performed by this audit. Earlier v0.2 successful Windows build does not validate v0.3.

## P3 — polish / intentional gaps

- `desktop/src/ui.rs`: target design still differs from the approved PNG, QR flow intentionally not implemented.
- Android interface labels can misclassify emulator `wlan*` addresses as Wi-Fi; a user may mistakenly attempt direct 10.0.2.x access instead of ADB forwarding.
- Multiple files with the same basename can accumulate in Downloads; add duplicate/version policy rather than relying on automatic filename suffixes.
- Auto-send setting updates can activate watcher immediately in UI, before user presses **Save**, because `sync_watcher()` runs each frame. Make applying settings explicit.
- `scripts/build-android.ps1` advertises JDK 17–23, while release documentation should clearly differentiate Gradle-runtime compatibility from the Android Gradle Plugin's JDK requirement.

## Recommended implementation order

1. **Reliability release v0.3.1:** S-01, S-02, S-05, S-09, S-12; persistent queue, event-driven drain, accurate disk check, publication acknowledgement and unambiguous progress.
2. **Receiver hardening v0.3.2:** S-03, S-06, S-07, S-14; time bounds, orphan cleanup, safer watch policy, diagnostic logging.
3. **Lifecycle & security v0.3.3:** S-04, S-08, S-11, S-13; compatible Android foreground behavior, credentials/transport, health state and installer lifecycle.
4. **Gate to v0.4:** S-15 plus device matrix and approved design fidelity; QR and new features only after stabilization.

## Minimum regression matrix

| Case | Expected |
| --- | --- |
| A sends while B enters watched folder | B reliably starts after A if auto-send is enabled |
| App restarts with unsent queue | Pending files are restored |
| Cable/Wi-Fi disconnects halfway | Partial upload removed or reconciled; file remains queued |
| Process killed mid-2GiB upload | No unbounded hidden storage accumulation |
| Wrong token / stalled client | Quick rejection; other users' health checks and sessions remain responsive |
| Insufficient destination disk | Accurate failure and no false HTTP 201 |
| API 29, 33, 35, 37 emulator/phone | Receiver start/stop and installer request behave as expected |
| Two files named app-debug.apk | Per-file progress belongs only to the correct job |
| Windows 100–200% DPI | No clipped content or text |
| Code push / PR | Reproducible CI passes before merge |

**Source locations:** https://github.com/kabasik007/synchro/tree/main  
**Platform docs:** https://developer.android.com/develop/background-work/services/fgs/timeout | https://developer.android.com/reference/android/provider/MediaStore.MediaColumns | https://developer.android.com/training/data-storage/shared/media | https://docs.gradle.org/current/userguide/compatibility.html

**Audit policy:** This document describes findings; it does **not** claim that fixes have been applied, that v0.3 builds, or that failures have been reproduced on a device.
