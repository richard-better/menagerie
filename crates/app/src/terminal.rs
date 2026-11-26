//! An interactive shell on a machine, drawn in the window.
//!
//! The shell runs on a PTY. Its output goes through libghostty-vt (Ghostty's
//! terminal emulation library), which keeps the screen state; after each
//! batch of output the visible grid is copied into rows of styled glyphs,
//! which is all the view draws. Keys are translated to the bytes a terminal
//! sends.
//!
//! Nothing here blocks the UI: reading, writing and reaping the child each
//! happen on their own thread. Output crosses into the UI through a bounded
//! channel, so a flood of output pushes back on the reader instead of
//! piling up in memory, and each UI turn takes a capped amount of it.

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use gpui::{
    Context, FocusHandle, Focusable, FontWeight, IntoElement, KeyDownEvent, ParentElement, Render,
    Rgba, Styled, Task, Window, div, prelude::*, px, rgb,
};
use libghostty_vt::mode::Mode;
use libghostty_vt::render::{CellIterator, RowIterator};
use libghostty_vt::screen::CellWide;
use libghostty_vt::{RenderState, Terminal};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::sync::Place;
use crate::theme;

const FONT_SIZE: f32 = 12.;
const LINE_HEIGHT: f32 = 16.;
/// Menlo's advance at 12pt, to size the grid.
const CELL_WIDTH: f32 = 7.23;
pub const COLS: u16 = 100;
pub const ROWS: u16 = 22;
/// Output chunks the reader may get ahead of the UI by (8 KiB each).
const QUEUE: usize = 16;
/// Output chunks handled per UI turn before letting a frame through.
const PER_TURN: usize = 32;

/// One drawn glyph and the grid columns it covers.
#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    pub col: u16,
    pub width: u16,
    pub text: String,
}

/// Glyphs that share a style.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub glyphs: Vec<Glyph>,
    pub fg: Option<(u8, u8, u8)>,
    pub bg: Option<(u8, u8, u8)>,
    pub bold: bool,
}

impl Run {
    #[cfg(test)]
    fn text(&self) -> String {
        self.glyphs.iter().map(|g| g.text.as_str()).collect()
    }
}

/// Turns PTY output into rows of runs.
pub struct Screen {
    term: Terminal<'static, 'static>,
    render: RenderState<'static>,
    rows: RowIterator<'static>,
    cells: CellIterator<'static>,
}

impl Screen {
    /// `reply` receives what the terminal answers to queries (cursor
    /// position reports and the like), to send back to the PTY.
    pub fn new(cols: u16, rows: u16, reply: impl FnMut(&[u8]) + 'static) -> Result<Self, String> {
        let mut term = Terminal::new(cols, rows).map_err(|e| e.to_string())?;
        // Nothing here draws images, so APC payloads (Kitty graphics and
        // the like) are refused rather than buffered on the UI thread.
        term.set_apc_max_bytes(Some(0))
            .and_then(|t| t.set_unknown_max_bytes(0))
            .map_err(|e| e.to_string())?;
        let reply = std::cell::RefCell::new(reply);
        term.on_pty_write(move |_, data| (reply.borrow_mut())(data))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            term,
            render: RenderState::new().map_err(|e| e.to_string())?,
            rows: RowIterator::new().map_err(|e| e.to_string())?,
            cells: CellIterator::new().map_err(|e| e.to_string())?,
        })
    }

    pub fn write(&mut self, bytes: &[u8]) {
        self.term.vt_write(bytes);
    }

    fn mode(&self, mode: Mode) -> bool {
        self.term.mode(mode).unwrap_or(false)
    }

    /// The visible rows, and the cursor's cell when it should show.
    pub fn snapshot(&mut self) -> (Vec<Vec<Run>>, Option<(u16, u16)>) {
        let mut out = Vec::new();
        let Ok(snapshot) = self.render.update(&self.term) else {
            return (out, None);
        };
        let cursor = snapshot
            .cursor_visible()
            .unwrap_or(false)
            .then(|| snapshot.cursor_viewport().ok().flatten())
            .flatten()
            .map(|c| (c.x, c.y));
        let Ok(mut rows) = self.rows.update(&snapshot) else {
            return (out, cursor);
        };
        while let Some(row) = rows.next() {
            let mut runs: Vec<Run> = Vec::new();
            if let Ok(mut cells) = self.cells.update(row) {
                let mut col: u16 = 0;
                while let Some(cell) = cells.next() {
                    let wide = cell
                        .raw_cell()
                        .and_then(|c| c.wide())
                        .unwrap_or(CellWide::Narrow);
                    let width = match wide {
                        // Covered by the wide glyph before it.
                        CellWide::SpacerTail => {
                            col += 1;
                            continue;
                        }
                        CellWide::Wide => 2,
                        _ => 1,
                    };
                    let mut text = String::new();
                    if cell.graphemes_utf8(&mut text).is_err() || text.is_empty() {
                        text.push(' ');
                    }
                    let fg = cell.fg_color().ok().flatten().map(|c| (c.r, c.g, c.b));
                    let bg = cell.bg_color().ok().flatten().map(|c| (c.r, c.g, c.b));
                    let bold = cell.style().map(|s| s.bold).unwrap_or(false);
                    let glyph = Glyph { col, width, text };
                    match runs.last_mut() {
                        Some(run) if run.fg == fg && run.bg == bg && run.bold == bold => {
                            run.glyphs.push(glyph)
                        }
                        _ => runs.push(Run {
                            glyphs: vec![glyph],
                            fg,
                            bg,
                            bold,
                        }),
                    }
                    col += 1;
                }
            }
            out.push(runs);
        }
        (out, cursor)
    }
}

