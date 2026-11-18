//! Files of any type and size, any number of them, in either direction (ADR-0006 part 3).
//!
//! Control stream, once per batch of up to [`MAX_OFFER_ENTRIES`] files:
//! `Offer` → `OfferReply` (accept, and bytes already held per file) → data streams → `OfferDone`.
//!
//! Data stream: one unidirectional QUIC stream per file, up to [`PARALLEL`] at once:
//! `id [16] | index u32 LE | offset u64 LE`, then the file's bytes from `offset`, then BLAKE3 of the whole file
//! (32 bytes). The receiver writes `<name>.vutric-part`, hashes the held prefix and the new bytes, and renames the
//! file into place only when the hash matches. An interrupted transfer resumes where its part file ends.
//!
//! Where a file lands is the user's choice ([`SaveTo`], ADR-0007 part 4): save folder, a folder per kind,
//! a subfolder per device or day, the sender's tree kept or flattened, and what to do on a name conflict.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use iroh::endpoint::{Connection, RecvStream};
use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _, AsyncWriteExt as _};
use vutric_core::message::{FailReason, MAX_OFFER_ENTRIES, OfferEntry};
use vutric_core::storage::{Arrival, Conflict, SaveRules};
use vutric_core::{FlingId, Message, filename};

use crate::Session;

/// Bytes read or written per step.
const CHUNK: usize = 1 << 20;
/// Files in flight at once.
pub const PARALLEL: usize = 4;
const HEADER: usize = 16 + 4 + 8;
const PART: &str = ".vutric-part";
/// Deepest folder tree sent from here (the receiver applies the same limit).
const MAX_DEPTH: usize = 32;
/// A transfer that moves no byte for this long is given up.
const IDLE: Duration = Duration::from_secs(60);
/// How long the sender waits for the user on the other side to accept.
const ASK: Duration = Duration::from_secs(300);
/// Encoded size budget of one offer's entries, well under the control-frame cap.
const BATCH_BYTES: usize = 96 * 1024;
/// How often progress is reported.
const TICK: Duration = Duration::from_millis(150);

/// The other side said no.
#[derive(Debug)]
pub struct Declined;

impl std::fmt::Display for Declined {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the other device declined the files")
    }
}

impl std::error::Error for Declined {}

/// The other side has no room for the files (its free-space guard said no).
#[derive(Debug)]
pub struct NoRoom;

impl std::fmt::Display for NoRoom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the other device has no room for these files")
    }
}

impl std::error::Error for NoRoom {}

/// Asks the user what to do about a file that is already at this path ([`Conflict::Ask`], backlog T7). The
/// answer is one of the other rules; another `Ask`, or no answer within [`ASK_TIMEOUT`], keeps both.
/// The answer to one name-clash question.
pub type AskFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Conflict> + Send>>;

#[derive(Clone)]
pub struct AskConflict(pub Arc<dyn Fn(PathBuf) -> AskFuture + Send + Sync>);

impl std::fmt::Debug for AskConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AskConflict(..)")
    }
}

/// How long a name-clash question waits before both files are kept.
pub const ASK_TIMEOUT: Duration = Duration::from_secs(300);

/// Where and how received files are saved: the user's rules for this sender, plus what the rules need.
#[derive(Debug, Clone)]
pub struct SaveTo {
    pub rules: SaveRules,
    /// Used when the rules name no folder.
    pub default_folder: PathBuf,
    /// The sender's display name, for a per-device subfolder.
    pub device: String,
    /// Today, `YYYY-MM-DD` in the user's calendar, for a per-day subfolder.
    pub date: String,
    /// Refuse when less than this would stay free.
    pub min_free_bytes: u64,
    /// The question for [`Conflict::Ask`]; without it, a clash keeps both files.
    pub ask: Option<AskConflict>,
}

impl SaveTo {
    /// Default rules into `dir`, the sender's tree kept, no free-space guard: what a plain "save here" means.
    #[must_use]
    pub fn folder(dir: impl Into<PathBuf>) -> Self {
        Self {
            rules: SaveRules::default(),
            default_folder: dir.into(),
            device: String::new(),
            date: today_utc(),
            min_free_bytes: 0,
            ask: None,
        }
    }

