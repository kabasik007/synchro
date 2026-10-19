package com.synchro.app

import android.content.Context
import android.net.Uri
import androidx.documentfile.provider.DocumentFile
import androidx.work.*
import org.json.JSONArray
import org.json.JSONObject
import java.net.URI
import java.net.URL
import java.security.MessageDigest
import java.util.concurrent.TimeUnit
import javax.net.ssl.HttpsURLConnection

/** Explicit opt-in HTTPS backup. Remote relay is TEMPORARY and NOT E2EE. */
object FolderBackup {
    private const val PREF = "folder_backup"
    private const val JOB = "synchro.selected.folder.backup"
    fun folders(context: Context): List<Uri> = try {
        val raw = context.getSharedPreferences(PREF, Context.MODE_PRIVATE)
            .getString("folders", "[]") ?: "[]"
        val array = JSONArray(raw)
        (0 until array.length()).map { Uri.parse(array.getString(it)) }
    } catch (_: Exception) { emptyList() }
    fun addFolder(context: Context, uri: Uri) {
        val all = folders(context).map { it.toString() }.toMutableList()
        if (!all.contains(uri.toString())) {
            require(all.size < 5) { "До 5 папок одночасно" }
            all.add(uri.toString())
        }
        context.getSharedPreferences(PREF, Context.MODE_PRIVATE).edit()
            .putString("folders", JSONArray(all).toString()).apply()
    }
    fun enabled(context: Context) = context.getSharedPreferences(PREF, Context.MODE_PRIVATE)
        .getBoolean("enabled", false)
    fun config(context: Context, server: String, token: String, recipient: String) {
        require(server.startsWith("https://")) { "Потрібен HTTPS" }
        require(token.matches(Regex("[0-9a-fA-F]{64}"))) { "Неправильний токен Android" }
        require(recipient.matches(Regex("[0-9a-fA-F-]{36}"))) { "Неправильний UUID Windows" }
        context.getSharedPreferences(PREF, Context.MODE_PRIVATE).edit()
            .putString("server", server).putString("token", token)
            .putString("recipient", recipient).apply()
    }
    private fun wifi() = Constraints.Builder().setRequiredNetworkType(NetworkType.UNMETERED).build()
    fun schedule(context: Context) {
        require(folders(context).isNotEmpty()) { "Спочатку вибери папку" }
        context.getSharedPreferences(PREF, Context.MODE_PRIVATE).edit()
            .putBoolean("enabled", true).apply()
        val req = PeriodicWorkRequest.Builder(FolderBackupWorker::class.java, 15, TimeUnit.MINUTES)
            .setConstraints(wifi()).build()
        WorkManager.getInstance(context).enqueueUniquePeriodicWork(
            JOB, ExistingPeriodicWorkPolicy.UPDATE, req)
    }
    fun once(context: Context) {
        require(folders(context).isNotEmpty()) { "Спочатку вибери папку" }
        val req = OneTimeWorkRequest.Builder(FolderBackupWorker::class.java)
            .setConstraints(wifi()).build()
        WorkManager.getInstance(context).enqueueUniqueWork(
            "$JOB.once", ExistingWorkPolicy.KEEP, req)
    }
    fun stop(context: Context, clear: Boolean = false) {
        val p = context.getSharedPreferences(PREF, Context.MODE_PRIVATE)
        p.edit().putBoolean("enabled", false)
            .also { if (clear) it.putString("folders", "[]") }.apply()
        WorkManager.getInstance(context).cancelUniqueWork(JOB)
        WorkManager.getInstance(context).cancelUniqueWork("$JOB.once")
    }
}

class FolderBackupWorker(ctx: Context, params: WorkerParameters): Worker(ctx, params) {
    private val prefs = applicationContext.getSharedPreferences("folder_backup", Context.MODE_PRIVATE)
    private val ledger = applicationContext.getSharedPreferences("folder_backup_ledger", Context.MODE_PRIVATE)
    private val CHUNK = 8 * 1024 * 1024
    private val MAX = 2L * 1024 * 1024 * 1024
    private fun sha(bytes: ByteArray): String = MessageDigest.getInstance("SHA-256")
        .digest(bytes).joinToString("") { "%02x".format(it.toInt() and 255) }

