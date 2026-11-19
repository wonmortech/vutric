package io.wonmor.vutric_spike.w6

import android.os.ParcelFileDescriptor
import java.net.Socket

/**
 * Where the connected sockets of Wi-Fi Aware (R6) and Wi-Fi Direct (R7) go (ADR-0131). The link modules make a
 * connected TCP stream on a network iroh cannot open by itself; Rust then runs the normal Vutric handshake over it.
 *
 * The descriptor is detached from Java, so whoever takes it owns it and must close it.
 */
object LinkSockets {
    /** Takes ownership of [fd]; [link] is `aware` or `direct`, [role] `dialer` or `listener`. */
    fun interface Sink {
        fun take(link: String, role: String, fd: Int)
    }

    /**
     * TEMP until W3 #23: `vutric-net` cannot run a session over a socket yet, so the default sink closes the socket
     * and tells Dart `needs_w3`, which keeps descriptors from leaking and lets the UI say "not ready yet".
     */
    val closeAndReport = Sink { link, role, fd ->
        runCatching { ParcelFileDescriptor.adoptFd(fd).close() }
        W6.emit(mapOf("kind" to "w6.link", "link" to link, "role" to role, "state" to "needs_w3"))
    }

    /**
     * For W1 once W3 #23 lands: Dart gets the descriptor in the event and passes it to the bridge
     * (`attach_link_fd`), which owns it from then on.
     */
    val toDart = Sink { link, role, fd ->
        W6.emit(mapOf("kind" to "w6.link", "link" to link, "role" to role, "state" to "connected", "fd" to fd))
    }

    @Volatile var sink: Sink = closeAndReport

    /** Detaches [socket]'s descriptor and hands it on; the Java socket is closed, the duplicate lives on. */
    fun hand(link: String, role: String, socket: Socket) {
        val fd = try {
            ParcelFileDescriptor.fromSocket(socket).detachFd()
        } catch (_: Exception) {
            runCatching { socket.close() }
            W6.emit(mapOf("kind" to "w6.link", "link" to link, "role" to role, "state" to "failed", "reason" to "fd"))
            return
        }
        runCatching { socket.close() }
        sink.take(link, role, fd)
    }
}
