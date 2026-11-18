package io.wonmor.vutric_spike

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import java.io.File

/**
 * ShareEntry (ADR-0009 part 2, backlog A3): Vutric in every Share menu. Turns an ACTION_SEND / SEND_MULTIPLE
 * intent into real files the Rust sender can open. A content:// item has no path of its own, so it is streamed once
 * into the app cache (`cache/shared/<n>/<name>`); the cache folder is cleared at the next share.
 *
 * Holds no state: the result goes to Flutter, which adds the files to the send selection.
 */
object ShareEntry {
    fun isShare(intent: Intent?): Boolean =
        intent?.action == Intent.ACTION_SEND || intent?.action == Intent.ACTION_SEND_MULTIPLE

    /** `[{path, name}]` for every stream in the intent, or an empty list for plain text or an unreadable item. */
    fun files(context: Context, intent: Intent): List<Map<String, String>> {
        val uris = uris(intent)
        if (uris.isEmpty()) return emptyList()
        val root = File(context.cacheDir, "shared").apply { deleteRecursively(); mkdirs() }
        val out = mutableListOf<Map<String, String>>()
        uris.forEachIndexed { i, uri ->
            try {
                val name = safeName(displayName(context, uri) ?: "fayl-${i + 1}")
                val dest = File(File(root, "$i").apply { mkdirs() }, name)
                val copied = context.contentResolver.openInputStream(uri)?.use { input ->
                    dest.outputStream().use { input.copyTo(it, 1 shl 16) }
                }
                if (copied != null) out += mapOf("path" to dest.absolutePath, "name" to name)
            } catch (_: Exception) {
                // One unreadable item (revoked grant, cloud file offline) does not drop the rest.
            }
        }
        return out
    }

    @Suppress("DEPRECATION")
    private fun uris(intent: Intent): List<Uri> = when (intent.action) {
        Intent.ACTION_SEND -> listOfNotNull(
            if (Build.VERSION.SDK_INT >= 33) intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
            else intent.getParcelableExtra(Intent.EXTRA_STREAM),
        )
        Intent.ACTION_SEND_MULTIPLE ->
            (if (Build.VERSION.SDK_INT >= 33) intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
            else intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM)) ?: emptyList()
        else -> emptyList()
    }

    private fun displayName(context: Context, uri: Uri): String? {
        if (uri.scheme == "content") {
            context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                if (c.moveToFirst()) {
                    val i = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                    if (i >= 0) c.getString(i)?.let { return it }
                }
            }
        }
        return uri.lastPathSegment
    }

    /** A name that cannot climb out of its folder; the receiver sanitises again (vutric-core file-name safety). */
    private fun safeName(name: String): String =
        name.replace(Regex("""[\\/:*?"<>|\x00-\x1f]"""), "_").trim().trimStart('.').ifEmpty { "fayl" }.take(200)
}
