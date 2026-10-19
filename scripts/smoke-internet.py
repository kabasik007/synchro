#!/usr/bin/env python3
"""Staging-only Synchro 0.5.1 integrity/recovery smoke test. Requires HTTPS and admin token."""
import hashlib
import json
import os
import secrets
import sys
import time
import urllib.error
import urllib.request
from urllib.parse import urlparse

BASE = os.environ.get("SYNCHRO_TEST_URL", "").rstrip("/")
ADMIN = os.environ.get("SYNCHRO_TEST_ADMIN_TOKEN", "")
if not BASE or urlparse(BASE).scheme != "https" or len(ADMIN) < 32:
    sys.exit("Set SYNCHRO_TEST_URL=https://... and SYNCHRO_TEST_ADMIN_TOKEN privately.")

def request(route, token, method="GET", data=None, payload=None):
    body = json.dumps(payload).encode() if payload is not None else data
    headers = {"Authorization": "Bearer " + token, "Cache-Control": "no-store"}
    if payload is not None:
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(BASE + route, headers=headers, data=body, method=method)
    with urllib.request.urlopen(req, timeout=90) as resp:
        out = resp.read()
        if route.endswith("/download"):
            return resp.status, out
        return resp.status, json.loads(out.decode()) if out else None

devices = []
try:
    for label in ("Synchro smoke sender", "Synchro smoke receiver"):
        _, d = request("/api/v1/admin/devices", ADMIN, "POST", payload={"name": label})
        devices.append(d)
    sender, receiver = devices
    denied = False
    try:
        request("/api/v1/me", "0" * 64)
    except urllib.error.HTTPError as e:
        denied = e.code == 401
    assert denied, "Invalid bearer token must fail"
    blob = secrets.token_bytes(8 * 1024 * 1024 + 91)
    digest = hashlib.sha256(blob).hexdigest()
    _, created = request("/api/v1/transfers", sender["token"], "POST", payload={
        "to_device_id": receiver["id"], "filename": "smoke.bin",
        "size_bytes": len(blob), "sha256": digest, "encrypted": False})
    id = created["transfer"]["id"]
    path = "/api/v1/transfers/" + id
    assert created["chunk_count"] == 2
    request(path + "/chunks/0", sender["token"], "PUT", data=blob[:8 * 1024 * 1024])
    _, checkpoint = request(path + "/chunks", sender["token"])
    assert checkpoint["uploaded_indices"] == [0], checkpoint
    assert checkpoint["uploaded_chunks"][0]["index"] == 0
    assert checkpoint["uploaded_chunks"][0]["sha256"] == hashlib.sha256(blob[:8 * 1024 * 1024]).hexdigest()
    # Simulate corruption: explicitly replace one accepted block, then recover.
    request(path + "/chunks/0", sender["token"], "DELETE")
    _, checkpoint = request(path + "/chunks", sender["token"])
    assert checkpoint["uploaded_indices"] == [], checkpoint
    request(path + "/chunks/0", sender["token"], "PUT", data=b"X" * (8 * 1024 * 1024))
    _, checkpoint = request(path + "/chunks", sender["token"])
    assert checkpoint["uploaded_chunks"][0]["sha256"] != hashlib.sha256(blob[:8 * 1024 * 1024]).hexdigest()
    request(path + "/chunks/0", sender["token"], "DELETE")
    request(path + "/chunks/0", sender["token"], "PUT", data=blob[:8 * 1024 * 1024])
    request(path + "/chunks/1", sender["token"], "PUT", data=blob[8 * 1024 * 1024:])
    status, _ = request(path + "/finish", sender["token"], "POST", data=b"")
    assert status in (200, 202)
    for _ in range(90):
        _, state = request(path, sender["token"])
        if state["status"] == "ready":
            break
        assert state["status"] == "finalizing", state
        time.sleep(1)
    else:
        raise AssertionError("Finalization timeout")
    _, inbox = request("/api/v1/inbox", receiver["token"])
    assert any(t["id"] == id for t in inbox)
    _, downloaded = request(path + "/download", receiver["token"])
    assert hashlib.sha256(downloaded).hexdigest() == digest
    request(path + "/ack", receiver["token"], "POST", payload={"sha256": digest})
    print("PASS: auth rejection, chunk SHA-256/replacement, async finish, inbox, verified ACK")
finally:
    for device in devices:
        try:
            request("/api/v1/admin/devices/" + device["id"] + "/revoke", ADMIN, "POST", data=b"")
        except Exception as error:
            print("Warning: could not revoke test device:", error, file=sys.stderr)
