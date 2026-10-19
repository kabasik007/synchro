package com.synchro.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.ContentValues
import android.content.Intent
import android.os.IBinder
import android.os.Environment
import android.os.StatFs
import android.system.ErrnoException
import android.system.OsConstants
import android.provider.MediaStore
import org.json.JSONObject
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.io.EOFException
import java.io.InputStream
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketException
import java.net.SocketTimeoutException
import java.net.URLDecoder
import java.nio.charset.StandardCharsets
import java.security.MessageDigest
import java.util.Locale
import java.util.Collections
import java.util.concurrent.Semaphore
import java.util.concurrent.TimeUnit
import java.util.concurrent.Executors

/** Explicitly started foreground LAN receiver. No public internet port is needed. */
class ReceiverService : Service() {
    companion object {
        @Volatile var running = false
            private set
        @Volatile var bytesReceived: Long = 0
            private set
        @Volatile var bytesTotal: Long = 0
            private set
        @Volatile var lastError: String? = null
            private set
        @Volatile var lastTransferError: String? = null
            private set
        private const val CHANNEL_ID = "synchro-receiver"
        private const val NOTIFICATION_ID = 1001
        private const val MAX_HEADERS = 16 * 1024
    }

    private var serverSocket: ServerSocket? = null
    private val activeSockets: MutableSet<Socket> = Collections.synchronizedSet(mutableSetOf())
    private val worker = Executors.newSingleThreadExecutor()
    private val clients = Executors.newFixedThreadPool(3)
    private val clientSlots = Semaphore(3)
    private val uploadSlot = Semaphore(1)

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
            .setSmallIcon(R.drawable.ic_synchro)
            .setContentIntent(open)
            .setOngoing(true)
            .build()
        startForeground(NOTIFICATION_ID, notice)
        worker.execute {
            lastError = null
            lastTransferError = null
            try {
                cleanupPendingUploads()
                ServerSocket(SynchroConfig.PORT).use { server ->
                    serverSocket = server
                    running = true
                    while (!Thread.currentThread().isInterrupted && !server.isClosed) {
                        val socket = server.accept()
                        if (!clientSlots.tryAcquire()) {
                            try { socket.use { reply(it, 503, "Receiver busy, retry shortly") } }
                            catch (_: Exception) {}
                            continue
                        }
                        activeSockets.add(socket)
                        try {
                            clients.execute {
                                try {
                                    socket.use { handle(it) }
                                } catch (_: Exception) {
                                    // A bad client never stops other healthy sessions.
                                } finally {
                                    activeSockets.remove(socket)
                                    clientSlots.release()
                                }
                            }
                        } catch (_: Exception) {
                            activeSockets.remove(socket)
                            clientSlots.release()
                            try { socket.close() } catch (_: Exception) {}
                        }
                    }
                }
            } catch (e: SocketException) {
                if (running) lastError = e.message
            } catch (e: Exception) {
                lastError = e.message ?: "Unable to bind receiver port"
            } finally {
                running = false
                stopSelf()
            }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int = START_NOT_STICKY

    override fun onDestroy() {
        running = false
        bytesReceived = 0
        bytesTotal = 0
        try { serverSocket?.close() } catch (_: Exception) {}
        synchronized(activeSockets) {
            activeSockets.forEach { try { it.close() } catch (_: Exception) {} }
            activeSockets.clear()
        }
        worker.shutdownNow()
        clients.shutdownNow()
        super.onDestroy()
    }

    override fun onTimeout(startId: Int, fgsType: Int) {
        // Android 15+: dataSync has a six-hour background budget per 24h.
        lastError = "Android stopped the receiver after the foreground-service time limit. Open Synchro to restart."
        stopSelf()
    }

    private fun readLine(socket: Socket, input: InputStream, limit: Int, deadlineNs: Long): String {
        val out = java.io.ByteArrayOutputStream()
        while (out.size() <= limit) {
            val remainingMs = TimeUnit.NANOSECONDS.toMillis(deadlineNs - System.nanoTime())
            if (remainingMs <= 0) throw SocketTimeoutException("Request headers timed out")
            socket.soTimeout = minOf(remainingMs, 3000L).toInt().coerceAtLeast(1)
            val byte = input.read()
            if (byte < 0) throw EOFException("Connection closed")
            if (byte == 10) return out.toString("UTF-8").trimEnd('\r')
            out.write(byte)
        }
        throw IllegalArgumentException("Header too long")
    }

    private fun cleanupPendingUploads() {
        // Only our application's own Downloads collection is visible by default on Android 10+.
        // An upload from a killed process can leave a hidden IS_PENDING item behind.
        try {
            @Suppress("DEPRECATION")
            val withPending = MediaStore.setIncludePending(MediaStore.Downloads.EXTERNAL_CONTENT_URI)
            contentResolver.query(withPending, arrayOf(MediaStore.MediaColumns._ID),
                "${MediaStore.MediaColumns.RELATIVE_PATH}=? AND ${MediaStore.MediaColumns.IS_PENDING}=1",
                arrayOf(SynchroConfig.FOLDER), null)?.use { cursor ->
                val index = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns._ID)
                while (cursor.moveToNext()) {
                    val item = android.content.ContentUris.withAppendedId(
                        MediaStore.Downloads.EXTERNAL_CONTENT_URI, cursor.getLong(index))
                    try { contentResolver.delete(item, null, null) } catch (_: Exception) {}
                }
            }
        } catch (_: Exception) {
            // MediaStore may not be mounted yet; any pending entry retains platform expiry.
        }
    }