    fn target(&self, e: &OfferEntry) -> PathBuf {
        self.rules.target(
            &self.default_folder,
            Arrival {
                path: &e.path,
                mime: &e.mime,
                device: if self.device.is_empty() {
                    "Vutric"
                } else {
                    &self.device
                },
                date: &self.date,
            },
        )
    }
}

impl SaveTo {
    /// The user's rules for whoever is on the other end of `s`: global settings with that device's overrides.
    #[must_use]
    pub fn for_peer(
        s: &Session,
        settings: &vutric_core::storage::Settings,
        default_folder: PathBuf,
        date: String,
    ) -> Self {
        let (rules, _) = settings.rules_for(&s.remote_id().to_string());
        Self {
            rules,
            default_folder,
            device: s.remote_name.clone(),
            date,
            min_free_bytes: settings.min_free_bytes,
            ask: None,
        }
    }
}

/// Today in UTC, `YYYY-MM-DD`.
#[must_use]
pub fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    vutric_core::storage::date_of_unix(secs)
}

/// One file on this device, ready to offer.
#[derive(Debug, Clone)]
pub struct Outgoing {
    pub source: PathBuf,
    /// The path the receiver sees, `/`-separated (a picked folder keeps its name and tree).
    pub path: String,
    pub size: u64,
    pub mime: String,
}

impl Outgoing {
    /// One file, offered under `name` (or its own file name).
    pub fn file(source: impl Into<PathBuf>, name: Option<String>) -> Result<Self> {
        let source = source.into();
        let meta =
            std::fs::metadata(&source).with_context(|| format!("open {}", source.display()))?;
        ensure!(meta.is_file(), "{} is not a file", source.display());
        let path = name
            .filter(|n| !n.trim().is_empty())
            .or_else(|| source.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "file".into());
        Ok(Self {
            mime: mime_of(&path).into(),
            path,
            size: meta.len(),
            source,
        })
    }
}

/// Expands what the user picked: a file is itself, a folder is every file under it (symbolic links are not
/// followed), offered as `Folder/sub/file`.
pub fn collect(picked: &[PathBuf]) -> Result<Vec<Outgoing>> {
    let mut out = Vec::new();
    for p in picked {
        let meta = std::fs::symlink_metadata(p).with_context(|| format!("open {}", p.display()))?;
        if meta.is_dir() {
            let root = p
                .file_name()
                .map_or_else(|| "folder".into(), |n| n.to_string_lossy().into_owned());
            walk(p, &root, 0, &mut out)?;
        } else {
            out.push(Outgoing::file(p, None)?);
        }
    }
    ensure!(!out.is_empty(), "nothing to send (empty folder?)");
    Ok(out)
}

fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<Outgoing>) -> Result<()> {
    ensure!(depth < MAX_DEPTH, "folder tree deeper than {MAX_DEPTH}");
    let mut items: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("read {}", dir.display()))?
        .filter_map(std::result::Result::ok)
        .collect();
    items.sort_by_key(std::fs::DirEntry::file_name);
    for item in items {
        let Ok(kind) = item.file_type() else { continue };
        let name = item.file_name().to_string_lossy().into_owned();
        let path = format!("{prefix}/{name}");
        if kind.is_dir() {
            walk(&item.path(), &path, depth + 1, out)?;
        } else if kind.is_file() {
            out.push(Outgoing::file(item.path(), Some(path))?);
        }
    }
    Ok(())
}

/// MIME type from the extension; the receiver uses it only to pick an icon or a viewer.
#[must_use]
pub fn mime_of(path: &str) -> &'static str {
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "heic" | "heif" => "image/heic",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "txt" | "md" | "csv" => "text/plain",
        "apk" => "application/vnd.android.package-archive",
        _ => "application/octet-stream",
    }
}

