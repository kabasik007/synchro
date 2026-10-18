package com.synchro.app

import android.Manifest
import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ContentUris
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.MediaStore
import android.provider.Settings
import android.view.ViewGroup
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import java.net.Inet4Address
import java.net.NetworkInterface
import java.util.Locale

class MainActivity : Activity() {
    private lateinit var status: TextView
    private lateinit var files: LinearLayout
    private val handler = Handler(Looper.getMainLooper())
    private val update = object : Runnable {
        override fun run() {
            refresh()
            handler.postDelayed(this, 3000)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(28, 24, 28, 24)
        }
        val scroll = ScrollView(this)
        scroll.addView(root)
        setContentView(scroll)
        root.addView(TextView(this).apply { text = "Synchro  ·  Android"; textSize = 26f })
        root.addView(TextView(this).apply {
            text = "Receive APKs and files directly from your PC.\nDownloads/Synchro/Debug"
            textSize = 15f
        })
        status = TextView(this).apply { textSize = 18f }
        root.addView(status)
        root.addView(TextView(this).apply {
            text = "Android addresses (use the Wi-Fi or Tailscale address on your PC):"
        })
        val pairInfo = TextView(this).apply {
            text = pairText()
            textSize = 16f
            setTextIsSelectable(true)
        }
        root.addView(pairInfo)
        root.addView(Button(this).apply {
            text = "Copy pairing details"
            setOnClickListener {
                val clipboard = getSystemService(ClipboardManager::class.java)
                clipboard.setPrimaryClip(ClipData.newPlainText("Synchro pairing", pairText()))
            }
        })
        root.addView(Button(this).apply {
            text = "Start receiving"
            setOnClickListener {
                try {
                    startForegroundService(Intent(this@MainActivity, ReceiverService::class.java))
                    status.text = "Starting receiver…"
                    askNotifications()
                } catch (e: Exception) { status.text = "Couldn't start: ${e.message}" }
            }
        })
        root.addView(Button(this).apply {
            text = "Stop receiving"
            setOnClickListener { stopService(Intent(this@MainActivity, ReceiverService::class.java)); refresh() }
        })
        root.addView(Button(this).apply {
            text = "Refresh received files"
            setOnClickListener { refresh() }
        })
        root.addView(TextView(this).apply {
            text = "Received files — tap Install for APK files"
            textSize = 20f
        })
        files = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        root.addView(files, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.WRAP_CONTENT))
        root.addView(TextView(this).apply {
            text = "For remote access, connect Windows and Android to the same Tailscale network. " +
                "Never forward port 8765 to the public Internet. " +
                "Android requires explicit approval for app installation."
        })
    }

    override fun onResume() {
        super.onResume()
        handler.removeCallbacks(update)
        handler.post(update)
    }

    override fun onPause() {
        handler.removeCallbacks(update)
        super.onPause()
    }

    private fun askNotifications() {
        if (Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 101)
        }
    }

    private fun pairText(): String {
        val interfaces = NetworkInterface.getNetworkInterfaces()?.toList() ?: emptyList()
        val ips = interfaces.asSequence().filter { it.isUp && !it.isLoopback }
            .flatMap { network -> network.inetAddresses.toList().asSequence()
                .filterIsInstance<Inet4Address>()
                .filter { !it.isLoopbackAddress }
                .map { "${network.displayName}: http://${it.hostAddress}:${SynchroConfig.PORT}" } }
            .toList()
        return (if (ips.isEmpty()) "No active IPv4 address" else ips.joinToString("\n")) +
            "\n\nPairing token:\n${SynchroConfig.token(this)}"
    }

    private fun refresh() {
        status.text = if (ReceiverService.running) "● Receiving enabled · port ${SynchroConfig.PORT}"
            else "○ Receiver stopped"
        files.removeAllViews()
        val cols = arrayOf(MediaStore.MediaColumns._ID, MediaStore.MediaColumns.DISPLAY_NAME,
            MediaStore.MediaColumns.SIZE)
        try {
            contentResolver.query(MediaStore.Downloads.EXTERNAL_CONTENT_URI, cols,
                "${MediaStore.MediaColumns.RELATIVE_PATH}=? AND ${MediaStore.MediaColumns.IS_PENDING}=0",
                arrayOf(SynchroConfig.FOLDER),
                "${MediaStore.MediaColumns.DATE_ADDED} DESC")?.use { cursor ->
                val idIx = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns._ID)
                val nameIx = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.DISPLAY_NAME)
                val sizeIx = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.SIZE)
                var n = 0
                while (cursor.moveToNext() && n++ < 40) {
                    val file = cursor.getString(nameIx) ?: continue
                    val size = cursor.getLong(sizeIx)
                    val uri = ContentUris.withAppendedId(MediaStore.Downloads.EXTERNAL_CONTENT_URI,
                        cursor.getLong(idIx))
                    val line = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
                    line.addView(TextView(this).apply {
                        text = "$file · ${size / 1024} KiB"
                        textSize = 15f
                    })
                    line.addView(Button(this).apply {
                        text = if (file.lowercase(Locale.ROOT).endsWith(".apk")) "Install APK" else "Open file"
                        setOnClickListener { openFile(uri, file) }
                    })
                    files.addView(line)
                }
                if (n == 0) files.addView(TextView(this).apply { text = "No received files yet" })
            }
        } catch (e: Exception) {
            files.addView(TextView(this).apply { text = "Cannot list files: ${e.message}" })
        }
    }

    private fun openFile(uri: Uri, name: String) {
        val apk = name.lowercase(Locale.ROOT).endsWith(".apk")
        if (apk && !packageManager.canRequestPackageInstalls()) {
            startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES).apply {
                data = Uri.parse("package:$packageName")
            })
            return
        }
        try {
            val type = if (apk) "application/vnd.android.package-archive" else "*/*"
            startActivity(Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, type)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            })
        } catch (e: Exception) {
            status.text = "No app can open this file: ${e.message}"
        }
    }
}