    private fun base(): String {
        val uri = URI(prefs.getString("server", "").orEmpty().trim())
        require(uri.scheme == "https" && !uri.host.isNullOrBlank() &&
            uri.userInfo == null && uri.query == null && uri.fragment == null &&
            (uri.path.isNullOrEmpty() || uri.path == "/")) { "HTTPS origin без параметрів" }
        return uri.toASCIIString().trimEnd('/')
    }
    private fun connection(base: String, route: String, token: String, method: String = "GET") =
        (URL(base + route).openConnection() as HttpsURLConnection).apply {
            requestMethod = method
            instanceFollowRedirects = false
            connectTimeout = 15000
            readTimeout = 120000
            useCaches = false
            setRequestProperty("Authorization", "Bearer " + token)
            setRequestProperty("Cache-Control", "no-store")
        }
    private fun ok(c: HttpsURLConnection) {
        require(c.responseCode in 200..299) { "Relay HTTP " + c.responseCode }
    }
    private fun get(base: String, path: String, token: String): JSONObject {
        val c = connection(base, path, token)
        try {
            ok(c)
            return JSONObject(c.inputStream.bufferedReader().use { it.readText() })
        } finally { c.disconnect() }
    }
    private fun post(base: String, path: String, token: String, data: JSONObject?): JSONObject {
        val c = connection(base, path, token, "POST")
        try {
            c.doOutput = true
            if (data != null) c.setRequestProperty("Content-Type", "application/json")
            c.outputStream.use {
                if (data != null) it.write(data.toString().toByteArray(Charsets.UTF_8))
            }
            ok(c)
            return JSONObject(c.inputStream.bufferedReader().use { it.readText() })
        } finally { c.disconnect() }
    }
    private fun request(base: String, path: String, token: String, method: String, data: ByteArray? = null) {
        val c = connection(base, path, token, method)
        try {
            if (data != null) {
                c.doOutput = true
                c.setFixedLengthStreamingMode(data.size)
                c.setRequestProperty("Content-Type", "application/octet-stream")
                c.outputStream.use { it.write(data) }
            }
            ok(c)
        } finally { c.disconnect() }
    }
    private fun streamHash(uri: Uri): String {
        val sha = MessageDigest.getInstance("SHA-256")
        val buf = ByteArray(64 * 1024)
        applicationContext.contentResolver.openInputStream(uri)?.use { input ->
            while (true) {
                if (isStopped) throw InterruptedException("Cancelled")
                val n = input.read(buf)
                if (n == -1) break
                sha.update(buf, 0, n)
            }
        } ?: error("Cannot open selected file")
        return sha.digest().joinToString("") { "%02x".format(it.toInt() and 255) }
    }
    private fun nextChunk(stream: java.io.InputStream, size: Int): ByteArray {
        val buf = ByteArray(size)
        var offset = 0
        while (offset < size) {
            if (isStopped) throw InterruptedException("Cancelled")
            val n = stream.read(buf, offset, size - offset)
            require(n > 0) { "File changed during upload" }
            offset += n
        }
        return buf
    }
    private fun name(parent: String, file: String): String {
        var value = (parent.take(25) + "_" + file.take(115))
            .filter { it.code > 31 && it != '/' && it != '\\' }.trim()
        while (value.toByteArray(Charsets.UTF_8).size > 180) value = value.dropLast(1)
        require(value.isNotBlank()) { "Bad filename" }
        return value
    }
    private fun upload(base: String, token: String, recipient: String, dir: DocumentFile, doc: DocumentFile) {
        val size = doc.length()
        if (size !in 1..MAX) return
        val modified = doc.lastModified()
        val key = sha((base + "|" + recipient + "|" + doc.uri + "|" + size + "|" + modified)
            .toByteArray(Charsets.UTF_8))
        var id = ledger.getString(key, null)?.substringBefore("|").orEmpty()
        val digest = streamHash(doc.uri)
        val old = ledger.getString(key, "").orEmpty()
        if (old.endsWith("|delivered|" + digest)) return
        if (id.isNotBlank()) {
            val state = try { get(base, "/api/v1/transfers/" + id, token) } catch (_: Exception) { null }
            if (state == null || state.optString("sha256") != digest ||
                state.optString("recipient_id") != recipient) id = ""
            else when (state.optString("status")) {
                "delivered" -> {
                    ledger.edit().putString(key, id + "|delivered|" + digest).commit()
                    return
                }
                "ready", "finalizing" -> return
            }
        }
        if (id.isBlank()) {
            id = post(base, "/api/v1/transfers", token, JSONObject()
                .put("to_device_id", recipient)
                .put("filename", name(dir.name ?: "Android", doc.name ?: "file.bin"))
                .put("size_bytes", size).put("sha256", digest).put("encrypted", false))
                .getJSONObject("transfer").getString("id")
            ledger.edit().putString(key, id + "|uploading|" + digest).commit()
        }
        val route = "/api/v1/transfers/" + id
        val checkpoint = get(base, route + "/chunks", token)
        if (checkpoint.optString("status") != "uploading") return
        val hashes = mutableMapOf<Int, String>()
        val saved = checkpoint.optJSONArray("uploaded_chunks") ?: JSONArray()
        for (i in 0 until saved.length()) {
            val item = saved.getJSONObject(i)
            hashes[item.getInt("index")] = item.getString("sha256")
        }
        require(checkpoint.getJSONArray("uploaded_indices").length() == 0 || hashes.isNotEmpty()) {
            "Онови Relay до v0.5.1 для перевірки чанків"
        }
        applicationContext.contentResolver.openInputStream(doc.uri)?.use { input ->
            var at = 0L
            var index = 0
            while (at < size) {
                val chunk = nextChunk(input, minOf(CHUNK.toLong(), size - at).toInt())
                if (hashes[index] != sha(chunk)) {
                    if (hashes.containsKey(index)) request(base, route + "/chunks/" + index, token, "DELETE")
                    request(base, route + "/chunks/" + index, token, "PUT", chunk)
                }
                at += chunk.size
                index++
            }
        } ?: error("Cannot reopen file")
        require(doc.length() == size && doc.lastModified() == modified) { "Source changed" }
        post(base, route + "/finish", token, null)
        // NOT delivered yet. The next WorkManager run checks for Windows ACK.
        ledger.edit().putString(key, id + "|uploading|" + digest).commit()
    }
    override fun doWork(): Result {
        return try {
            val base = base()
            val token = prefs.getString("token", "").orEmpty()
            val recipient = prefs.getString("recipient", "").orEmpty()
            require(token.matches(Regex("[0-9a-fA-F]{64}")) &&
                recipient.matches(Regex("[0-9a-fA-F-]{36}"))) { "Check Android token and Windows UUID" }
            val grants = applicationContext.contentResolver.persistedUriPermissions
                .filter { it.isReadPermission }.map { it.uri.toString() }.toSet()
            for (uri in FolderBackup.folders(applicationContext)) {
                if (isStopped) return Result.retry()
                require(uri.toString() in grants) { "SAF permission has expired" }
                val dir = DocumentFile.fromTreeUri(applicationContext, uri) ?: error("Folder unavailable")
                for (doc in dir.listFiles().filter { it.isFile }.take(200)) {
                    if (isStopped) return Result.retry()
                    val file = doc.name.orEmpty()
                    if (file.startsWith(".") || file.endsWith(".tmp") || file.endsWith(".part")) continue
                    upload(base, token, recipient, dir, doc)
                }
            }
            prefs.edit().putString("last_result", "Перевірено " + System.currentTimeMillis())
                .remove("last_error").apply()
            Result.success()
        } catch (e: Exception) {
            prefs.edit().putString("last_error", e.message ?: "Помилка копіювання").apply()
            Result.retry()
        }
    }
}