/// Where a transfer is, for the progress bar.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: u32,
    pub files_total: u32,
    pub failed: u32,
    /// The file most recently started.
    pub current: String,
    pub bytes_per_sec: f64,
}

/// How a transfer ended.
#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub files: u32,
    pub bytes: u64,
    /// Paths (as offered) that did not arrive whole.
    pub failed: Vec<String>,
    /// Receiver only: where the files were saved.
    pub saved: Vec<PathBuf>,
    /// Receiver only: the chosen folder was missing, so the default was used (tell the user once, T2; see
    /// `Settings::tell_missing_folder`).
    pub missing_folder: Option<PathBuf>,
    pub elapsed: Duration,
}

/// Callback for progress, called about every [`TICK`] and once at the end.
pub type OnProgress = Arc<dyn Fn(Progress) + Send + Sync>;

struct Meter {
    done: AtomicU64,
    /// Bytes that were already there (resumed); they do not count towards the speed.
    resumed: AtomicU64,
    files_done: AtomicU32,
    failed: AtomicU32,
    current: Mutex<String>,
    bytes_total: u64,
    files_total: u32,
    started: Instant,
}

impl Meter {
    fn new(bytes_total: u64, files_total: u32) -> Arc<Self> {
        Arc::new(Self {
            done: AtomicU64::new(0),
            resumed: AtomicU64::new(0),
            files_done: AtomicU32::new(0),
            failed: AtomicU32::new(0),
            current: Mutex::new(String::new()),
            bytes_total,
            files_total,
            started: Instant::now(),
        })
    }

    fn add(&self, n: u64) {
        self.done.fetch_add(n, Ordering::Relaxed);
    }

    fn resumed(&self, n: u64) {
        self.add(n);
        self.resumed.fetch_add(n, Ordering::Relaxed);
    }

    fn start(&self, name: &str) {
        name.clone_into(&mut self.current.lock().expect("meter lock"));
    }

    fn snapshot(&self) -> Progress {
        let done = self.done.load(Ordering::Relaxed);
        let moved = done.saturating_sub(self.resumed.load(Ordering::Relaxed));
        let secs = self.started.elapsed().as_secs_f64();
        Progress {
            bytes_done: done,
            bytes_total: self.bytes_total,
            files_done: self.files_done.load(Ordering::Relaxed),
            files_total: self.files_total,
            failed: self.failed.load(Ordering::Relaxed),
            current: self.current.lock().expect("meter lock").clone(),
            bytes_per_sec: if secs > 0.0 { moved as f64 / secs } else { 0.0 },
        }
    }

    fn ticker(self: &Arc<Self>, on: OnProgress) -> Ticker {
        let me = Arc::clone(self);
        Ticker(tokio::spawn(async move {
            loop {
                tokio::time::sleep(TICK).await;
                on(me.snapshot());
            }
        }))
    }
}

/// Stops the progress task however the transfer ends.
struct Ticker(tokio::task::JoinHandle<()>);

