//! The daemon side: starts the adapter process with low privileges, sends it commands, and reads its events
//! defensively (ADR-0010 §3.4, ADR-0054).
//!
//! Limits on the adapter process:
//! - Windows: a job object that kills it with the daemon, caps its memory at 512 MiB and forbids child processes;
//! - Linux: `no_new_privs`, killed when the daemon dies, no core dumps;
//! - other Unix: no core dumps;
//! - everywhere: no inherited handles besides its pipes, and every path it reports must be a plain file directly
//!   inside the staging folder ([`checked_path`]).
//!
//! On Android the adapters cannot be a child process; the app starts them in its own `:compat` process
//! ([`crate::serve::serve_socket_fd`]) and talks to them through [`Adapter::attach`] on the other end of a socket
//! pair (ADR-0066). The same defensive reading and [`checked_path`] apply.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context as _, Result, bail};
use tokio::io::{AsyncBufReadExt as _, AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use crate::ipc::{self, FromAdapter, Start, ToAdapter};

/// Memory cap of the adapter process (Windows job object).
pub const MEMORY_LIMIT: usize = 512 * 1024 * 1024;

/// A running adapter: a child process ([`Adapter::spawn`]; dropping it kills the process) or a loop in another
/// process reached over a stream ([`Adapter::attach`]).
pub struct Adapter {
    child: Option<Child>,
    stdin: Box<dyn AsyncWrite + Send + Unpin>,
    staging: PathBuf,
    #[cfg(windows)]
    _job: Option<Job>,
}

impl std::fmt::Debug for Adapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Adapter")
            .field("child", &self.child)
            .field("staging", &self.staging)
            .finish_non_exhaustive()
    }
}

/// Whether `path`, reported by the adapter, is a file name directly inside `staging` (no `..`, no other
/// folder, no link out). The daemon moves only such files.
#[must_use]
pub fn checked_path(staging: &Path, path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    if vutric_core::filename::sanitize(name) != name {
        return None;
    }
    let real_staging = std::fs::canonicalize(staging).ok()?;
    let real = std::fs::canonicalize(path).ok()?;
    let meta = std::fs::symlink_metadata(path).ok()?;
    (real.parent() == Some(real_staging.as_path()) && meta.file_type().is_file()).then_some(real)
}

impl Adapter {
    /// Starts `exe` (the `vutric-compat` binary) and sends it `start`. Events arrive on the channel;
    /// it closes when the process exits.
    pub async fn spawn(exe: &Path, start: Start) -> Result<(Self, mpsc::Receiver<FromAdapter>)> {
        let staging = start.staging.clone();
        std::fs::create_dir_all(&staging)?;
        let mut cmd = Command::new(exe);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .env_clear();
        // Winsock does not start without SystemRoot (WSAEPROVIDERFAILEDINIT, 10106).
        #[cfg(windows)]
        if let Some(root) = std::env::var_os("SystemRoot") {
            cmd.env("SystemRoot", root);
        }
        #[cfg(unix)]
        restrict_unix(&mut cmd);
        let mut child = cmd
            .spawn()
            .with_context(|| format!("start {}", exe.display()))?;
        #[cfg(windows)]
        let job = Job::confine(&child)?;
        let stdin = child.stdin.take().context("adapter stdin")?;
        let stdout = child.stdout.take().context("adapter stdout")?;
        let mut me = Self {
            child: Some(child),
            stdin: Box::new(stdin),
            staging,
            #[cfg(windows)]
            _job: Some(job),
        };
        let rx = me.begin(stdout, start).await?;
        Ok((me, rx))
    }