/// The bytes a terminal sends for a key press, or `None` for keys it
/// doesn't send (Command shortcuts belong to the app). `app_cursor` is the
/// terminal's application cursor mode, in which arrows send `ESC O`.
pub fn key_bytes(
    key: &str,
    key_char: Option<&str>,
    control: bool,
    alt: bool,
    platform: bool,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    if platform {
        return None;
    }
    let arrow = |c: u8| {
        let mut b = if app_cursor {
            b"\x1bO".to_vec()
        } else {
            b"\x1b[".to_vec()
        };
        b.push(c);
        Some(b)
    };
    let named: Option<&[u8]> = match key {
        "up" => return arrow(b'A'),
        "down" => return arrow(b'B'),
        "right" => return arrow(b'C'),
        "left" => return arrow(b'D'),
        "enter" => Some(b"\r"),
        "backspace" => Some(b"\x7f"),
        "tab" => Some(b"\t"),
        "escape" => Some(b"\x1b"),
        "home" => Some(b"\x1b[H"),
        "end" => Some(b"\x1b[F"),
        "delete" => Some(b"\x1b[3~"),
        "pageup" => Some(b"\x1b[5~"),
        "pagedown" => Some(b"\x1b[6~"),
        _ => None,
    };
    if let Some(bytes) = named {
        return Some(bytes.to_vec());
    }
    if control && key.len() == 1 {
        let c = key.as_bytes()[0].to_ascii_lowercase();
        if c.is_ascii_lowercase() {
            return Some(vec![c - b'a' + 1]);
        }
    }
    let text = key_char.unwrap_or(key);
    if text.is_empty() || text.chars().count() != 1 && key_char.is_none() {
        return None;
    }
    let mut bytes = Vec::new();
    if alt {
        bytes.push(0x1b);
    }
    bytes.extend_from_slice(text.as_bytes());
    Some(bytes)
}

/// Pasted text, bracketed when the program asked for it so a multi-line
/// paste isn't run line by line.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        // No ESC at all inside the bracket, so no terminator can be put
        // together from the pasted bytes and end it early.
        let text = text.replace('\x1b', "");
        [b"\x1b[200~".as_slice(), text.as_bytes(), b"\x1b[201~"].concat()
    } else {
        text.as_bytes().to_vec()
    }
}

/// Writes queued for the PTY at most, and their total size. Past either the
/// program isn't reading its input, so more is dropped instead of held.
const WRITES: usize = 256;
const WRITE_BYTES: usize = 1 << 20;
/// The largest paste sent to the terminal.
pub const PASTE_MAX: usize = 256 * 1024;
/// How long a closed shell gets to exit on SIGHUP before it is killed.
const CLOSE_GRACE: Duration = Duration::from_secs(1);
/// How often the PTY threads look up from blocking work.
const POLL_MS: i32 = 100;

