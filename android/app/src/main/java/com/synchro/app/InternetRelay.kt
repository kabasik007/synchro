package com.synchro.app

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.provider.MediaStore
import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.net.URI
import java.net.URL
import java.security.MessageDigest
import javax.crypto.AEADBadTagException
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec
import javax.net.ssl.HttpsURLConnection

/** Explicit foreground-screen HTTPS inbox sync; no open Android port required. */
object InternetRelay {
    private const val MAX_CIPHER = 64L * 1024 * 1024 + 64
    private const val MAX_FILE = 2L * 1024 * 1024 * 1024

    private fun endpoint(value: String): String {
        val uri = URI(value.trim())
        require(uri.scheme == "https" && !uri.host.isNullOrBlank() &&
            uri.userInfo == null && uri.query == null && uri.fragment == null &&
            (uri.path.isNullOrEmpty() || uri.path == "/")) {
            "Введи HTTPS-адресу сервера без шляху та параметрів."
        }
        return uri.toASCIIString().trimEnd('/')
    }

    private fun connection(base: String, path: String, token: String,
                           method: String = "GET"): HttpsURLConnection =
        (URL(base + path).openConnection() as HttpsURLConnection).apply {
            instanceFollowRedirects = false
            requestMethod = method
            connectTimeout = 15000
            readTimeout = 120000
            useCaches = false
            setRequestProperty("Authorization", "Bearer " + token)
            setRequestProperty("Cache-Control", "no-store")
        }

    private fun check(conn: HttpsURLConnection) {
        if (conn.responseCode !in 200..299)
            throw IllegalStateException("Relay HTTP " + conn.responseCode)
    }

    private fun hex(bytes: ByteArray): String =
        bytes.joinToString("") { "%02x".format(it.toInt() and 255) }