impl Drop for Ticker {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn index_u32(i: usize) -> u32 {
    u32::try_from(i).expect("bounded by u32 total_files")
}

/// End (exclusive) of the batch that starts at `first`: at most [`MAX_OFFER_ENTRIES`] entries and about
/// [`BATCH_BYTES`] of names.
fn batch_end(files: &[Outgoing], first: usize) -> usize {
    let mut bytes = 0;
    let mut end = first;
    while end < files.len() && end - first < MAX_OFFER_ENTRIES {
        bytes += files[end].path.len() + files[end].mime.len() + 24;
        if bytes > BATCH_BYTES && end > first {
            break;
        }
        end += 1;
    }
    end
}

// ---- Sending ----

/// Offers `files` on the session and sends what the receiver accepts. Errors with [`Declined`] when the
/// receiver says no; a file that could not be read or did not verify is listed in [`Summary::failed`].
#[allow(clippy::too_many_lines)] // one protocol exchange, read top to bottom
pub async fn send(s: &mut Session, files: Vec<Outgoing>, on: OnProgress) -> Result<Summary> {
    ensure!(!files.is_empty(), "nothing to send");
    let total_files = u32::try_from(files.len()).context("too many files")?;
    let total_bytes = files.iter().map(|f| f.size).sum();
    let id = crate::new_fling_id();
    let meter = Meter::new(total_bytes, total_files);
    let ticker = meter.ticker(Arc::clone(&on));
    let files = Arc::new(files);
    let mut failed: Vec<String> = Vec::new();

    let mut first = 0;
    while first < files.len() {
        let end = batch_end(&files, first);
        let entries = files[first..end]
            .iter()
            .map(|f| OfferEntry {
                path: f.path.clone(),
                size: f.size,
                mime: f.mime.clone(),
            })
            .collect();
        s.send(&Message::Offer {
            id,
            first: index_u32(first),
            entries,
            total_files,
            total_bytes,
        })
        .await?;
        let wait = if first == 0 { ASK } else { IDLE };
        let have = loop {
            match tokio::time::timeout(wait, s.recv())
                .await
                .map_err(|_| anyhow!("no answer to the offer"))??
            {
                Message::OfferReply {
                    id: r,
                    accept,
                    have,
                } if r == id => {
                    if !accept {
                        return Err(Declined.into());
                    }
                    break have;
                }
                Message::FlingFailed { id: r, reason } if r == id => {
                    if reason == FailReason::DiskFull {
                        return Err(NoRoom.into());
                    }
                    bail!("the other device cancelled")
                }
                _ => {}
            }
        };

        let conn = s.conn.clone();
        let mut tasks = tokio::task::JoinSet::new();
        let mut next = first;
        let mut bad: HashSet<usize> = HashSet::new();
        loop {
            while next < end && tasks.len() < PARALLEL {
                let (conn, files, meter) = (conn.clone(), Arc::clone(&files), Arc::clone(&meter));
                let held = have.get(next - first).copied().unwrap_or(0);
                let i = next;
                tasks.spawn(async move {
                    let r = send_one(&conn, id, i, &files[i], held, &meter).await;
                    (i, r)
                });
                next += 1;
            }
            let Some(done) = tasks.join_next().await else {
                break;
            };
            let (i, r) = done.context("send task")?;
            if let Err(e) = r {
                if conn.close_reason().is_some() {
                    return Err(e.context("connection lost"));
                }
                crate::say(format!(
                    "vutric-net: {} not sent: {e:#}",
                    shown_name(&files[i].path)
                ));
                bad.insert(i);
            }
        }

        let refused = loop {
            match tokio::time::timeout(IDLE, s.recv())
                .await
                .map_err(|_| anyhow!("no word from the receiver after the files"))??
            {
                Message::OfferDone { id: r, failed, .. } if r == id => break failed,
                Message::FlingFailed { id: r, .. } if r == id => {
                    bail!("the other device cancelled")
                }
                _ => {}
            }
        };
        bad.extend(
            refused
                .iter()
                .map(|&i| i as usize)
                .filter(|i| (first..end).contains(i)),
        );
        meter
            .files_done
            .fetch_add(index_u32(end - first - bad.len()), Ordering::Relaxed);
        meter
            .failed
            .fetch_add(index_u32(bad.len()), Ordering::Relaxed);
        let mut bad: Vec<usize> = bad.into_iter().collect();
        bad.sort_unstable();
        failed.extend(bad.into_iter().map(|i| files[i].path.clone()));
        first = end;
    }
    drop(ticker);
    on(meter.snapshot());
    Ok(Summary {
        files: total_files - index_u32(failed.len()),
        bytes: meter.done.load(Ordering::Relaxed),
        failed,
        saved: Vec::new(),
        missing_folder: None,
        elapsed: meter.started.elapsed(),
    })
}

async fn send_one(
    conn: &Connection,
    id: FlingId,
    index: usize,
    f: &Outgoing,
    held: u64,
    meter: &Meter,
) -> Result<()> {
    let mut st = conn.open_uni().await.context("open data stream")?;
    let held = if held <= f.size { held } else { 0 };
    let mut header = [0u8; HEADER];
    header[..16].copy_from_slice(&id.0);
    header[16..20].copy_from_slice(&index_u32(index).to_le_bytes());
    header[20..].copy_from_slice(&held.to_le_bytes());
    let r = async {
        st.write_all(&header).await.context("write header")?;
        meter.start(&f.path);
        let mut file = tokio::fs::File::open(&f.source)
            .await
            .with_context(|| format!("open {}", f.source.display()))?;
        let mut hasher = blake3::Hasher::new();
        let mut buf = vec![0u8; CHUNK];
        // The receiver holds the first `held` bytes: hash them here, send only the rest.
        let mut left = held;
        while left > 0 {
            let want = usize::try_from(left).unwrap_or(CHUNK).min(CHUNK);
            let n = file.read(&mut buf[..want]).await?;
            ensure!(n > 0, "the file shrank while sending");
            hasher.update(&buf[..n]);
            left -= n as u64;
        }
        meter.resumed(held);
        let mut left = f.size - held;
        while left > 0 {
            let want = usize::try_from(left).unwrap_or(CHUNK).min(CHUNK);
            let n = file.read(&mut buf[..want]).await?;
            ensure!(n > 0, "the file shrank while sending");
            hasher.update(&buf[..n]);
            st.write_all(&buf[..n]).await.context("write data")?;
            meter.add(n as u64);
            left -= n as u64;
        }
        st.write_all(hasher.finalize().as_bytes())
            .await
            .context("write hash")?;
        st.finish()
            .map_err(|e| anyhow!("finish data stream: {e}"))?;
        // Returns once the receiver has read it all (or the connection went away).
        let _ = st.stopped().await;
        Ok(())
    }
    .await;
    if r.is_err() {
        let _ = st.reset(1u32.into());
    }
    r
}

// ---- Receiving ----

/// The first batch of an incoming transfer, for the "accept?" question.
#[derive(Debug, Clone)]
pub struct IncomingOffer {
    pub id: FlingId,
    pub total_files: u32,
    pub total_bytes: u64,
    first: u32,
    entries: Vec<OfferEntry>,
}

impl IncomingOffer {
    /// The offer in `msg`, if it opens a transfer (`first == 0`).
    #[must_use]
    pub fn from_message(msg: &Message) -> Option<Self> {
        match msg {
            Message::Offer {
                id,
                first: 0,
                entries,
                total_files,
                total_bytes,
            } => Some(Self {
                id: *id,
                total_files: *total_files,
                total_bytes: *total_bytes,
                first: 0,
                entries: entries.clone(),
            }),
            _ => None,
        }
    }

