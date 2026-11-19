//! The IPC server: accepts desktop windows on the per-user pipe and answers [`Request`]s.
//!
//! The pipe is created with `first_pipe_instance` (another process cannot squat our name before us) and refuses
//! remote clients; the per-user name keeps other users' sessions apart.

use std::sync::Arc;

use anyhow::Result;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio::sync::broadcast;

use crate::ipc::{self, Event, LineBuffer, Reply, Request};

/// What the server needs from the rest of the daemon. Implemented by the daemon state; mocked in tests.
pub trait Handler: Send + Sync + 'static {
    fn handle(&self, req: Request) -> Reply;
    fn events(&self) -> broadcast::Receiver<Event>;
    /// A connection closed; `subscribed` when it had asked for events (the desktop window or the macOS agent).
    fn closed(&self, subscribed: bool) {
        let _ = subscribed;
    }
}

/// Serves one connection until it closes, then tells the handler. Generic over the stream so tests use an
/// in-memory duplex.
pub async fn serve_conn<S, H>(stream: S, h: Arc<H>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
    H: Handler + ?Sized,
{
    let mut subscribed = false;
    let r = conn_loop(stream, &h, &mut subscribed).await;
    h.closed(subscribed);
    r
}

async fn conn_loop<S, H>(stream: S, h: &Arc<H>, subscribed: &mut bool) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
    H: Handler + ?Sized,
{
    let (mut rd, mut wr) = tokio::io::split(stream);
    let mut lines = LineBuffer::default();
    let mut buf = vec![0u8; 16 * 1024];
    let mut events: Option<broadcast::Receiver<Event>> = None;
    let mut greeted = false;
    loop {
        let n = tokio::select! {
            n = rd.read(&mut buf) => n?,
            ev = async {
                match events.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match ev {
                    Ok(ev) => wr.write_all(&ipc::encode(&Reply::Event(ev))).await?,
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => events = None,
                }
                continue;
            }
        };
        if n == 0 {
            return Ok(());
        }
        for line in lines.push(&buf[..n]) {
            let reply = match line.and_then(|l| ipc::decode::<Request>(&l)) {
                Err(e) => Reply::Error {
                    message: e.to_string(),
                },
                Ok(Request::Hello { version }) => {
                    greeted = version == ipc::VERSION;
                    if greeted {
                        Reply::Hello {
                            version: ipc::VERSION,
                            daemon: env!("CARGO_PKG_VERSION").into(),
                        }
                    } else {
                        Reply::Error {
                            message: format!(
                                "protocol {version} unsupported, daemon speaks {}",
                                ipc::VERSION
                            ),
                        }
                    }
                }
                Ok(_) if !greeted => Reply::Error {
                    message: "say hello first".into(),
                },
                Ok(Request::Subscribe) => {
                    events = Some(h.events());
                    *subscribed = true;
                    Reply::Ok
                }
                Ok(req) => h.handle(req),
            };
            wr.write_all(&ipc::encode(&reply)).await?;
        }
    }
}

/// Listens on the per-user named pipe forever.
#[cfg(windows)]
pub async fn listen<H: Handler>(name: &str, h: Arc<H>) -> Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(name)?;
    loop {
        server.connect().await?;
        let conn = server;
        server = ServerOptions::new()
            .reject_remote_clients(true)
            .create(name)?;
        let h = Arc::clone(&h);
        tokio::spawn(async move {
            let _ = serve_conn(conn, h).await;
        });
    }
}

/// Listens on the per-user Unix socket ([`ipc::socket_path`]).
#[cfg(unix)]
pub async fn listen<H: Handler>(name: &str, h: Arc<H>) -> Result<()> {
    let path = ipc::socket_path(name)?;
    let _ = std::fs::remove_file(&path);
    let l = tokio::net::UnixListener::bind(&path)?;
    loop {
        let (conn, _) = l.accept().await?;
        let h = Arc::clone(&h);
        tokio::spawn(async move {
            let _ = serve_conn(conn, h).await;
        });
    }
}

/// Sends one request to a running daemon and returns its reply. `Err` when no daemon is listening.
#[cfg(windows)]
pub async fn call(name: &str, req: &Request) -> Result<Reply> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let pipe = ClientOptions::new().open(name)?;
    exchange(pipe, req).await
}

#[cfg(unix)]
pub async fn call(name: &str, req: &Request) -> Result<Reply> {
    let s = tokio::net::UnixStream::connect(ipc::socket_path(name)?).await?;
    exchange(s, req).await
}