    private fun decrypt(data: ByteArray, secret: String): ByteArray {
        require(secret.matches(Regex("[0-9a-fA-F]{64}"))) { "E2EE ключ: 64 hex-символи" }
        require(data.size >= 37 && String(data, 0, 8, Charsets.US_ASCII) == "SYNCHRO1") {
            "Невідомий E2EE формат"
        }
        val key = ByteArray(32) { secret.substring(it * 2, it * 2 + 2).toInt(16).toByte() }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, SecretKeySpec(key, "AES"),
            GCMParameterSpec(128, data.copyOfRange(8, 20)))
        return cipher.doFinal(data, 20, data.size - 20)
    }

    private fun ack(base: String, token: String, id: String, sha: String) {
        val c = connection(base, "/api/v1/transfers/" + id + "/ack", token, "POST")
        try {
            c.doOutput = true
            c.setRequestProperty("Content-Type", "application/json")
            c.outputStream.use {
                it.write(JSONObject().put("sha256", sha).toString().toByteArray(Charsets.UTF_8))
            }
            if (c.responseCode !in listOf(200, 204, 404, 409)) check(c)
        } finally { c.disconnect() }
    }

    fun receive(context: Context, server: String, token: String, e2eeKey: String,
                progress: (String) -> Unit): String {
        val base = endpoint(server)
        require(token.matches(Regex("[0-9a-fA-F]{64}"))) { "Токен пристрою: 64 hex-символи" }
        val inbox = connection(base, "/api/v1/inbox", token)
        val items: JSONArray
        try {
            check(inbox)
            items = JSONArray(inbox.inputStream.bufferedReader().use { it.readText() })
        } finally { inbox.disconnect() }
        val receipts = context.getSharedPreferences("internet_receipts", Context.MODE_PRIVATE)
        var received = 0
        var restored = 0
        var missingKey = 0
        var invalidKey = 0
        for (i in 0 until items.length()) {
            val item = items.getJSONObject(i)
            if (item.getString("status") != "ready") continue
            val id = item.getString("id")
            require(id.matches(Regex("[0-9a-f-]{36}"))) { "Invalid transfer ID" }
            val expected = item.getString("sha256")
            val total = item.getLong("size_bytes")
            require(total in 1..MAX_FILE) { "Перевищено максимальний розмір" }
            if (receipts.getString(id, null) == expected) {
                ack(base, token, id, expected)
                restored++
                continue
            }
            val encrypted = item.optBoolean("encrypted", false)
            if (encrypted && e2eeKey.isBlank()) { missingKey++; continue }
            if (encrypted && !e2eeKey.matches(Regex("[0-9a-fA-F]{64}"))) {
                throw IllegalArgumentException("E2EE-ключ має містити рівно 64 hex-символи")
            }
            if (encrypted && total > MAX_CIPHER)
                throw IllegalStateException("E2EE файл завеликий для мобільного дешифрування.")
            val name = SynchroConfig.safeName(item.getString("filename"))
            progress("Отримання " + name)
            val download = connection(base, "/api/v1/transfers/" + id + "/download", token)
            var local: Uri? = null
            try {
                check(download)
                val resolver = context.contentResolver
                val destination = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI,
                    ContentValues().apply {
                        put(MediaStore.MediaColumns.DISPLAY_NAME, name)
                        put(MediaStore.MediaColumns.MIME_TYPE, "application/octet-stream")
                        put(MediaStore.MediaColumns.RELATIVE_PATH, SynchroConfig.FOLDER)
                        put(MediaStore.MediaColumns.IS_PENDING, 1)
                    }) ?: error("MediaStore unavailable")
                local = destination
                val digest = MessageDigest.getInstance("SHA-256")
                var count = 0L
                if (encrypted) {
                    val data = ByteArrayOutputStream()
                    download.inputStream.use { input ->
                        val buf = ByteArray(65536)
                        while (true) {
                            val n = input.read(buf)
                            if (n < 0) break
                            count += n
                            require(count <= total && count <= MAX_CIPHER) { "Too many bytes" }
                            digest.update(buf, 0, n)
                            data.write(buf, 0, n)
                        }
                    }
                    require(count == total && hex(digest.digest()).equals(expected, true)) {
                        "SHA-256 ciphertext mismatch"
                    }
                    val cleartext = decrypt(data.toByteArray(), e2eeKey)
                    resolver.openOutputStream(destination, "w")?.use { it.write(cleartext) }
                        ?: error("Cannot write Downloads file")
                } else {
                    resolver.openOutputStream(destination, "w")?.use { output ->
                        download.inputStream.use { input ->
                            val buf = ByteArray(65536)
                            while (true) {
                                val n = input.read(buf)
                                if (n < 0) break
                                count += n
                                require(count <= total) { "Unexpected response size" }
                                digest.update(buf, 0, n)
                                output.write(buf, 0, n)
                            }
                        }
                        output.flush()
                    } ?: error("Cannot write Downloads file")
                    require(count == total && hex(digest.digest()).equals(expected, true)) {
                        "SHA-256 mismatch"
                    }
                }
                require(resolver.update(destination, ContentValues().apply {
                    put(MediaStore.MediaColumns.IS_PENDING, 0)
                }, null, null) == 1) { "Failed to publish download" }
                receipts.edit().putString(id, expected).apply()
                local = null
                // The local verified receipt survives an HTTP ACK failure.
                ack(base, token, id, expected)
                received++
            } catch (_: AEADBadTagException) {
                // The wrong key for one transfer must not abort downloads of
                // unrelated ready files. The finally block deletes the pending file.
                invalidKey++
                progress("Ключ не підходить для: " + name)
            } finally {
                download.disconnect()
                if (local != null) context.contentResolver.delete(local, null, null)
            }
        }
        return "Отримано: " + received + ", збережено раніше: " + restored +
            ", потрібен E2EE-ключ: " + missingKey +
            ", неправильний ключ: " + invalidKey
    }
}
