@file:Suppress("DEPRECATION")

package com.synchro.app

import android.Manifest
import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ContentUris
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.MediaStore
import android.provider.Settings
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import java.net.Inet4Address
import java.net.NetworkInterface
import java.text.DateFormat
import java.util.Date
import java.util.Locale

/** v0.3 — light native UI, with no simulated connectivity or device details. */
class MainActivity : Activity() {
    private val ink = Color.rgb(17, 29, 55)
    private val muted = Color.rgb(105, 124, 153)
    private val blue = Color.rgb(24, 106, 245)
    private val paleBlue = Color.rgb(232, 240, 255)
    private val border = Color.rgb(224, 231, 244)
    private val green = Color.rgb(14, 181, 103)
    private val page = Color.rgb(246, 249, 255)

    private lateinit var status: TextView
    private lateinit var detail: TextView
    private lateinit var statusCard: LinearLayout
    private lateinit var historyTitle: TextView
    private lateinit var files: LinearLayout
    private lateinit var transfer: TextView
    private lateinit var start: TextView
    private lateinit var stop: TextView
    private var lastList = "__initial__"
    private var pendingInstall: Pair<Uri, String>? = null
    private val handler = Handler(Looper.getMainLooper())
    private val timer = object : Runnable {
        override fun run() {
            refresh()
            handler.postDelayed(this, 2000)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val savedUri = savedInstanceState?.getString("pending_apk_uri")
        val savedName = savedInstanceState?.getString("pending_apk_name")
        if (savedUri != null && savedName != null) {
            pendingInstall = Uri.parse(savedUri) to savedName
        }
        window.statusBarColor = page
        window.navigationBarColor = page
        window.decorView.systemUiVisibility = View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR or
            View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR
        val shell = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(page)
        }
        shell.setOnApplyWindowInsetsListener { view, insets ->
            val top: Int
            val bottom: Int
            if (Build.VERSION.SDK_INT >= 30) {
                val bars = insets.getInsets(WindowInsets.Type.systemBars())
                top = bars.top
                bottom = bars.bottom
            } else {
                        top = insets.systemWindowInsetTop
                        bottom = insets.systemWindowInsetBottom
            }
            view.setPadding(dp(18), top + dp(12), dp(18), bottom + dp(12))
            insets
        }
        val scroller = ScrollView(this).apply { isFillViewport = true }
        val content = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        scroller.addView(content)
        shell.addView(scroller, LinearLayout.LayoutParams(-1, -1))
        setContentView(shell)
        header(content)
        gap(content, 18)
        receiverCard(content)
        gap(content, 12)
        addressesCard(content)
        gap(content, 12)
        tokenCard(content)
        gap(content, 12)
        controls(content)
        gap(content, 18)
        historyCard(content)
        gap(content, 16)
        label(content, "Папка: Download/Synchro/Debug", 12f, muted)
        gap(content, 6)
        label(content, "Для доступу через інтернет використовуй Tailscale. Не відкривай порт 8765 назовні.", 12f, muted)
        gap(content, 25)
    }

