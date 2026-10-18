# Synchro v0.3 protocol (compatible with v0.1 uploads)

Transport: HTTP/1.1 POST /upload to Android TCP port 8765.

Authenticated health probe: `GET /health HTTP/1.1` with `X-Synchro-Token`. A matching token and running receiver returns HTTP 200 with a JSON body containing `message`, `device`, `sdk`, `appVersion`, and `protocol`. A missing/incorrect token returns 401. Device name and connection status in the PC must come from this real response. Older Android v0.1 clients lack this route and must be rebuilt.

Required headers:
- X-Synchro-Token: 32-character random hex secret shown in Android app
- X-Synchro-Name: percent-encoded UTF-8 basename
- X-Synchro-Sha256: 64-character lowercase SHA-256 digest
- Content-Type: application/octet-stream
- Content-Length: fixed payload length, 1 byte to 2 GiB

Payload is a raw file stream, not an archive. Android authenticates first, checks length and free space, streams into MediaStore Downloads with IS_PENDING=1, verifies SHA-256, then makes the file visible by clearing IS_PENDING. Incomplete files are deleted. HTTP 201 indicates success; 400/401/413/507 indicate failure; each connection closes after a response.

This first version uses token-authenticated plain HTTP on a trusted LAN. For remote use, pass traffic inside a private encrypted Tailscale connection; do not forward Android port 8765 to the public internet. Android always controls APK installation confirmation.

## Transfer behavior and safety

- The Android receiver handles one transfer at a time. The socket times out after 60 seconds without incoming data.
- Android exposes `bytesReceived` and `bytesTotal` to its local UI while a transfer is active; these values are not network protocol fields.
- A failed checksum or interrupted upload removes the pending MediaStore entry. Windows retains any files that Android has not confirmed with HTTP 201.
- In v0.3 Windows only connects to literal private LAN/loopback IPs, Tailscale's 100.64.0.0/10 IP range, or private IPv6. Public HTTP URLs and DNS names are rejected to reduce accidental exposure of the pairing secret.
- For Android emulators, use `adb forward tcp:8765 tcp:8765` and connect Windows to `http://127.0.0.1:8765`.
- The protocol is plaintext HTTP on the transport. For untrusted networks use private encrypted Tailscale instead of internet port forwarding. A future protocol should use mutual cryptographic pairing and encrypted transport.
