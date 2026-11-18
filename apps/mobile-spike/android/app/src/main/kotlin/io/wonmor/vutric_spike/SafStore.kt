package io.wonmor.vutric_spike

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import android.webkit.MimeTypeMap
import java.io.File

/**
 * SafStore (ADR-0009 part 2, backlog A8): save received files into any folder the user picks (SD card, a cloud
 * provider's folder, Download/…), which a plain path cannot reach on Android 11+ (the limit noted in ADR-0008).
 *
 * Rust always writes into the app's own folder first and checks every file there; only then does [moveInto] copy
 * the finished files into the picked tree and delete the originals. A half-written file never lands in the user's
 * folder. Holds no state: the tree URI is kept by the caller, the read/write grant by Android (persisted).
 */
object SafStore {
    const val REQUEST_TREE = 4711

    @Suppress("DEPRECATION")
    fun pickTree(activity: Activity) {
        val i = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).addFlags(
            Intent.FLAG_GRANT_READ_URI_PERMISSION or
                Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
                Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION,
        )
        activity.startActivityForResult(i, REQUEST_TREE)
    }

    /** Keeps the grant across reboots; returns the tree URI and a name to show, or null when the user backed out. */
    fun onPicked(context: Context, data: Intent?): Map<String, String>? {
        val tree = data?.data ?: return null
        val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
        context.contentResolver.takePersistableUriPermission(tree, flags)
        val label = DocumentsContract.getTreeDocumentId(tree).substringAfter(':').ifEmpty { "Tanlangan papka" }
        return mapOf("uri" to tree.toString(), "label" to label)
    }

    /** Whether the grant for [uri] is still held (the user may revoke it in system settings). */
    fun usable(context: Context, uri: String): Boolean =
        context.contentResolver.persistedUriPermissions.any { it.uri.toString() == uri && it.isWritePermission }

    fun release(context: Context, uri: String) {
        val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
        runCatching { context.contentResolver.releasePersistableUriPermission(Uri.parse(uri), flags) }
    }

    /**
     * Copies each finished file into the tree and deletes the local copy after a full, size-checked copy.
     * Returns how many moved; a file that fails stays where Rust saved it, so nothing is ever lost.
     */
    fun moveInto(context: Context, uri: String, paths: List<String>): Int {
        val tree = Uri.parse(uri)
        val parent = DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree))
        var moved = 0
        for (p in paths) {
            val src = File(p)
            if (!src.isFile) continue
            try {
                val mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(src.extension.lowercase())
                    ?: "application/octet-stream"
                // The provider picks a free name ("photo (1).jpg") when one is taken.
                val doc = DocumentsContract.createDocument(context.contentResolver, parent, mime, src.name) ?: continue
                val written = context.contentResolver.openOutputStream(doc, "w")?.use { out ->
                    src.inputStream().use { it.copyTo(out, 1 shl 16) }
                } ?: -1L
                if (written == src.length()) {
                    src.delete()
                    moved++
                } else {
                    DocumentsContract.deleteDocument(context.contentResolver, doc)
                }
            } catch (_: Exception) {
                // Left in the app folder; the history entry still points at it.
            }
        }
        return moved
    }
}
