# Synchro Folder Sync — experimental preview

Synchro supports opt-in transfers in both directions. No mirroring, background
deletions or recursive folder access. The relay is an intermediary, not a
permanent archive (transfer expiration defaults to 72 hours). Android → Windows
backup encrypts file contents with AES-256-GCM before HTTPS upload. The server
still sees metadata (filenames and sizes). Windows → Android and ordinary LAN
file transfer are still NOT end-to-end encrypted.

## PC → Android: send a chosen folder

1. Configure Synchro Windows to send via Wi-Fi/Tailscale or HTTPS Internet
   Mode with the correct recipient Android device UUID.
2. In Windows Settings select a local folder (for example D:\SynchroShare).
3. Enable folder watching and optionally ALL file types, not just APK.
   Save settings; auto-send must also be enabled to upload automatically.
4. Preexisting files are intentionally not uploaded by simply enabling the
   watcher. Use Add existing files to queue, review the queued items, and Send.
5. Watcher handles direct files only: no subfolders, symlinks, dotfiles or
   temporary .part/.tmp files. Originals are not deleted or moved.
6. Internet Mode marks files as stored on Relay until Android downloads and
   acknowledges them. Android currently pulls Inbox manually.

## Android → Windows: selective backup of photos/videos

1. Deploy Synchro Relay v0.5.1 or later on a trusted HTTPS server.
   Register both Android and Windows as separate devices in admin UI.
2. On Android, use the system folder picker under ANDROID → PC · BACKUP.
   Select up to five specific folders, such as DCIM/Camera or Movies.
   Only direct files in those directories are scanned; no subfolders.
3. In Synchro Windows → Settings → Android → PC Backup, click Generate key.
   Copy the 64-character secret into a password manager, then paste it into
   the E2EE key field in Android → Backup. Android wraps it with a hardware/
   OS-backed Android Keystore key; Windows holds it in memory for this session
   only. NEVER lose the recovery key. Enter Relay HTTPS origin, Android's device
   bearer token and recipient Windows UUID. The server sees names and sizes,
   but the file content is encrypted BEFORE upload.
4. Start a one-time backup or opt in to Wi-Fi/unmetered WorkManager scheduling
   (nominally every 15 minutes). Android may defer tasks.
5. In Synchro Windows Settings, select an existing trusted local backup folder.
   Click Receive files or enable polling every ~60 seconds while the desktop
   application remains open. The PC must download before relay expiration.
6. Enter the same recovery key in the Windows Backup panel each time you
   restart Synchro. Windows streams encrypted downloads, validates SHA-256
   and AES-GCM authentication, writes decrypted files to a temporary file,
   publishes without overwriting, then ACKs Relay. Android checks the ACK
   on a later run.

## Security, retention and limitations

- This is a preview, not a guaranteed archival solution. Default retention:
  72h; server storage quota applies. Keep the PC running to receive in time.
- Windows decrypts only the SYNBKP01 backup envelope (1 MiB authenticated
  records, per-file random salt and nonce prefix), NOT the browser SYNCHRO1
  format. Losing the 64-hex master backup key permanently prevents recovery.
- Native Windows → Android sender is still HTTPS only (no E2EE).
- Files are NOT encrypted at rest on Windows after decryption. Protect the
  selected backup directory with full-disk encryption and access controls.
- Folder paths are NOT preserved: flat filenames have a source-folder prefix.
- Android reads at most 200 direct file entries per selected folder per run (round-robin pagination),
  up to five selected folders, file size up to 2GiB (excluding empty files).
- Per-file sync history is held in app-private settings; it distinguishes
  stored-on-relay from Windows delivery ACK, and retries expired uploads.
- Windows watches a chosen directory while running, not as an OS service.
  Windows folder queue is capped at 500 items, no recursive subdirectories.
- No remote deletion or modification of Android originals, no silent access
  to other Android folders. Removing permissions disables access to a folder.
- Use a private HTTPS server and local disk encryption. Do not use highly
  sensitive personal photos before native E2EE and recovery tests.

## Test checklist before relying on this

1. Verify both parts compile in GitHub Actions, including the shared AES-GCM
   golden vector tests, and then build the Windows EXE/Android APK locally.
2. Select Android test folders with harmless JPG, MP4 and nested folders.
   Confirm direct files upload and nested files do not.
3. Have Windows poll the Relay and confirm SHA-256 before ACK.
4. Interrupt a multi-chunk upload, resume, check hashes, avoid duplicates.
5. Restart Android and PC between upload and ACK; verify delivery state.
6. Test invalid tokens, revoked recipient UUID, disk full and connection loss.
7. Test Android background scheduling with battery optimization and Wi-Fi loss.
8. Test duplicate filenames, oversized videos, symlinks and 72h expiration.
9. Validate UI at 850x620 and DPI150. No false backup-success status.

Run Windows builds from PowerShell:

    cd "M:\projects\synchro"
    git pull --ff-only
    .\scripts\build-all.ps1 -NoPull

Server must be compiled and deployed on Linux; do not upload the Windows EXE.