/// What the UI and the PTY threads share.
#[derive(Default)]
struct Shared {
    /// The shell's PID once it runs, 0 before.
    pid: std::sync::atomic::AtomicI32,
    /// Set by the supervisor once it has reaped the shell.
    reaped: AtomicBool,
    /// Set when the terminal is closed: the threads stop, queued input is
    /// discarded, and the shell is ended.
    closed: AtomicBool,
    /// Bytes queued for the writer thread.
    pending: AtomicUsize,
}

/// Every shell that may still be running, for quitting.
static SHELLS: Mutex<Vec<std::sync::Weak<Shared>>> = Mutex::new(Vec::new());

/// Kill every shell outright. For quitting: gpui gives quit handlers 100 ms,
/// too short for the hang-up grace period. Only unreaped shells are signalled.
pub fn kill_all() {
    let shells = SHELLS.lock().map(|s| s.clone()).unwrap_or_default();
    for shared in shells.iter().filter_map(std::sync::Weak::upgrade) {
        shared.close();
        let pid = shared.pid.load(Ordering::SeqCst);
        if pid > 0 && !shared.reaped.load(Ordering::SeqCst) {
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

impl Shared {
    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

/// Owns the shell: the only thread that reaps it and the only one that
/// signals it. It signals only after `try_wait` found the shell unreaped, and
/// an exited shell stays a zombie until this thread reaps it, so the PID can
/// never belong to someone else when it is signalled.
fn supervise(mut child: Box<dyn Child + Send + Sync>, shared: Arc<Shared>) {
    let pid = child
        .process_id()
        .and_then(|p| libc::pid_t::try_from(p).ok());
    let mut hung_up: Option<std::time::Instant> = None;
    loop {
        if !matches!(child.try_wait(), Ok(None)) {
            shared.reaped.store(true, Ordering::SeqCst);
            return;
        }
        if shared.is_closed()
            && let Some(pid) = pid
        {
            let signal = match hung_up {
                None => {
                    hung_up = Some(std::time::Instant::now());
                    Some(libc::SIGHUP)
                }
                Some(at) if at.elapsed() >= CLOSE_GRACE => Some(libc::SIGKILL),
                Some(_) => None,
            };
            if let Some(signal) = signal {
                // The shell leads its own process group on the PTY.
                unsafe {
                    libc::kill(-pid, signal);
                    libc::kill(pid, signal);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(if shared.is_closed() {
            50
        } else {
            250
        }));
    }
}

/// A new descriptor for the PTY master: close-on-exec, so no other child
/// inherits it, and non-blocking, so neither PTY thread can get stuck in a
/// read or write after Close. Closing it is all its drop does: portable-pty's
/// own writer sends a newline and EOF when dropped, which would run a
/// half-typed command. The two duplicates share one open file, so they share
/// the non-blocking flag too.
fn dup_fd(fd: RawFd) -> Result<OwnedFd, String> {
    let owned = unsafe { BorrowedFd::borrow_raw(fd) }
        .try_clone_to_owned()
        .map_err(|e| e.to_string())?;
    let flags = unsafe { libc::fcntl(owned.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(owned.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(owned)
}

/// Wait up to `POLL_MS` for `fd` to be ready for `events`.
fn wait_for(fd: RawFd, events: libc::c_short) -> bool {
    let mut poll = libc::pollfd {
        fd,
        events,
        revents: 0,
    };
    unsafe { libc::poll(&mut poll, 1, POLL_MS) > 0 }
}

/// What the setup worker hands back once the shell is running. Until a
/// session takes it over, dropping it ends the shell.
struct Spawned {
    master: Option<Box<dyn MasterPty + Send>>,
    read: Option<OwnedFd>,
    write: Option<OwnedFd>,
    shared: Arc<Shared>,
    adopted: bool,
}

impl Drop for Spawned {
    fn drop(&mut self) {
        if !self.adopted {
            self.shared.close();
        }
    }
}

/// Open a PTY and start the shell on it. Runs on a worker thread.
fn spawn_shell(place: Place, dir: Option<std::path::PathBuf>) -> Result<Spawned, String> {
    let pty = native_pty_system()
        .openpty(PtySize {
            rows: ROWS,
            cols: COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;
    let mut command = match &place {
        Place::Here => {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_owned());
            let mut c = CommandBuilder::new(shell);
            c.arg("-l");
            if let Some(dir) = dir.filter(|d| d.is_dir()) {
                c.cwd(dir);
            } else if let Some(home) = std::env::var_os("HOME") {
                c.cwd(home);
            }
            c
        }
        Place::Ssh(to) => {
            let mut c = CommandBuilder::new("ssh");
            c.args(["-t", "--", to]);
            c
        }
    };
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    let child = pty
        .slave
        .spawn_command(command)
        .map_err(|e| e.to_string())?;
    // The child holds its own copy; the parent's must close so EOF arrives.
    drop(pty.slave);
    let shared = Arc::new(Shared::default());
    if let Some(pid) = child.process_id().and_then(|p| i32::try_from(p).ok()) {
        shared.pid.store(pid, Ordering::SeqCst);
    }
    if let Ok(mut shells) = SHELLS.lock() {
        shells.retain(|w| w.strong_count() > 0);
        shells.push(Arc::downgrade(&shared));
    }
    {
        let shared = shared.clone();
        std::thread::spawn(move || supervise(child, shared));
    }
    // From here on, any failure drops `spawned`, which ends the shell.
    let mut spawned = Spawned {
        master: None,
        read: None,
        write: None,
        shared,
        adopted: false,
    };
    let fd = pty
        .master
        .as_raw_fd()
        .ok_or("the PTY has no file descriptor")?;
    spawned.read = Some(dup_fd(fd)?);
    spawned.write = Some(dup_fd(fd)?);
    spawned.master = Some(pty.master);
    Ok(spawned)
}

/// The live half of a terminal: the PTY and the threads around it.
struct Session {
    /// Bytes for the PTY, written by the writer thread.
    input: SyncSender<Vec<u8>>,
    shared: Arc<Shared>,
    _master: Box<dyn MasterPty + Send>,
    _reader: Task<()>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.close();
    }
}

/// A shell on a machine, as a view.
pub struct TerminalView {
    screen: Option<Screen>,
    lines: Vec<Vec<Run>>,
    cursor: Option<(u16, u16)>,
    session: Option<Session>,
    focus: FocusHandle,
    starting: bool,
    exited: bool,
    error: Option<String>,
    _start: Task<()>,
}

impl TerminalView {
    /// Start a login shell here (in `dir`), or an SSH session to `place`.
    /// Setup happens on a worker; a failure shows in the view.
    pub fn new(place: &Place, dir: Option<std::path::PathBuf>, cx: &mut Context<Self>) -> Self {
        let place = place.clone();
        // If the view goes away first, this task is dropped along with the
        // `Spawned` it holds, and that ends the shell.
        let start = cx.spawn(async move |this, cx| {
            let spawned = cx
                .background_executor()
                .spawn(async move { spawn_shell(place, dir) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.starting = false;
                match spawned {
                    Ok(s) => view.attach(s, cx),
                    Err(e) => {
                        view.error = Some(e);
                        view.exited = true;
                    }
                }
                cx.notify();
            });
        });
        Self {
            screen: None,
            lines: Vec::new(),
            cursor: None,
            session: None,
            focus: cx.focus_handle(),
            starting: true,
            exited: false,
            error: None,
            _start: start,
        }
    }

    /// Wire a running shell to the screen. On the UI thread, where the
    /// libghostty-vt handles live.
    fn attach(&mut self, mut spawned: Spawned, cx: &mut Context<Self>) {
        let (Some(master), Some(read), Some(write)) = (
            spawned.master.take(),
            spawned.read.take(),
            spawned.write.take(),
        ) else {
            return;
        };
        let shared = spawned.shared.clone();

        let (input, keys) = std::sync::mpsc::sync_channel::<Vec<u8>>(WRITES);
        let writer_shared = shared.clone();
        std::thread::spawn(move || {
            let mut file = std::fs::File::from(write);
            for bytes in keys {
                let len = bytes.len();
                let mut sent = 0;
                // In pieces, checking for Close before each one: after Close
                // nothing more is typed, not even the rest of a paste.
                let ok = loop {
                    if writer_shared.is_closed() {
                        break false;
                    }
                    if sent == len {
                        break true;
                    }
                    match file.write(&bytes[sent..]) {
                        Ok(n) => sent += n,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            wait_for(file.as_raw_fd(), libc::POLLOUT);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(_) => break false,
                    }
                };
                writer_shared.pending.fetch_sub(len, Ordering::SeqCst);
                if !ok {
                    return;
                }
            }
        });
        let replies = input.clone();
        let reply_shared = shared.clone();
        let screen = match Screen::new(COLS, ROWS, move |data| {
            queue(&replies, &reply_shared, data.to_vec());
        }) {
            Ok(screen) => screen,
            Err(e) => {
                // `spawned` drops un-adopted at the end of this function,
                // ending the shell.
                self.error = Some(e);
                self.exited = true;
                return;
            }
        };

        let (mut tx, mut rx) = futures::channel::mpsc::channel::<Vec<u8>>(QUEUE);
        let reader_shared = shared.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut file = std::fs::File::from(read);
            loop {
                if reader_shared.is_closed() {
                    return;
                }
                // Wait for output, but look up regularly: a job outside the
                // shell's group can hold the PTY open after Close.
                if !wait_for(file.as_raw_fd(), libc::POLLIN) {
                    continue;
                }
                match file.read(&mut buf) {
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) =>
                    {
                        continue;
                    }
                    Ok(0) | Err(_) => return,
                    // Blocks while the queue is full: backpressure on the PTY.
                    Ok(n) => {
                        if futures::executor::block_on(tx.send(buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        let reader_task = cx.spawn(async move |this, cx| {
            // Chunks handled since the UI last got a turn, across batches.
            let mut since_yield = 0;
            while let Some(mut chunk) = rx.next().await {
                since_yield += 1;
                while since_yield < PER_TURN {
                    match rx.try_recv() {
                        Ok(more) => {
                            chunk.extend_from_slice(&more);
                            since_yield += 1;
                        }
                        Err(_) => break,
                    }
                }
                if this.update(cx, |view, cx| view.feed(&chunk, cx)).is_err() {
                    return;
                }
                if since_yield >= PER_TURN {
                    // Flooding: let a frame and input through.
                    cx.background_executor()
                        .timer(Duration::from_millis(8))
                        .await;
                    since_yield = 0;
                }
            }
            let _ = this.update(cx, |view, cx| {
                view.exited = true;
                // The rows already drawn stay; the PTY and threads go.
                view.session = None;
                view.screen = None;
                cx.notify();
            });
        });

        spawned.adopted = true;
        self.screen = Some(screen);
        self.session = Some(Session {
            input,
            shared,
            _master: master,
            _reader: reader_task,
        });
    }

    fn feed(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        if let Some(screen) = &mut self.screen {
            screen.write(bytes);
            let (lines, cursor) = screen.snapshot();
            self.lines = lines;
            self.cursor = cursor;
        }
        cx.notify();
    }

    fn send(&mut self, bytes: Vec<u8>) {
        if let Some(session) = self.session.as_ref().filter(|_| !self.exited) {
            queue(&session.input, &session.shared, bytes);
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let mode = |m| self.screen.as_ref().is_some_and(|s| s.mode(m));
        if k.modifiers.platform && k.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                if text.len() > PASTE_MAX {
                    self.error = Some(format!(
                        "Not pasted: {} KB is more than the {} KB limit.",
                        text.len() / 1024,
                        PASTE_MAX / 1024
                    ));
                    cx.notify();
                } else {
                    let bytes = paste_bytes(&text, mode(Mode::BRACKETED_PASTE));
                    self.send(bytes);
                }
            }
            cx.stop_propagation();
            return;
        }
        if let Some(bytes) = key_bytes(
            &k.key,
            k.key_char.as_deref(),
            k.modifiers.control,
            k.modifiers.alt,
            k.modifiers.platform,
            mode(Mode::DECCKM),
        ) {
            self.send(bytes);
            cx.stop_propagation();
        }
    }
}

/// Queue bytes for the writer thread, unless the queue is full or they
/// would take the pending total past `WRITE_BYTES`.
fn queue(input: &SyncSender<Vec<u8>>, shared: &Shared, bytes: Vec<u8>) -> bool {
    let len = bytes.len();
    if shared.pending.fetch_add(len, Ordering::SeqCst) + len > WRITE_BYTES
        || input.try_send(bytes).is_err()
    {
        shared.pending.fetch_sub(len, Ordering::SeqCst);
        return false;
    }
    true
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

fn colour((r, g, b): (u8, u8, u8)) -> Rgba {
    rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32)
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let cursor = self.cursor.filter(|_| focused && !self.exited);
        // The real advance of Menlo here, so ASCII text lands on the grid.
        let text_system = window.text_system();
        let cell = text_system
            .advance(
                text_system.resolve_font(&gpui::font(theme::MONO)),
                px(FONT_SIZE),
                'm',
            )
            .map(|size| size.width)
            .unwrap_or(px(CELL_WIDTH));
        let rows = self.lines.iter().enumerate().map(|(y, runs)| {
            let cursor_col = cursor.filter(|&(_, cy)| cy as usize == y).map(|(cx, _)| cx);
            let mut row = div().relative().h(px(LINE_HEIGHT));
            for run in runs {
                let style = |text: String, col: u16, inverse: bool| {
                    let mut span = div().absolute().left(cell * col as f32).top_0().child(text);
                    let fg = run.fg.map(colour).unwrap_or(rgb(0xd8d8de));
                    let bg = run.bg.map(colour);
                    if inverse {
                        span = span.bg(fg).text_color(bg.unwrap_or(rgb(0x0f0f11)));
                    } else {
                        span = span.text_color(fg);
                        if let Some(bg) = bg {
                            span = span.bg(bg);
                        }
                    }
                    if run.bold {
                        span = span.font_weight(FontWeight::BOLD);
                    }
                    span
                };
                // Each segment starts at its own grid column, so no glyph's
                // shaped width can push later ones off the grid. Plain ASCII
                // shares a segment (Menlo's advance is the cell); anything
                // else, and the glyph under the cursor, gets its own cell(s).
                let mut ascii = String::new();
                let mut ascii_col = 0;
                for glyph in &run.glyphs {
                    let under =
                        cursor_col.is_some_and(|c| c >= glyph.col && c < glyph.col + glyph.width);
                    let plain = glyph.width == 1 && glyph.text.is_ascii() && !under;
                    if plain {
                        if ascii.is_empty() {
                            ascii_col = glyph.col;
                        }
                        ascii.push_str(&glyph.text);
                        continue;
                    }
                    if !ascii.is_empty() {
                        row = row.child(style(std::mem::take(&mut ascii), ascii_col, false));
                    }
                    row = row.child(
                        style(glyph.text.clone(), glyph.col, under)
                            .w(cell * glyph.width as f32)
                            .overflow_hidden(),
                    );
                }
                if !ascii.is_empty() {
                    row = row.child(style(ascii, ascii_col, false));
                }
            }
            row
        });
        div()
            .id("terminal")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_click(cx.listener(|this, _, window, _| this.focus.focus(window)))
            .w_full()
            .rounded(px(9.))
            .bg(rgb(0x0f0f11))
            .border_1()
            .border_color(if focused {
                rgb(0x3a3a46)
            } else {
                theme::line()
            })
            // The PTY is a fixed size: narrow windows scroll sideways.
            .overflow_x_scroll()
            .child(
                div()
                    .w(cell * COLS as f32 + px(20.))
                    .h(px(LINE_HEIGHT * ROWS as f32 + 20.))
                    .p(px(10.))
                    .font_family(theme::MONO)
                    .text_size(px(FONT_SIZE))
                    .line_height(px(LINE_HEIGHT))
                    .children(rows),
            )
            .when(self.starting, |el| {
                el.child(
                    div()
                        .px(px(10.))
                        .pb(px(8.))
                        .font_family(theme::MONO)
                        .text_size(px(FONT_SIZE))
                        .text_color(theme::mute())
                        .child("Starting…"),
                )
            })
            .when(self.exited, |el| {
                el.child(
                    div()
                        .px(px(10.))
                        .pb(px(8.))
                        .font_family(theme::MONO)
                        .text_size(px(FONT_SIZE))
                        .text_color(theme::mute())
                        .child("[session ended]"),
                )
            })
            .when_some(self.error.clone(), |el, e| {
                el.child(
                    div()
                        .px(px(10.))
                        .pb(px(8.))
                        .text_color(rgb(0xe5534b))
                        .child(e),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_become_terminal_bytes() {
        let key = |k, c, ctrl, alt, cmd| key_bytes(k, c, ctrl, alt, cmd, false);
        assert_eq!(
            key("enter", None, false, false, false),
            Some(b"\r".to_vec())
        );
        assert_eq!(key("c", Some("c"), true, false, false), Some(vec![3]));
        assert_eq!(
            key("up", None, false, false, false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key("a", Some("a"), false, false, false),
            Some(b"a".to_vec())
        );
        assert_eq!(
            key("b", Some("b"), false, true, false),
            Some(b"\x1bb".to_vec())
        );
        assert_eq!(key("c", None, false, false, true), None);
        assert_eq!(
            key("2", Some("é"), false, false, false),
            Some("é".as_bytes().to_vec())
        );
        // Application cursor mode, as less and vim ask for.
        assert_eq!(
            key_bytes("up", None, false, false, false, true),
            Some(b"\x1bOA".to_vec())
        );
    }

    #[test]
    fn pastes_are_bracketed_when_asked() {
        assert_eq!(paste_bytes("a\nb", false), b"a\nb");
        assert_eq!(paste_bytes("a\nb", true), b"\x1b[200~a\nb\x1b[201~");
        assert_eq!(
            paste_bytes("x\x1b[201~rm -rf", true),
            b"\x1b[200~x[201~rm -rf\x1b[201~"
        );
        // Removing one terminator mustn't leave the pieces of another.
        let tricky = paste_bytes("\x1b[20\x1b[201~1~ls", true);
        let inner = &tricky[6..tricky.len() - 6];
        assert!(!inner.contains(&0x1b), "{inner:?}");
    }

    #[test]
    fn the_screen_follows_modes() {
        let mut screen = Screen::new(10, 2, |_| {}).unwrap();
        assert!(!screen.mode(Mode::DECCKM));
        screen.write(b"\x1b[?1h\x1b[?2004h");
        assert!(screen.mode(Mode::DECCKM));
        assert!(screen.mode(Mode::BRACKETED_PASTE));
    }

    #[test]
    fn wide_glyphs_cover_two_columns_and_the_cursor_follows_columns() {
        let mut screen = Screen::new(10, 2, |_| {}).unwrap();
        screen.write("a界b".as_bytes());
        let (rows, cursor) = screen.snapshot();
        let glyphs: Vec<(u16, u16, String)> = rows[0]
            .iter()
            .flat_map(|r| r.glyphs.iter())
            .take(3)
            .map(|g| (g.col, g.width, g.text.clone()))
            .collect();
        assert_eq!(
            glyphs,
            vec![
                (0, 1, "a".to_owned()),
                (1, 2, "界".to_owned()),
                (3, 1, "b".to_owned())
            ]
        );
        assert_eq!(cursor, Some((4, 0)));
    }

    /// A real shell on a real PTY: what is typed reaches it, and what it
    /// prints reaches the screen.
    #[test]
    fn a_shell_on_a_pty_round_trips() {
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: 5,
                cols: 40,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "printf 'name? '; read x; echo \"hello $x\""]);
        let mut child = pty.slave.spawn_command(command).unwrap();
        drop(pty.slave);
        let mut reader = pty.master.try_clone_reader().unwrap();
        let mut writer = pty.master.take_writer().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 1024];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    return;
                }
            }
        });
        let mut screen = Screen::new(40, 5, |_| {}).unwrap();
        let text = |screen: &mut Screen| -> String {
            let (rows, _) = screen.snapshot();
            rows.iter()
                .map(|r| r.iter().map(Run::text).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut typed = false;
        while std::time::Instant::now() < deadline {
            if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(100)) {
                screen.write(&chunk);
            }
            let shown = text(&mut screen);
            if !typed && shown.contains("name?") {
                writer.write_all(b"pty\r").unwrap();
                typed = true;
            }
            if shown.contains("hello pty") {
                let _ = child.wait();
                return;
            }
        }
        panic!("never saw the reply: {:?}", text(&mut screen));
    }

    /// A shell that ignores SIGHUP is still gone soon after it is closed.
    #[test]
    fn closing_kills_a_shell_that_ignores_hangup() {
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: 5,
                cols: 40,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "trap '' HUP; sleep 30"]);
        let child = pty.slave.spawn_command(command).unwrap();
        drop(pty.slave);
        let shared = Arc::new(Shared::default());
        let supervisor = {
            let shared = shared.clone();
            std::thread::spawn(move || supervise(child, shared))
        };
        std::thread::sleep(Duration::from_millis(300));
        let started = std::time::Instant::now();
        shared.close();
        while !supervisor.is_finished() {
            assert!(
                started.elapsed() < CLOSE_GRACE * 4,
                "the shell outlived closing"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(started.elapsed() >= CLOSE_GRACE, "it ignores SIGHUP");
        drop(pty.master);
    }

    #[test]
    fn pty_descriptors_close_on_exec_and_never_block() {
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: 5,
                cols: 40,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let fd = dup_fd(pty.master.as_raw_fd().unwrap()).unwrap();
        let fd_flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
        assert_ne!(fd_flags & libc::FD_CLOEXEC, 0);
        let fl_flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
        assert_ne!(fl_flags & libc::O_NONBLOCK, 0);
        // Nothing to read: a non-blocking read returns at once.
        let mut file = std::fs::File::from(fd);
        let mut buf = [0u8; 16];
        let err = file.read(&mut buf).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
    }

    /// Quitting doesn't wait for a shell that ignores SIGHUP.
    #[test]
    fn kill_all_ends_shells_at_once() {
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: 5,
                cols: 40,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "trap '' HUP; sleep 30"]);
        let child = pty.slave.spawn_command(command).unwrap();
        drop(pty.slave);
        let shared = Arc::new(Shared::default());
        shared
            .pid
            .store(child.process_id().unwrap() as i32, Ordering::SeqCst);
        SHELLS.lock().unwrap().push(Arc::downgrade(&shared));
        let supervisor = {
            let shared = shared.clone();
            std::thread::spawn(move || supervise(child, shared))
        };
        std::thread::sleep(Duration::from_millis(300));
        let started = std::time::Instant::now();
        kill_all();
        while !supervisor.is_finished() {
            assert!(
                started.elapsed() < CLOSE_GRACE,
                "quit waited for the grace period"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(shared.reaped.load(Ordering::SeqCst));
        drop(pty.master);
    }

    #[test]
    fn graphics_payloads_are_refused() {
        let mut screen = Screen::new(20, 2, |_| {}).unwrap();
        // A Kitty graphics command with a large payload, then text.
        let mut bytes = b"\x1b_Ga=T,f=24;".to_vec();
        bytes.extend(std::iter::repeat_n(b'A', 1 << 20));
        bytes.extend_from_slice(b"\x1b\\after");
        screen.write(&bytes);
        let (rows, _) = screen.snapshot();
        let line: String = rows[0].iter().map(Run::text).collect();
        assert_eq!(line.trim_end(), "after");
    }

    #[test]
    fn the_write_budget_holds() {
        let (input, _keys) = std::sync::mpsc::sync_channel(WRITES);
        let shared = Shared::default();
        assert!(queue(&input, &shared, vec![0; WRITE_BYTES]));
        assert!(!queue(&input, &shared, vec![0; 1]));
        assert_eq!(shared.pending.load(Ordering::SeqCst), WRITE_BYTES);
    }

    #[test]
    fn the_screen_keeps_text_colours_and_the_cursor() {
        let replies = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = replies.clone();
        let mut screen =
            Screen::new(20, 3, move |data| sink.borrow_mut().extend_from_slice(data)).unwrap();
        screen.write(b"ab\x1b[31mred\x1b[0m\r\nnext");
        // Ask where the cursor is: the terminal answers on the PTY.
        screen.write(b"\x1b[6n");
        let (rows, cursor) = screen.snapshot();
        let line: String = rows[0].iter().map(Run::text).collect();
        assert_eq!(line.trim_end(), "abred");
        let red = rows[0]
            .iter()
            .find(|r| r.text().starts_with("red"))
            .unwrap();
        assert!(red.fg.is_some_and(|(r, g, _)| r > g), "{:?}", red.fg);
        assert_eq!(cursor, Some((4, 1)));
        assert_eq!(replies.borrow().as_slice(), b"\x1b[2;5R");
    }
}