    /// Up to `n` of the offered paths, to show.
    #[must_use]
    pub fn names(&self, n: usize) -> Vec<String> {
        self.entries
            .iter()
            .take(n)
            .map(|e| e.path.clone())
            .collect()
    }
}

/// Says no to an offer.
pub async fn decline(s: &mut Session, offer: &IncomingOffer) -> Result<()> {
    s.send(&Message::OfferReply {
        id: offer.id,
        accept: false,
        have: Vec::new(),
    })
    .await
}

fn part_of(target: &Path) -> PathBuf {
    let mut s = target.as_os_str().to_owned();
    s.push(PART);
    PathBuf::from(s)
}

/// One accepted file of a batch: where it goes, and how much of it is already there.
struct Planned {
    entry: OfferEntry,
    target: PathBuf,
    part: PathBuf,
    held: u64,
}

/// Accepts the transfer and saves it by the rules in `to`. Later batches of the same transfer are accepted
/// without asking again. When the disk would drop under the free-space guard, the sender is told
/// ([`FailReason::DiskFull`]) and this returns [`NoRoom`].
#[allow(clippy::too_many_lines)] // one protocol exchange, read top to bottom
pub async fn receive(
    s: &mut Session,
    offer: IncomingOffer,
    to: &SaveTo,
    on: OnProgress,
) -> Result<Summary> {
    let (rules, missing_folder) = to.rules.usable(Path::is_dir);
    let to = &SaveTo {
        rules,
        ..to.clone()
    };
    let base = to
        .rules
        .folder
        .clone()
        .unwrap_or_else(|| to.default_folder.clone());
    std::fs::create_dir_all(&base).with_context(|| format!("create {}", base.display()))?;
    let id = offer.id;
    let conflict = to.rules.conflict;
    let meter = Meter::new(offer.total_bytes, offer.total_files);
    let ticker = meter.ticker(Arc::clone(&on));
    let renames = Arc::new(tokio::sync::Mutex::new(()));
    let mut used: HashSet<PathBuf> = HashSet::new();
    let mut summary = Summary {
        missing_folder,
        ..Summary::default()
    };
    let ask = to.ask.clone();
    let mut batch = offer;
    loop {
        let first = batch.first as usize;
        let plan: Vec<Planned> = batch
            .entries
            .iter()
            .enumerate()
            .map(|(k, e)| {
                let mut target = to.target(e);
                // Two entries that sanitize to one name must not share a part file.
                if !used.insert(target.clone()) {
                    let name = target
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    target.set_file_name(format!("{} {name}", first + k));
                    used.insert(target.clone());
                }
                let part = part_of(&target);
                let held = std::fs::metadata(&part).map_or(0, |m| m.len());
                Planned {
                    entry: e.clone(),
                    held: if held <= e.size { held } else { 0 },
                    target,
                    part,
                }
            })
            .collect();
        let needed: u64 = plan.iter().map(|p| p.entry.size - p.held).sum();
        // The target's volume, and the app's own storage (the default folder) where phones stage files before
        // handing them to the gallery or a SAF folder (T4): the smaller of the two decides.
        let free = plan
            .first()
            .and_then(|p| crate::disk::free_bytes(&p.target))
            .into_iter()
            .chain(crate::disk::free_bytes(&to.default_folder))
            .min();
        if free.is_some_and(|free| free < needed.saturating_add(to.min_free_bytes)) {
            s.send(&Message::FlingFailed {
                id,
                reason: FailReason::DiskFull,
            })
            .await?;
            return Err(NoRoom.into());
        }
        s.send(&Message::OfferReply {
            id,
            accept: true,
            have: plan.iter().map(|p| p.held).collect(),
        })
        .await?;

        let plan = Arc::new(plan);
        let mut tasks = tokio::task::JoinSet::new();
        let mut seen = HashSet::new();
        let mut results: Vec<Option<Result<PathBuf>>> = (0..plan.len()).map(|_| None).collect();
        let conn = s.conn.clone();
        while seen.len() < plan.len() || !tasks.is_empty() {
            tokio::select! {
                st = tokio::time::timeout(IDLE, conn.accept_uni()), if seen.len() < plan.len() => {
                    let mut st = st
                        .map_err(|_| anyhow!("the sender went quiet"))?
                        .context("accept data stream")?;
                    let mut head = [0u8; HEADER];
                    st.read_exact(&mut head).await.context("read data header")?;
                    let index = u32::from_le_bytes(head[16..20].try_into().expect("4 bytes")) as usize;
                    let offset = u64::from_le_bytes(head[20..].try_into().expect("8 bytes"));
                    let k = index.wrapping_sub(first);
                    if head[..16] != id.0 || k >= plan.len() || !seen.insert(k) || offset != plan[k].held {
                        let _ = st.stop(1u32.into());
                        bail!("unexpected data stream");
                    }
                    let (plan, meter, renames) = (Arc::clone(&plan), Arc::clone(&meter), Arc::clone(&renames));
                    let ask = ask.clone();
                    tasks.spawn(async move {
                        let p = &plan[k];
                        let r = match receive_one(st, p, &meter).await {
                            Ok(()) => place(&p.part, &p.target, conflict, ask.as_ref(), &renames).await,
                            Err(e) => Err(e),
                        };
                        let counter = if r.is_ok() { &meter.files_done } else { &meter.failed };
                        counter.fetch_add(1, Ordering::Relaxed);
                        (k, r)
                    });
                }
                Some(done) = tasks.join_next(), if !tasks.is_empty() => {
                    let (k, r) = done.context("receive task")?;
                    results[k] = Some(r);
                }
            }
        }

        let mut failed = Vec::new();
        for (k, r) in results.into_iter().enumerate() {
            if let Some(Ok(path)) = r {
                summary.saved.push(path);
                continue;
            }
            if let Some(Err(e)) = r {
                crate::say(format!(
                    "vutric-net: {} not received: {e:#}",
                    shown_name(&plan[k].entry.path)
                ));
            }
            failed.push(index_u32(first + k));
            summary.failed.push(plan[k].entry.path.clone());
        }
        s.send(&Message::OfferDone {
            id,
            received: index_u32(plan.len() - failed.len()),
            failed,
        })
        .await?;

        let next = first + plan.len();
        if next >= batch.total_files as usize {
            break;
        }
        batch = loop {
            let msg = tokio::time::timeout(IDLE, s.recv())
                .await
                .map_err(|_| anyhow!("the sender went quiet"))??;
            match msg {
                Message::Offer {
                    id: r,
                    first: f,
                    entries,
                    total_files,
                    total_bytes,
                } if r == id && f as usize == next => {
                    break IncomingOffer {
                        id,
                        total_files,
                        total_bytes,
                        first: f,
                        entries,
                    };
                }
                Message::FlingFailed { id: r, .. } if r == id => bail!("the sender cancelled"),
                _ => {}
            }
        };
    }
    drop(ticker);
    on(meter.snapshot());
    summary.files = index_u32(summary.saved.len());
    summary.bytes = meter.done.load(Ordering::Relaxed);
    summary.elapsed = meter.started.elapsed();
    Ok(summary)
}

async fn receive_one(mut st: RecvStream, p: &Planned, meter: &Meter) -> Result<()> {
    if let Some(parent) = p.part.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    meter.start(&p.entry.path);
    let mut file = tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&p.part)
        .await
        .with_context(|| format!("create {}", p.part.display()))?;
    file.set_len(p.held).await?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; CHUNK];
    let mut left = p.held;
    while left > 0 {
        let want = usize::try_from(left).unwrap_or(CHUNK).min(CHUNK);
        let n = file.read(&mut buf[..want]).await?;
        ensure!(n > 0, "part file shrank");
        hasher.update(&buf[..n]);
        left -= n as u64;
    }
    meter.resumed(p.held);
    file.seek(std::io::SeekFrom::Start(p.held)).await?;
    let mut left = p.entry.size - p.held;
    while left > 0 {
        let want = usize::try_from(left).unwrap_or(CHUNK).min(CHUNK);
        let n = tokio::time::timeout(IDLE, st.read(&mut buf[..want]))
            .await
            .map_err(|_| anyhow!("the sender went quiet"))?
            .context("read data")?
            .ok_or_else(|| anyhow!("data stream ended early"))?;
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).await.context("write file")?;
        meter.add(n as u64);
        left -= n as u64;
    }
    let mut want = [0u8; 32];
    st.read_exact(&mut want).await.context("read hash")?;
    file.flush().await?;
    file.sync_data().await?;
    drop(file);
    if *hasher.finalize().as_bytes() != want {
        let _ = tokio::fs::remove_file(&p.part).await;
        bail!("arrived damaged (hash mismatch), will be sent again from the start");
    }
    Ok(())
}

