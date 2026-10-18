# Synchro v0.1 protocol

Transport: HTTP/1.1 POST /upload to Android TCP port 8765.

Required headers:
- X-Synchro-Token: 32-character random hex secret shown in Android app
- X-Synchro-Name: percent-encoded UTF-8 basename
- X-Synchro-Sha256: 64-character lowercase SHA-256 digest
- Content-Type: application/octet-stream
- Content-Length: fixed payload length, 1 byte to 2 GiB

Payload is a raw file stream, not an archive. Android authenticates first, checks length and free space, streams into MediaStore Downloads with IS_PENDING=1, verifies SHA-256, then makes the file visible by clearing IS_PENDING. Incomplete files are deleted. HTTP 201 indicates success; 400/401/413/507 indicate failure; each connection closes after a response.

This first version uses token-authenticated plain HTTP on a trusted LAN. For remote use, pass traffic inside a private encrypted Tailscale connection; do not forward Android port 8765 to the public internet. Android always controls APK installation confirmation.
