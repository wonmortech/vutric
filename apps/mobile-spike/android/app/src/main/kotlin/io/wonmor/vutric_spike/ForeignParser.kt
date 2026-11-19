package io.wonmor.vutric_spike

import android.app.Service
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Message
import android.os.Messenger
import org.json.JSONObject

/**
 * The low-privilege parser for foreign discovery datagrams (ADR-0010 rule: pre-authentication bytes are parsed
 * only in a separate process). Declared with `android:isolatedProcess="true"`: it runs as its own throwaway user
 * with no permissions, no network and no access to Vutric's files, so a hostile LocalSend announcement can at worst
 * crash this process. It returns a handful of strings, each cut short, and nothing else.
 */
class ForeignParser : Service() {
    private val messenger = Messenger(
        Handler(Looper.getMainLooper()) { msg ->
            if (msg.what == PARSE_LOCALSEND) {
                val reply = msg.replyTo
                val out = runCatching { localSend(msg.data) }.getOrNull()
                if (reply != null && out != null) {
                    runCatching { reply.send(Message.obtain(null, PARSE_LOCALSEND).apply { data = out }) }
                }
            }
            true
        },
    )

    override fun onBind(intent: Intent): IBinder = messenger.binder

    /** LocalSend v2 announcement: `{"alias", "deviceModel", "deviceType", "fingerprint", "port", "protocol"}`. */
    private fun localSend(input: Bundle): Bundle? {
        val bytes = input.getByteArray("bytes") ?: return null
        if (bytes.size > MAX_BYTES) return null
        val j = JSONObject(String(bytes, Charsets.UTF_8))
        val fingerprint = j.optString("fingerprint").take(MAX_FIELD)
        if (fingerprint.isEmpty()) return null
        val port = j.optInt("port", 53317)
        if (port !in 1..65535) return null
        return Bundle().apply {
            putString("id", fingerprint)
            putString("name", j.optString("alias").take(MAX_FIELD))
            putString("type", j.optString("deviceType").take(MAX_FIELD))
            putString("host", input.getString("host").orEmpty().take(MAX_FIELD))
            putInt("port", port)
        }
    }

    companion object {
        const val PARSE_LOCALSEND = 1
        private const val MAX_BYTES = 4096
        private const val MAX_FIELD = 64
    }
}