    /// Talks to an adapter loop that already runs elsewhere ([`crate::serve::serve`]), over `reader` and
    /// `writer`: on Android, the app process's end of the socket pair whose other end the `:compat` process
    /// serves (ADR-0066). Sends `start` first. The adapter is as untrusted as a spawned one: lines are capped and
    /// parsed into closed enums, and received files go through [`checked_path`].
    pub async fn attach<R, W>(
        reader: R,
        writer: W,
        start: Start,
    ) -> Result<(Self, mpsc::Receiver<FromAdapter>)>
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let staging = start.staging.clone();
        std::fs::create_dir_all(&staging)?;
        let mut me = Self {
            child: None,
            stdin: Box::new(writer),
            staging,
            #[cfg(windows)]
            _job: None,
        };
        let rx = me.begin(reader, start).await?;
        Ok((me, rx))
    }

    /// [`Self::attach`] on a connected Unix stream socket: the app process's end of the Android socket pair whose
    /// other end went to the `:compat` service (ADR-0066). Call inside a Tokio runtime.
    ///
    /// # Safety
    ///
    /// `fd` must be an open, connected stream socket that nothing else uses or closes; this takes ownership.
    #[cfg(unix)]
    pub async unsafe fn attach_socket_fd(
        fd: std::os::fd::RawFd,
        start: Start,
    ) -> Result<(Self, mpsc::Receiver<FromAdapter>)> {
        use std::os::fd::FromRawFd as _;
        // SAFETY: the caller hands over an open, connected stream socket it no longer uses.
        let std = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
        std.set_nonblocking(true)?;
        let (r, w) = tokio::net::UnixStream::from_std(std)?.into_split();
        Self::attach(r, w, start).await
    }

    async fn begin<R: AsyncRead + Unpin + Send + 'static>(
        &mut self,
        out: R,
        start: Start,
    ) -> Result<mpsc::Receiver<FromAdapter>> {
        self.tell(&ToAdapter::Start(start)).await?;
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let mut lines = BufReader::with_capacity(64 * 1024, out);
            let mut buf = Vec::new();
            // A line longer than MAX_LINE, or the end of output, ends the adapter's turn to talk.
            while let Ok(true) = read_line(&mut lines, &mut buf, ipc::MAX_LINE).await {
                if let Some(ev) = ipc::parse::<FromAdapter>(buf.trim_ascii_end())
                    && tx.send(ev).await.is_err()
                {
                    break;
                }
            }
        });
        Ok(rx)
    }

    async fn tell(&mut self, m: &ToAdapter) -> Result<()> {
        self.stdin
            .write_all(&ipc::line(m))
            .await
            .context("adapter gone")?;
        self.stdin.flush().await.context("adapter gone")
    }

    /// Sends a file, text or link to a paired KDE Connect device; the answer is a `FromAdapter` sent/failed event
    /// with `request_id`.
    pub async fn kde_send(&mut self, cmd: ToAdapter) -> Result<()> {
        if !matches!(cmd, ToAdapter::KdeSend { .. }) {
            bail!("not a KDE Connect send");
        }
        self.tell(&cmd).await
    }

    /// Starts a magic-wormhole send or receive (`ToAdapter::WormholeSend` / `WormholeReceive`, ADR-0067).
    pub async fn wormhole(&mut self, cmd: ToAdapter) -> Result<()> {
        if !matches!(
            cmd,
            ToAdapter::WormholeSend { .. } | ToAdapter::WormholeReceive { .. }
        ) {
            bail!("not a magic-wormhole command");
        }
        self.tell(&cmd).await
    }

    pub async fn decide(&mut self, session_id: &str, accept: Vec<String>) -> Result<()> {
        self.tell(&ToAdapter::Decide {
            session_id: session_id.to_owned(),
            accept,
        })
        .await
    }

    pub async fn announce(&mut self) -> Result<()> {
        self.tell(&ToAdapter::Announce).await
    }

    pub async fn send(
        &mut self,
        request_id: u64,
        to: std::net::SocketAddr,
        https: bool,
        paths: Vec<PathBuf>,
        pin: Option<String>,
    ) -> Result<()> {
        self.tell(&ToAdapter::Send {
            request_id,
            to,
            https,
            paths,
            pin,
        })
        .await
    }

    /// Opens a browser link; the answer is [`FromAdapter::Shared`] or [`FromAdapter::ShareFailed`].
    pub async fn share(
        &mut self,
        request_id: u64,
        paths: Vec<PathBuf>,
        ttl_secs: u64,
    ) -> Result<()> {
        self.tell(&ToAdapter::Share {
            request_id,
            paths,
            ttl_secs,
        })
        .await
    }

    pub async fn unshare(&mut self, token: &str) -> Result<()> {
        self.tell(&ToAdapter::Unshare {
            token: token.to_owned(),
        })
        .await
    }

    /// The staged file of a [`FromAdapter::Received`], if its path is acceptable.
    #[must_use]
    pub fn received_file(&self, path: &Path) -> Option<PathBuf> {
        checked_path(&self.staging, path)
    }

    /// Asks the adapter to stop, then makes sure it is gone. An attached adapter is only asked; its host process
    /// (the Android service) ends when its loop returns.
    pub async fn stop(mut self) -> Result<()> {
        let _ = self.tell(&ToAdapter::Stop).await;
        let Some(child) = self.child.as_mut() else {
            let _ = self.stdin.shutdown().await;
            return Ok(());
        };
        if tokio::time::timeout(std::time::Duration::from_secs(3), child.wait())
            .await
            .is_err()
        {
            child.kill().await?;
            bail!("adapter did not stop; killed");
        }
        Ok(())
    }
}

/// Reads one line into `buf`, never holding more than `max` bytes. `Ok(false)` at the end of input or for a
/// line longer than `max`.
pub async fn read_line<R: tokio::io::AsyncBufRead + Unpin>(
    r: &mut R,
    buf: &mut Vec<u8>,
    max: usize,
) -> std::io::Result<bool> {
    buf.clear();
    loop {
        let avail = r.fill_buf().await?;
        if avail.is_empty() {
            return Ok(false);
        }
        let (take, done) = match avail.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (avail.len(), false),
        };
        if buf.len() + take > max + 1 {
            return Ok(false);
        }
        buf.extend_from_slice(&avail[..take]);
        r.consume(take);
        if done {
            return Ok(true);
        }
    }
}

#[cfg(unix)]
fn restrict_unix(cmd: &mut Command) {
    // SAFETY: only async-signal-safe calls (setrlimit, prctl) between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            let none = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            libc::setrlimit(libc::RLIMIT_CORE, &raw const none);
            #[cfg(target_os = "linux")]
            {
                libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0);
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct Job(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
unsafe impl Send for Job {}

#[cfg(windows)]
impl Job {
    fn confine(child: &Child) -> Result<Self> {
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
            JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation, SetInformationJobObject,
        };
        let raw = child.raw_handle().context("adapter process handle")?;
        // SAFETY: plain Win32 calls on handles we own; the struct is zero-initialised as the API expects.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                bail!("CreateJobObjectW failed");
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_PROCESS_MEMORY
                | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
            info.BasicLimitInformation.ActiveProcessLimit = 1;
            info.ProcessMemoryLimit = MEMORY_LIMIT;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                u32::try_from(std::mem::size_of_val(&info)).expect("small struct"),
            );
            if ok == 0 || AssignProcessToJobObject(job, raw.cast()) == 0 {
                windows_sys::Win32::Foundation::CloseHandle(job);
                bail!("could not confine the adapter process");
            }
            Ok(Self(job))
        }
    }
}

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateJobObjectW and is closed once.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