    private fun header(parent: LinearLayout) {
        val row = horizontal(parent)
        val heading = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        label(heading, "Synchro  ·  Android", 25f, ink, true)
        gap(heading, 5)
        label(heading, "Приймай APK та інші файли напряму з ПК.", 13f, muted)
        row.addView(heading, LinearLayout.LayoutParams(0, -2, 1f))
        row.addView(button("⚙", false, true) {
            startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS).apply {
                data = Uri.parse("package:" + packageName)
            })
        })
    }

    private fun receiverCard(parent: LinearLayout) {
        statusCard = panel(parent, Color.rgb(235, 249, 243), Color.rgb(212, 236, 226))
        val row = horizontal(statusCard)
        label(row, "●", 22f, green)
        widthGap(row, 10)
        val column = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        status = label(column, "Перевірка приймача…", 17f, ink, true)
        detail = label(column, "Перевіряємо стан", 12f, muted)
        row.addView(column, LinearLayout.LayoutParams(0, -2, 1f))
        transfer = label(statusCard, "", 12f, muted)
    }

    private fun addressesCard(parent: LinearLayout) {
        val card = panel(parent, Color.WHITE, border)
        label(card, "АДРЕСИ ПРИСТРОЮ", 11f, muted, true)
        gap(card, 12)
        val addr = addresses()
        if (addr.isEmpty()) {
            label(card, "Немає активної IPv4-адреси. Увімкни Wi-Fi або Tailscale.", 13f, muted)
        }
        for ((index, pair) in addr.withIndex()) {
            if (index > 0) gap(card, 9)
            val name = pair.first
            val ip = pair.second
            val isTail = name.contains("tail", true) || ip.startsWith("100.")
            val kind = when {
                isTail -> "◈  Tailscale"
                name.startsWith("wlan") -> "⌁  Wi-Fi"
                ip.startsWith("10.0.2.") -> "▣  Emulator"
                else -> "⌁  " + name
            }
            val url = "http://" + ip + ":" + SynchroConfig.PORT
            val row = horizontal(card)
            val v = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
            label(v, kind, 12f, muted)
            label(v, url, 14f, blue, true).setTextIsSelectable(true)
            row.addView(v, LinearLayout.LayoutParams(0, -2, 1f))
            row.addView(button("⧉", false, true) { copy("Synchro address", url) })
            if (ip.startsWith("10.0.2.")) {
                gap(card, 5)
                label(card, "Емулятор: на Windows виконай adb forward tcp:8765 tcp:8765, " +
                    "потім введи http://127.0.0.1:8765.", 11f, muted)
            }
        }
    }

    private fun tokenCard(parent: LinearLayout) {
        val card = panel(parent, Color.WHITE, border)
        label(card, "КОД СПОЛУЧЕННЯ", 11f, muted, true)
        gap(card, 10)
        val secret = SynchroConfig.token(this)
        val row = horizontal(card)
        label(row, secret, 13f, ink, true).setTextIsSelectable(true)
        row.addView(button("⧉", false, true) { copy("Synchro token", secret) })
        gap(card, 8)
        label(card, "Секретний код для ПК. Не надсилай його стороннім людям.", 11f, muted)
        gap(card, 12)
        card.addView(button("⧉  Скопіювати всі дані підключення", true) {
            val ips = addresses().joinToString("\n") {
                it.first + ": http://" + it.second + ":" + SynchroConfig.PORT
            }
            copy("Synchro pairing", ips + "\nToken: " + secret)
        }, LinearLayout.LayoutParams(-1, -2))
    }

    private fun controls(parent: LinearLayout) {
        val card = panel(parent, Color.WHITE, border)
        start = button("▶  Почати приймання", true) {
            try {
                startForegroundService(Intent(this, ReceiverService::class.java))
                status.text = "Запускаємо приймач…"
                askNotifications()
                handler.postDelayed({ refresh() }, 500)
            } catch (e: Exception) {
                toast("Не вдалося запустити: " + e.message)
            }
        }
        card.addView(start, LinearLayout.LayoutParams(-1, -2))
        gap(card, 9)
        stop = button("■  Зупинити приймання", false) {
            stopService(Intent(this, ReceiverService::class.java))
            handler.postDelayed({ refresh() }, 150)
        }
        card.addView(stop, LinearLayout.LayoutParams(-1, -2))
        gap(card, 9)
        card.addView(button("↻  Оновити отримані файли", false) {
            lastList = ""
            refresh()
            toast("Список оновлено")
        }, LinearLayout.LayoutParams(-1, -2))
    }

    private fun historyCard(parent: LinearLayout) {
        val card = panel(parent, Color.WHITE, border)
        val row = horizontal(card)
        historyTitle = label(row, "Отримані файли", 18f, ink, true)
        row.addView(button("↻", false, true) {
            lastList = ""
            refresh()
        })
        gap(card, 12)
        files = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        card.addView(files)
    }

    private data class Received(val name: String, val size: Long, val timestamp: Long, val uri: Uri)

    private fun queryFiles(): List<Received> {
        val result = mutableListOf<Received>()
        val cols = arrayOf(MediaStore.MediaColumns._ID, MediaStore.MediaColumns.DISPLAY_NAME,
            MediaStore.MediaColumns.SIZE, MediaStore.MediaColumns.DATE_ADDED)
        contentResolver.query(MediaStore.Downloads.EXTERNAL_CONTENT_URI, cols,
            MediaStore.MediaColumns.RELATIVE_PATH + "=? AND " +
                MediaStore.MediaColumns.IS_PENDING + "=0",
            arrayOf(SynchroConfig.FOLDER), MediaStore.MediaColumns.DATE_ADDED + " DESC"
        )?.use { cursor ->
            val iId = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns._ID)
            val iName = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.DISPLAY_NAME)
            val iSize = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.SIZE)
            val iDate = cursor.getColumnIndexOrThrow(MediaStore.MediaColumns.DATE_ADDED)
            while (cursor.moveToNext() && result.size < 50) {
                val name = cursor.getString(iName) ?: continue
                val uri = ContentUris.withAppendedId(MediaStore.Downloads.EXTERNAL_CONTENT_URI,
                    cursor.getLong(iId))
                result.add(Received(name, cursor.getLong(iSize),
                    cursor.getLong(iDate) * 1000, uri))
            }
        }
        return result
    }

    private fun refresh() {
        val running = ReceiverService.running
        status.text = if (running) "Готовий приймати файли" else "Приймач зупинено"
        detail.text = if (running) "Пристрій доступний у мережі · порт 8765"
            else ReceiverService.lastError?.let { "Помилка запуску: " + it }
                ?: "Натисни «Почати приймання»"
        statusCard.background = background(if (running) Color.rgb(234, 250, 242) else page,
            if (running) Color.rgb(210, 239, 223) else border)
        start.isEnabled = !running
        start.alpha = if (running) 0.55f else 1f
        stop.isEnabled = running
        stop.alpha = if (running) 1f else 0.55f
        val done = ReceiverService.bytesReceived
        val total = ReceiverService.bytesTotal
        transfer.text = when {
            running && total > 0 -> "Передавання: " + formatSize(done) + " / " + formatSize(total)
            ReceiverService.lastTransferError != null ->
                "Помилка останньої передачі: " + ReceiverService.lastTransferError
            else -> ""
        }
        try {
            val list = queryFiles()
            val signature = list.joinToString("|") { it.uri.toString() + it.size + it.timestamp }
            if (signature != lastList) {
                lastList = signature
                showFiles(list)
            }
        } catch (e: Exception) {
            lastList = ""
            files.removeAllViews()
            label(files, "Помилка списку: " + e.message, 12f, muted)
        }
    }

    private fun showFiles(items: List<Received>) {
        files.removeAllViews()
        historyTitle.text = "Отримані файли  ·  " + items.size
        if (items.isEmpty()) {
            label(files, "Поки порожньо. Надішли перший APK із Windows.", 13f, muted)
            return
        }
        for (item in items) {
            val apk = item.name.lowercase(Locale.ROOT).endsWith(".apk")
            val row = horizontal(files)
            val glyph = label(row, if (apk) "◆" else "▧", 22f, if (apk) green else blue)
            glyph.gravity = Gravity.CENTER
            glyph.background = background(if (apk) Color.rgb(226, 249, 237) else paleBlue, border, 10)
            glyph.layoutParams = LinearLayout.LayoutParams(dp(42), dp(42))
            widthGap(row, 9)
            val column = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
            label(column, item.name, 13f, ink, true).apply {
                maxLines = 2
                ellipsize = android.text.TextUtils.TruncateAt.END
            }
            val date = DateFormat.getDateTimeInstance(DateFormat.SHORT, DateFormat.SHORT)
                .format(Date(item.timestamp))
            label(column, formatSize(item.size) + "  ·  " + date, 11f, muted)
            row.addView(column, LinearLayout.LayoutParams(0, -2, 1f))
            row.addView(button(if (apk) "Install" else "Open", false, true) {
                openFile(item.uri, item.name)
            })
            gap(files, 12)
        }
    }

    private fun openFile(uri: Uri, filename: String) {
        val apk = filename.lowercase(Locale.ROOT).endsWith(".apk")
        if (apk && !packageManager.canRequestPackageInstalls()) {
            pendingInstall = uri to filename
            startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES).apply {
                data = Uri.parse("package:" + packageName)
            })
            toast("Дозволь Synchro встановлювати APK, потім повернись.")
            return
        }
        try {
            val mime = if (apk) "application/vnd.android.package-archive"
                else contentResolver.getType(uri) ?: "application/octet-stream"
            startActivity(Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, mime)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            })
        } catch (e: Exception) {
            toast("Не вдалося відкрити: " + e.message)
        }
    }

    override fun onSaveInstanceState(outState: Bundle) {
        pendingInstall?.let {
            outState.putString("pending_apk_uri", it.first.toString())
            outState.putString("pending_apk_name", it.second)
        }
        super.onSaveInstanceState(outState)
    }

    override fun onResume() {
        super.onResume()
        handler.removeCallbacks(timer)
        handler.post(timer)
        val pending = pendingInstall
        if (pending != null && packageManager.canRequestPackageInstalls()) {
            pendingInstall = null
            handler.postDelayed({ openFile(pending.first, pending.second) }, 350)
        }
    }

    override fun onPause() {
        handler.removeCallbacks(timer)
        super.onPause()
    }

    private fun askNotifications() {
        if (Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 101)
        }
    }

    private fun addresses(): List<Pair<String, String>> {
        return try {
            (NetworkInterface.getNetworkInterfaces()?.toList() ?: emptyList())
                .filter { it.isUp && !it.isLoopback }.flatMap { net ->
                    net.inetAddresses.toList().filterIsInstance<Inet4Address>()
                        .filter { !it.isLoopbackAddress }.map { net.name to it.hostAddress.orEmpty() }
                }.filter { it.second.isNotEmpty() }
                .sortedBy {
                    when {
                        it.first.startsWith("wlan") -> 0
                        it.second.startsWith("100.") -> 1
                        else -> 2
                    }
                }
        } catch (_: Exception) { emptyList() }
    }

    private fun copy(title: String, value: String) {
        getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText(title, value))
        toast("Скопійовано")
    }
    private fun toast(msg: String) { Toast.makeText(this, msg, Toast.LENGTH_SHORT).show() }
    private fun dp(v: Int) = (v * resources.displayMetrics.density).toInt()
    private fun formatSize(n: Long) = when {
        n >= 1_048_576 -> String.format(Locale.US, "%.1f MB", n.toDouble() / 1_048_576)
        n >= 1024 -> String.format(Locale.US, "%.0f KB", n.toDouble() / 1024)
        else -> n.toString() + " B"
    }
    private fun background(fill: Int, stroke: Int, radius: Int = 15): GradientDrawable =
        GradientDrawable().apply {
            setColor(fill)
            cornerRadius = dp(radius).toFloat()
            setStroke(dp(1), stroke)
        }
    private fun gap(p: LinearLayout, height: Int) {
        p.addView(View(this), LinearLayout.LayoutParams(1, dp(height)))
    }
    private fun widthGap(p: LinearLayout, width: Int) {
        p.addView(View(this), LinearLayout.LayoutParams(dp(width), 1))
    }
    private fun horizontal(p: LinearLayout): LinearLayout {
        val row = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        p.addView(row, LinearLayout.LayoutParams(-1, -2))
        return row
    }
    private fun panel(p: LinearLayout, fill: Int, stroke: Int): LinearLayout {
        val card = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            background = background(fill, stroke)
            setPadding(dp(15), dp(15), dp(15), dp(15))
        }
        p.addView(card, LinearLayout.LayoutParams(-1, -2))
        return card
    }
    private fun label(p: LinearLayout, value: String, size: Float, color: Int,
        bold: Boolean = false): TextView {
        val view = TextView(this).apply {
            text = value
            textSize = size
            setTextColor(color)
            if (bold) typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            includeFontPadding = false
        }
        p.addView(view)
        return view
    }
    private fun button(value: String, primary: Boolean, compact: Boolean = false,
        click: () -> Unit): TextView = TextView(this).apply {
        text = value
        textSize = if (compact) 12f else 14f
        gravity = Gravity.CENTER
        typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
        setTextColor(if (primary) Color.WHITE else blue)
        background = background(if (primary) blue else paleBlue, if (primary) blue else paleBlue, 10)
        setPadding(dp(if (compact) 11 else 16), dp(if (compact) 10 else 13),
            dp(if (compact) 11 else 16), dp(if (compact) 10 else 13))
        isClickable = true
        isFocusable = true
        setOnClickListener { click() }
    }
}