async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(s: S, req: &Request) -> Result<Reply> {
    let (mut rd, mut wr) = tokio::io::split(s);
    wr.write_all(&ipc::encode(&Request::Hello {
        version: ipc::VERSION,
    }))
    .await?;
    wr.write_all(&ipc::encode(req)).await?;
    let mut lines = LineBuffer::default();
    let mut buf = vec![0u8; 16 * 1024];
    let mut seen = 0;
    loop {
        let n = rd.read(&mut buf).await?;
        anyhow::ensure!(n > 0, "daemon closed the pipe");
        for line in lines.push(&buf[..n]) {
            let reply: Reply = ipc::decode(&line?)?;
            seen += 1;
            if seen == 2 {
                return Ok(reply);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::ipc::Status;

    struct Fake {
        tx: broadcast::Sender<Event>,
        paused: Mutex<bool>,
    }

    impl Handler for Fake {
        fn handle(&self, req: Request) -> Reply {
            match req {
                Request::Pause { paused } => {
                    *self.paused.lock().unwrap() = paused;
                    let _ = self.tx.send(Event::Paused { paused });
                    Reply::Ok
                }
                Request::Status => Reply::Status(Status {
                    paused: *self.paused.lock().unwrap(),
                    ..Status::default()
                }),
                _ => Reply::Error {
                    message: "unsupported".into(),
                },
            }
        }
        fn events(&self) -> broadcast::Receiver<Event> {
            self.tx.subscribe()
        }
    }

    async fn read_replies<R: AsyncRead + Unpin>(rd: &mut R, n: usize) -> Vec<Reply> {
        let mut lines = LineBuffer::default();
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        while out.len() < n {
            let k = rd.read(&mut buf).await.unwrap();
            assert!(k > 0, "server closed early");
            for l in lines.push(&buf[..k]) {
                out.push(ipc::decode(&l.unwrap()).unwrap());
            }
        }
        out
    }

    #[tokio::test]
    async fn hello_then_requests_then_events() {
        let (tx, _) = broadcast::channel(8);
        let h = Arc::new(Fake {
            tx,
            paused: Mutex::new(false),
        });
        let (client, server) = tokio::io::duplex(64 * 1024);
        let task = tokio::spawn(serve_conn(server, Arc::clone(&h)));
        let (mut rd, mut wr) = tokio::io::split(client);

        wr.write_all(&ipc::encode(&Request::Status)).await.unwrap();
        let r = read_replies(&mut rd, 1).await;
        assert!(matches!(&r[0], Reply::Error { message } if message.contains("hello")));

        let mut batch = ipc::encode(&Request::Hello {
            version: ipc::VERSION,
        });
        batch.extend(ipc::encode(&Request::Subscribe));
        batch.extend(ipc::encode(&Request::Pause { paused: true }));
        batch.extend(ipc::encode(&Request::Status));
        wr.write_all(&batch).await.unwrap();
        let r = read_replies(&mut rd, 5).await;
        assert!(matches!(r[0], Reply::Hello { version: 1, .. }));
        assert_eq!(r[1], Reply::Ok);
        let rest = &r[2..];
        assert!(rest.contains(&Reply::Ok));
        assert!(rest.contains(&Reply::Event(Event::Paused { paused: true })));
        assert!(
            rest.iter()
                .any(|x| matches!(x, Reply::Status(s) if s.paused))
        );

        drop(wr);
        drop(rd);
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn wrong_version_and_bad_lines_do_not_kill_the_connection() {
        let (tx, _) = broadcast::channel(8);
        let h = Arc::new(Fake {
            tx,
            paused: Mutex::new(false),
        });
        let (client, server) = tokio::io::duplex(1 << 20);
        tokio::spawn(serve_conn(server, h));
        let (mut rd, mut wr) = tokio::io::split(client);
        let mut batch = ipc::encode(&Request::Hello { version: 99 });
        batch.extend(b"not json\n");
        batch.extend(vec![b'x'; ipc::MAX_LINE + 10]);
        batch.extend(b"\n");
        batch.extend(ipc::encode(&Request::Hello {
            version: ipc::VERSION,
        }));
        wr.write_all(&batch).await.unwrap();
        let r = read_replies(&mut rd, 4).await;
        assert!(matches!(&r[0], Reply::Error { message } if message.contains("99")));
        assert!(matches!(r[1], Reply::Error { .. }));
        assert!(matches!(&r[2], Reply::Error { message } if message.contains("longer")));
        assert!(matches!(r[3], Reply::Hello { .. }));
    }
}
