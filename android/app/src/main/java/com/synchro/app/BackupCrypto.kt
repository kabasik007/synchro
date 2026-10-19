package com.synchro.app

import android.content.Context
import android.net.Uri
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.io.InputStream
import java.security.KeyStore
import java.security.MessageDigest
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.Mac
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec
import java.nio.ByteBuffer

/** Synchro Backup v1: streamable, authenticated 1 MiB records.
 * Header = "SYNBKP01" (8 bytes) | salt (16) | nonce prefix (8) | length (8 BE).
 * Subkey = HMAC-SHA256(master, salt || "SynchroBackup-v1").
 * Record nonce = prefix (8) || record index (4 BE).
 * Record AAD = 40-byte header || record index (4 BE).
 * Each record carries an independent 16-byte GCM tag.
 */
object BackupCrypto {
    private const val KEY_ALIAS = "SynchroBackupWrappingKeyV1"
    private const val PREF = "synchro_backup_key"
    private const val PURPOSE = "SynchroBackup-v1"
    private const val RECORD = 1024 * 1024
    private val MAGIC = "SYNBKP01".toByteArray(Charsets.US_ASCII)
    private val random = SecureRandom()

    fun validateHex(value: String) {
        require(value.matches(Regex("[a-fA-F0-9]{64}"))) {
            "Ключ має складатися з 64 hex-символів (256 біт)"
        }
    }

    private fun wrapKey(): java.security.Key {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        store.getKey(KEY_ALIAS, null)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(KeyGenParameterSpec.Builder(KEY_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256).build())
        return gen.generateKey()
    }

    fun saveMaster(context: Context, hex: String) {
        validateHex(hex)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, wrapKey())
        val sealed = cipher.doFinal(hex.lowercase().toByteArray(Charsets.US_ASCII))
        val combined = cipher.iv + sealed
        require(context.getSharedPreferences(PREF, Context.MODE_PRIVATE).edit()
            .putString("wrapped", Base64.encodeToString(combined, Base64.NO_WRAP)).commit()) {
            "Не вдалося зберегти ключ у Android Keystore"
        }
    }

    fun hasMaster(context: Context): Boolean = context
        .getSharedPreferences(PREF, Context.MODE_PRIVATE).contains("wrapped")

    fun loadMaster(context: Context): ByteArray {
        val value = context.getSharedPreferences(PREF, Context.MODE_PRIVATE)
            .getString("wrapped", null) ?: error("Спочатку введи ключ резервного копіювання")
        val sealed = Base64.decode(value, Base64.NO_WRAP)
        require(sealed.size > 28) { "Пошкоджено збережений ключ" }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, wrapKey(), GCMParameterSpec(128, sealed.copyOfRange(0, 12)))
        val secret = String(cipher.doFinal(sealed, 12, sealed.size - 12), Charsets.US_ASCII)
        validateHex(secret)
        return ByteArray(32) { secret.substring(it * 2, it * 2 + 2).toInt(16).toByte() }
    }

    data class Parameters(val salt: ByteArray, val prefix: ByteArray) {
        init {
            require(salt.size == 16 && prefix.size == 8)
        }
        fun header(size: Long): ByteArray = ByteBuffer.allocate(40)
            .put(MAGIC).put(salt).put(prefix).putLong(size).array()
    }

    fun fresh(): Parameters = Parameters(ByteArray(16).also(random::nextBytes),
        ByteArray(8).also(random::nextBytes))

    fun hex(data: ByteArray) = data.joinToString("") { "%02x".format(it.toInt() and 255) }
    fun fromHex(data: String): ByteArray {
        require(data.length % 2 == 0 && data.all { it in "0123456789abcdefABCDEF" })
        return ByteArray(data.length / 2) { i -> data.substring(i*2, i*2+2).toInt(16).toByte() }
    }

    fun cipherSize(plain: Long): Long {
        require(plain in 1..(2L * 1024 * 1024 * 1024 - 40000))
        return 40L + plain + ((plain + RECORD - 1) / RECORD) * 16
    }

    fun stream(source: InputStream, size: Long, master: ByteArray, params: Parameters): InputStream {
        require(master.size == 32)
        return RecordEncryptionStream(source, size, master, params)
    }

    private class RecordEncryptionStream(
        private val source: InputStream, private val size: Long,
        master: ByteArray, private val params: Parameters
    ): InputStream() {
        private val header = params.header(size)
        private val mac = Mac.getInstance("HmacSHA256")
        private val key: SecretKeySpec
        private var buffer: ByteArray = header
        private var pos = 0
        private var produced = 0L
        private var index = 0
        private var finished = false

        init {
            mac.init(SecretKeySpec(master, "HmacSHA256"))
            key = SecretKeySpec(mac.doFinal(params.salt + PURPOSE.toByteArray(Charsets.US_ASCII)), "AES")
        }

        override fun read(): Int {
            val one = ByteArray(1)
            return if (read(one, 0, 1) < 0) -1 else one[0].toInt() and 255
        }
        override fun read(target: ByteArray, off: Int, len: Int): Int {
            if (off < 0 || len < 0 || off > target.size - len) throw IndexOutOfBoundsException()
            if (len == 0) return 0
            if (pos >= buffer.size) {
                if (finished) return -1
                fill()
                if (pos >= buffer.size) return -1
            }
            val n = minOf(len, buffer.size - pos)
            System.arraycopy(buffer, pos, target, off, n)
            pos += n
            return n
        }

        private fun fill() {
            if (produced >= size) {
                require(source.read() == -1) { "Файл змінився під час шифрування" }
                finished = true
                buffer = ByteArray(0)
                pos = 0
                return
            }
            require(index >= 0) { "Забагато блоків" }
            val length = minOf(RECORD.toLong(), size - produced).toInt()
            val plain = ByteArray(length)
            var offset = 0
            while (offset < length) {
                val n = source.read(plain, offset, length - offset)
                require(n > 0) { "Файл скоротився під час шифрування" }
                offset += n
            }
            val counter = ByteBuffer.allocate(4).putInt(index).array()
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, key,
                GCMParameterSpec(128, params.prefix + counter))
            cipher.updateAAD(header + counter)
            buffer = cipher.doFinal(plain)
            pos = 0
            produced += length
            index++
        }

        override fun close() { source.close() }
    }

    fun shaStream(stream: InputStream): String {
        val digest = MessageDigest.getInstance("SHA-256")
        stream.use {
            val bytes = ByteArray(64*1024)
            while (true) {
                val n = it.read(bytes)
                if (n == -1) break
                digest.update(bytes, 0, n)
            }
        }
        return hex(digest.digest())
    }
}