/// Moves a verified part file to its final name, by the user's conflict rule: `name (2).ext`, replace the old
/// file, keep the old file and drop the new one, or ask. Questions come one at a time (the rename lock is held).
async fn place(
    part: &Path,
    target: &Path,
    conflict: Conflict,
    ask: Option<&AskConflict>,
    renames: &tokio::sync::Mutex<()>,
) -> Result<PathBuf> {
    let _one = renames.lock().await;
    let conflict = match (conflict, ask) {
        (Conflict::Ask, Some(q)) if tokio::fs::try_exists(target).await.unwrap_or(false) => {
            match tokio::time::timeout(ASK_TIMEOUT, (q.0)(target.to_path_buf())).await {
                Ok(c @ (Conflict::Replace | Conflict::Skip)) => c,
                _ => Conflict::KeepBoth,
            }
        }
        (Conflict::Ask, _) => Conflict::KeepBoth,
        (c, _) => c,
    };
    match conflict {
        Conflict::Replace => {
            tokio::fs::rename(part, target)
                .await
                .with_context(|| format!("save {}", target.display()))?;
            return Ok(target.to_path_buf());
        }
        Conflict::Skip if tokio::fs::try_exists(target).await.unwrap_or(false) => {
            let _ = tokio::fs::remove_file(part).await;
            return Ok(target.to_path_buf());
        }
        Conflict::Skip | Conflict::KeepBoth | Conflict::Ask => {}
    }
    let dir = target.parent().context("no parent")?;
    let name = target
        .file_name()
        .context("no name")?
        .to_string_lossy()
        .into_owned();
    let free = filename::unique(&name, |c| dir.join(c).exists());
    let path = dir.join(free);
    tokio::fs::rename(part, &path)
        .await
        .with_context(|| format!("save {}", path.display()))?;
    Ok(path)
}

