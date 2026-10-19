# Synchro v0.5.1 — Stability & UI update

**Source milestone; not a production-ready release until GitHub Actions and device tests pass.**

Changes:
- Protocol checkpoint now supplies SHA-256 for every committed 8 MiB chunk, alongside legacy indices.
- Sender-authorized DELETE /api/v1/transfers/{id}/chunks/{index} permits safe repair of a corrupted persisted chunk only during uploading.
- Rust Windows and browser senders compare their local SHA-256 against server checkpoints; if different, they delete/re-upload that chunk. The server always verifies the full file hash after assembly.
- Finalization runs with a 2-job semaphore without holding the global mutation gate during gigabyte-long assembly. The expired-file pruner explicitly excludes finalizing transfers and the short completion/publish phase retains its lock.
- Windows verifies selected Android device UUID exists and is not revoked before showing Ready. The UI distinguishes "stored on Relay" from direct delivery to Android, with truncated long filenames.
- Android wrong E2EE key errors are isolated per transfer. Current UX accepts **one** E2EE key per run; retry synchronization separately for different encrypted files.
- SQLite migration handles legacy transfers tables missing encrypted or delivered_at, PRAGMA user_version 4.
- GitHub Actions CI added for Rust server, Windows desktop, Android APK, and web JavaScript.

## Compatibility and upgrade
Upgrade server before updating web/Windows client to v0.5.1. Old servers do not provide uploaded_chunks SHA-256 metadata or DELETE chunk recovery.
Forward migration from v0.5.0: SQLite user_version=4. Before deployment, stop the service and make a verified backup of the entire private storage directory AND database. A SQLite-only snapshot cannot restore stored files.

Build on Windows:
    cd M:\projects\synchro
    git pull --ff-only
    .\scripts\build-all.ps1 -NoPull

On Linux/aaPanel, build server on the target platform, do not copy Windows server EXE.
Only run the staging acceptance script with a valid separately provisioned admin token:
    SYNCHRO_TEST_URL=https://example.org SYNCHRO_TEST_ADMIN_TOKEN=... python3 scripts/smoke-internet.py

## Not yet complete
- No universal native E2EE on Windows and no automatic background Android Internet downloads.
- Browser E2EE resumption is limited to its still-open tab and browser file-size limit.
- Server checkpoint hashing is O(total uploaded bytes) during resume and holds the current metadata/file mutation gate; optimize per-chunk hash persistence before heavy multi-user usage.
- Storage backups are local SQLite-only; encrypted offsite blob backup, monitoring and disaster recovery testing remain outstanding.
- Automated CI compiles and tests; it does **not** run a live VPS, a physical Android device, or an end-to-end security test.

To accept: inspect green GitHub Actions runs, test 8 MiB and 2 GiB transfers, wrong E2EE key behavior, transfer recovery, status labels at 850x620/1180x790/DPI150, then stage-rollout.
