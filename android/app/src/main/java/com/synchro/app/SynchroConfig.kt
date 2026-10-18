package com.synchro.app

import android.content.Context
import java.security.SecureRandom

object SynchroConfig {
    const val PORT = 8765
    const val FOLDER = "Download/Synchro/Debug/"
    const val MAX_FILE_BYTES = 2L * 1024 * 1024 * 1024

    fun token(context: Context): String {
        val prefs = context.getSharedPreferences("pairing", Context.MODE_PRIVATE)
        val saved = prefs.getString("token", null)
        if (!saved.isNullOrEmpty()) return saved
        val bytes = ByteArray(16)
        SecureRandom().nextBytes(bytes)
        val generated = bytes.joinToString("") { "%02x".format(it.toInt() and 0xff) }
        prefs.edit().putString("token", generated).apply()
        return generated
    }

    fun safeName(input: String): String {
        val name = input.filter { it.code > 31 && it.code != 127 && it != '/' && it != '\\' }
            .trim().trimStart('.').take(150)
        return if (name.isBlank() || name == "." || name == "..") "file.bin" else name
    }
}