/// Where a desktop saves received files: `Downloads/Vutric` in the user's home.
pub fn downloads_dir() -> Result<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .context("no home folder")?;
    Ok(PathBuf::from(home).join("Downloads").join("Vutric"))
}

/// Progress as a log line every second at most, through [`crate::say`] (never blocks the network).
#[must_use]
pub fn log_progress(label: &'static str) -> OnProgress {
    let last = Mutex::new(Instant::now());
    Arc::new(move |p: Progress| {
        let mut last = last.lock().expect("log lock");
        let finished = p.files_done + p.failed == p.files_total;
        if !finished && last.elapsed() < Duration::from_secs(1) {
            return;
        }
        *last = Instant::now();
        let failed = if p.failed > 0 {
            format!(", {} failed", p.failed)
        } else {
            String::new()
        };
        crate::say(format!(
            "{label}: {}/{} files, {:.1}/{:.1} MB, {:.1} MB/s{failed}",
            p.files_done,
            p.files_total,
            p.bytes_done as f64 / 1e6,
            p.bytes_total as f64 / 1e6,
            p.bytes_per_sec / 1e6,
        ));
    })
}

/// Just the file name of an offered path, for logs.
#[must_use]
pub fn shown_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_stay_inside_the_save_folder() {
        let to = SaveTo::folder("save");
        let e = |path: &str| OfferEntry {
            path: path.into(),
            size: 1,
            mime: String::new(),
        };
        let dir = Path::new("save");
        assert_eq!(
            to.target(&e("../../etc/passwd")),
            dir.join("etc").join("passwd")
        );
        assert_eq!(
            to.target(&e("Trip/day 1/a.jpg")),
            dir.join("Trip").join("day 1").join("a.jpg")
        );
        assert!(to.target(&e("C:\\Windows\\x.dll")).starts_with(dir));
        assert!(to.target(&e("/")).starts_with(dir));
        assert_ne!(to.target(&e("/")), dir);
    }

    #[test]
    fn batches_respect_both_limits() {
        let f = |path: String| Outgoing {
            source: PathBuf::new(),
            path,
            size: 1,
            mime: "x".into(),
        };
        let small: Vec<_> = (0..600).map(|i| f(format!("{i}.jpg"))).collect();
        assert_eq!(batch_end(&small, 0), MAX_OFFER_ENTRIES);
        assert_eq!(batch_end(&small, 512), 600);
        let long: Vec<_> = (0..200)
            .map(|i| f(format!("{i}{}", "x".repeat(1000))))
            .collect();
        let end = batch_end(&long, 0);
        assert!(end > 0 && end < 200);
    }
}