    private fun outOfSpace(error: Throwable): Boolean {
        var cause: Throwable? = error
        while (cause != null) {
            if (cause is ErrnoException && cause.errno == OsConstants.ENOSPC) return true
            if (cause.message?.contains("No space", ignoreCase = true) == true) return true
            cause = cause.cause
        }
        return false
    }

    private fun reply(socket: Socket, code: Int, message: String) {
        val reason = when (code) {
            200 -> "OK"
            201 -> "Created"
            400 -> "Bad Request"
            401 -> "Unauthorized"
            408 -> "Request Timeout"
            413 -> "Content Too Large"
            500 -> "Internal Server Error"
            503 -> "Service Unavailable"
            507 -> "Insufficient Storage"
            else -> "Server Error"
        }
        val payload = JSONObject().put("message", message)
        if (code == 200) {
            payload.put("protocol", 1)
            payload.put("appVersion", "0.3.1")
            payload.put("device", android.os.Build.MANUFACTURER + " " + android.os.Build.MODEL)
            payload.put("sdk", android.os.Build.VERSION.SDK_INT)
        }
        val data = payload.toString().toByteArray(StandardCharsets.UTF_8)
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
            val headerDeadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(12)
            val request = readLine(socket, input, 512, headerDeadline)
            val isHealthCheck = request == "GET /health HTTP/1.1"
            if (!isHealthCheck && request != "POST /upload HTTP/1.1") {
                reply(socket, 400, "Expected POST /upload or GET /health HTTP/1.1")
                return
            }
            val fields = mutableMapOf<String, String>()
            var totalHeaderBytes = 0
            while (true) {
                val line = readLine(socket, input, MAX_HEADERS, headerDeadline)
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
            // Check the external Downloads volume, not app-private filesDir.
            @Suppress("DEPRECATION")
            val downloadFolder = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
            if (downloadFolder.exists()) {
                val disk = StatFs(downloadFolder.absolutePath)
                if (disk.availableBytes < length + 32L * 1024 * 1024) {
                    reply(socket, 507, "Not enough free space in Downloads")
                    return
                }
            }
            if (!uploadSlot.tryAcquire()) {
                reply(socket, 503, "Another upload is already running")
                return
            }
            bytesTotal = length
            bytesReceived = 0
            try {
                socket.soTimeout = 60_000
                val bodyDeadline = System.nanoTime() + TimeUnit.MINUTES.toNanos(30)
                receiveFile(input, length, hash, name, bodyDeadline)
            } finally {
                bytesTotal = 0
                bytesReceived = 0
                uploadSlot.release()
            }
            lastTransferError = null
            reply(socket, 201, "Saved to ${SynchroConfig.FOLDER}$name")
        } catch (e: Exception) {
            lastTransferError = (e.message ?: "Transfer failed").take(180)
            val status = when {
                outOfSpace(e) -> 507
                e is SocketTimeoutException -> 408
                e is IllegalArgumentException || e is EOFException -> 400
                else -> 500
            }
            try { reply(socket, status, e.message ?: "Upload failed") } catch (_: Exception) {}
        }
    }

    private fun receiveFile(input: InputStream, length: Long, expectedSha: String, name: String,
        deadlineNs: Long) {
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
                    if (System.nanoTime() >= deadlineNs) {
                        throw SocketTimeoutException("Upload exceeded 30 minute limit")
                    }
                    val n = input.read(buffer, 0, minOf(buffer.size.toLong(), remaining).toInt())
                    if (n < 0) throw EOFException("Transfer interrupted")
                    if (n == 0) continue
                    buffered.write(buffer, 0, n)
                    digest.update(buffer, 0, n)
                    remaining -= n
                    bytesReceived = length - remaining
                }
                buffered.flush()
            } ?: throw IllegalStateException("Unable to open Downloads output")
            val actualSha = digest.digest().joinToString("") { "%02x".format(it.toInt() and 0xff) }
            if (!actualSha.equals(expectedSha, ignoreCase = true)) {
                throw IllegalArgumentException("SHA-256 mismatch: incomplete or changed file")
            }
            val ready = ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }
            val updated = resolver.update(uri, ready, null, null)
            if (updated != 1) throw IllegalStateException("Could not publish received file")
            ok = true
        } finally {
            if (!ok) resolver.delete(uri, null, null)
        }
    }
}
