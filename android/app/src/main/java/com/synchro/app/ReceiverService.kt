package com.synchro.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.ContentValues
import android.content.Intent
import android.os.IBinder
import android.os.StatFs
import android.provider.MediaStore
import org.json.JSONObject
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.io.EOFException
import java.io.InputStream
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketException
import java.net.URLDecoder
import java.nio.charset.StandardCharsets
import java.security.MessageDigest
import java.util.Locale
import java.util.concurrent.Executors

/** Explicitly started foreground LAN receiver. No public internet port is needed. */
class ReceiverService : Service() {
    companion object {
        @Volatile var running = false
            private set
        private const val CHANNEL_ID = "synchro-receiver"
        private const val NOTIFICATION_ID = 1001
        private const val MAX_HEADERS = 16 * 1024
    }

    private var serverSocket: ServerSocket? = null
    private val worker = Executors.newSingleThreadExecutor()

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(NotificationChannel(
            CHANNEL_ID, "Synchro transfer", NotificationManager.IMPORTANCE_LOW
        ))
        val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val notice = Notification.Builder(this, CHANNEL_ID)
            .setContentTitle("Synchro is receiving")
            .setContentText("Ready for uploads · port ${SynchroConfig.PORT}")
            .setSmallIcon(android.R.drawable.stat_sys_download)
            .setContentIntent(open)
            .setOngoing(true)
            .build()
        startForeground(NOTIFICATION_ID, notice)
        worker.execute {
            try {
                ServerSocket(SynchroConfig.PORT).use { server ->
                    serverSocket = server
                    running = true
                    while (!Thread.currentThread().isInterrupted && !server.isClosed) {
                        val socket = server.accept()
                        // Sequential uploads to avoid uncontrolled memory/disk pressure.
                        try { socket.use { handle(it) } } catch (_: Exception) { /* one bad client must not end service */ }
                    }
                }
            } catch (_: SocketException) {
                // Expected on stop().
            } catch (_: Exception) {
                // Port in use or network unavailable.
            } finally {
                running = false
                stopSelf()
            }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int = START_NOT_STICKY

    override fun onDestroy() {
        running = false
        try { serverSocket?.close() } catch (_: Exception) {}
        worker.shutdownNow()
        super.onDestroy()
    }

    override fun onTimeout(startId: Int, fgsType: Int) {
        // Android 15+ has a budget for dataSync foreground services.
        stopSelf()
    }

    private fun readLine(input: InputStream, limit: Int): String {
        val out = java.io.ByteArrayOutputStream()
        while (out.size() <= limit) {
            val byte = input.read()
            if (byte < 0) throw EOFException("Connection closed")
            if (byte == 10) return out.toString("UTF-8").trimEnd('\r')
            out.write(byte)
        }
        throw IllegalArgumentException("Header too long")
    }

    private fun reply(socket: Socket, code: Int, message: String) {
        val reason = when (code) {
            200 -> "OK"
            201 -> "Created"
            400 -> "Bad Request"
            401 -> "Unauthorized"
            413 -> "Content Too Large"
            507 -> "Insufficient Storage"
            else -> "Server Error"
        }
        val data = JSONObject().put("message", message).toString().toByteArray(StandardCharsets.UTF_8)
        val headers = "HTTP/1.1 $code $reason\r\nContent-Type: application/json\r\n" +
            "Content-Length: ${data.size}\r\nConnection: close\r\n\r\n"
        socket.getOutputStream().write(headers.toByteArray(StandardCharsets.US_ASCII))
        socket.getOutputStream().write(data)
        socket.getOutputStream().flush()
    }

    private fun handle(socket: Socket) {
        socket.soTimeout = 60_000
        val input = BufferedInputStream(socket.getInputStream(), 64 * 1024)
        try {
            val request = readLine(input, 512)
            val isHealthCheck = request == "GET /health HTTP/1.1"
            if (!isHealthCheck && request != "POST /upload HTTP/1.1") {
                reply(socket, 400, "Expected POST /upload or GET /health HTTP/1.1")
                return
            }
            val fields = mutableMapOf<String, String>()
            var totalHeaderBytes = 0
            while (true) {
                val line = readLine(input, MAX_HEADERS)
                totalHeaderBytes += line.length + 2
                if (totalHeaderBytes > MAX_HEADERS) throw IllegalArgumentException("Headers too large")
                if (line.isEmpty()) break
                val colon = line.indexOf(':')
                if (colon <= 0) throw IllegalArgumentException("Bad header")
                val key = line.substring(0, colon).trim().lowercase(Locale.ROOT)
                if (fields.containsKey(key)) throw IllegalArgumentException("Duplicate header")
                fields[key] = line.substring(colon + 1).trim()
            }
            val expectedToken = SynchroConfig.token(this).toByteArray(StandardCharsets.US_ASCII)
            val suppliedToken = (fields["x-synchro-token"] ?: "")
                .toByteArray(StandardCharsets.US_ASCII)
            if (!MessageDigest.isEqual(expectedToken, suppliedToken)) {
                reply(socket, 401, "Pairing token not accepted")
                return
            }
            if (isHealthCheck) {
                reply(socket, 200, "Synchro Android receiver is ready")
                return
            }
            val length = fields["content-length"]?.toLongOrNull()
            if (length == null || length <= 0) {
                reply(socket, 400, "Missing Content-Length")
                return
            }
            if (length > SynchroConfig.MAX_FILE_BYTES) {
                reply(socket, 413, "Maximum file size is 2 GiB")
                return
            }
            if (fields["transfer-encoding"] != null) {
                reply(socket, 400, "Chunked upload not supported")
                return
            }
            val hash = fields["x-synchro-sha256"] ?: ""
            if (!hash.matches(Regex("[a-fA-F0-9]{64}"))) {
                reply(socket, 400, "Missing SHA-256")
                return
            }
            val rawName = fields["x-synchro-name"] ?: ""
            if (rawName.length > 1000 || rawName.isBlank()) {
                reply(socket, 400, "Invalid filename")
                return
            }
            val name = SynchroConfig.safeName(
                URLDecoder.decode(rawName, StandardCharsets.UTF_8.name()))
            val disk = StatFs(filesDir.absolutePath)
            if (disk.availableBytes < length + 32L * 1024 * 1024) {
                reply(socket, 507, "Not enough free space")
                return
            }
            receiveFile(input, length, hash, name)
            reply(socket, 201, "Saved to ${SynchroConfig.FOLDER}$name")
        } catch (e: Exception) {
            try { reply(socket, 400, e.message ?: "Upload failed") } catch (_: Exception) {}
        }
    }

    private fun receiveFile(input: InputStream, length: Long, expectedSha: String, name: String) {
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, name)
            put(MediaStore.MediaColumns.MIME_TYPE,
                if (name.lowercase(Locale.ROOT).endsWith(".apk"))
                    "application/vnd.android.package-archive" else "application/octet-stream")
            put(MediaStore.MediaColumns.RELATIVE_PATH, SynchroConfig.FOLDER)
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
        val resolver = contentResolver
        val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
            ?: throw IllegalStateException("Unable to create Downloads file")
        var ok = false
        try {
            val digest = MessageDigest.getInstance("SHA-256")
            resolver.openOutputStream(uri, "w")?.use { output ->
                val buffered = BufferedOutputStream(output, 64 * 1024)
                val buffer = ByteArray(64 * 1024)
                var remaining = length
                while (remaining > 0) {
                    val n = input.read(buffer, 0, minOf(buffer.size.toLong(), remaining).toInt())
                    if (n < 0) throw EOFException("Transfer interrupted")
                    if (n == 0) continue
                    buffered.write(buffer, 0, n)
                    digest.update(buffer, 0, n)
                    remaining -= n
                }
                buffered.flush()
            } ?: throw IllegalStateException("Unable to open Downloads output")
            val actualSha = digest.digest().joinToString("") { "%02x".format(it.toInt() and 0xff) }
            if (!actualSha.equals(expectedSha, ignoreCase = true)) {
                throw IllegalArgumentException("SHA-256 mismatch: incomplete or changed file")
            }
            val ready = ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }
            resolver.update(uri, ready, null, null)
            ok = true
        } finally {
            if (!ok) resolver.delete(uri, null, null)
        }
    }
}
