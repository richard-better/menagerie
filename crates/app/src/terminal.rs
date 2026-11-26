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
use std::sync::atomic::{AtomicBool, Ordering};
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
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

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

/// Writes queued for the PTY at most. Past this the program isn't reading
/// its input, so more is dropped rather than held in memory.
const WRITES: usize = 256;
/// How long a closed shell gets to exit on SIGHUP before it is killed.
const CLOSE_GRACE: Duration = Duration::from_secs(1);

/// The shell's process. Closing it signals from its own thread, and one
/// reaper thread collects its exit, so nothing here waits on the UI thread.
#[derive(Clone)]
struct Process {
    pid: Option<u32>,
    killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
    reaped: Arc<AtomicBool>,
}

impl Process {
    /// SIGHUP, as closing a terminal sends, then SIGKILL to the shell's
    /// process group if it is still running after a grace period. The PID is
    /// only signalled while unreaped, so it can't be someone else's.
    fn close(&self) {
        let this = self.clone();
        std::thread::spawn(move || {
            if this.reaped.load(Ordering::SeqCst) {
                return;
            }
            let _ = this.killer.lock().map(|mut k| k.kill());
            std::thread::sleep(CLOSE_GRACE);
            if let Some(pid) = this.pid
                && !this.reaped.load(Ordering::SeqCst)
            {
                // The shell leads its own session and process group on the PTY.
                let _ = std::process::Command::new("kill")
                    .args(["-KILL", "--", &format!("-{pid}")])
                    .status();
            }
        });
    }
}

/// What the setup worker hands back once the shell is running.
struct Spawned {
    master: Box<dyn MasterPty + Send>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    process: Process,
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
    let mut child = pty
        .slave
        .spawn_command(command)
        .map_err(|e| e.to_string())?;
    // The child holds its own copy; the parent's must close so EOF arrives.
    drop(pty.slave);
    let process = Process {
        pid: child.process_id(),
        killer: Arc::new(Mutex::new(child.clone_killer())),
        reaped: Arc::new(AtomicBool::new(false)),
    };
    let reaped = process.reaped.clone();
    std::thread::spawn(move || {
        let _ = child.wait();
        reaped.store(true, Ordering::SeqCst);
    });
    let started = (|| {
        let reader = pty.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pty.master.take_writer().map_err(|e| e.to_string())?;
        Ok::<_, String>((reader, writer))
    })();
    match started {
        Ok((reader, writer)) => Ok(Spawned {
            master: pty.master,
            reader,
            writer,
            process,
        }),
        Err(e) => {
            // Partly started: the shell mustn't outlive the failure.
            process.close();
            Err(e)
        }
    }
}

/// The live half of a terminal: the PTY and the process on it.
struct Session {
    /// Bytes for the PTY, written by the writer thread.
    input: SyncSender<Vec<u8>>,
    process: Process,
    _master: Box<dyn MasterPty + Send>,
    _reader: Task<()>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.process.close();
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
        let start = cx.spawn(async move |this, cx| {
            let spawned = cx
                .background_executor()
                .spawn(async move { spawn_shell(place, dir) })
                .await;
            let mut spawned = Some(spawned);
            let attached = this.update(cx, |view, cx| {
                view.starting = false;
                match spawned.take() {
                    Some(Ok(s)) => view.attach(s, cx),
                    Some(Err(e)) => {
                        view.error = Some(e);
                        view.exited = true;
                    }
                    None => {}
                }
                cx.notify();
            });
            // Closed while it was starting: end what was started.
            if attached.is_err()
                && let Some(Ok(s)) = spawned
            {
                s.process.close();
            }
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
    fn attach(&mut self, spawned: Spawned, cx: &mut Context<Self>) {
        let Spawned {
            master,
            mut reader,
            mut writer,
            process,
        } = spawned;
        let (input, keys) = std::sync::mpsc::sync_channel::<Vec<u8>>(WRITES);
        std::thread::spawn(move || {
            for bytes in keys {
                if writer
                    .write_all(&bytes)
                    .and_then(|_| writer.flush())
                    .is_err()
                {
                    return;
                }
            }
        });
        let replies = input.clone();
        let screen = match Screen::new(COLS, ROWS, move |data| {
            let _ = replies.try_send(data.to_vec());
        }) {
            Ok(screen) => screen,
            Err(e) => {
                process.close();
                self.error = Some(e);
                self.exited = true;
                return;
            }
        };

        let (mut tx, mut rx) = futures::channel::mpsc::channel::<Vec<u8>>(QUEUE);
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
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
                cx.notify();
            });
        });

        self.screen = Some(screen);
        self.session = Some(Session {
            input,
            process,
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
            // Full means the program isn't reading; dropping beats queueing.
            let _ = session.input.try_send(bytes);
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let mode = |m| self.screen.as_ref().is_some_and(|s| s.mode(m));
        if k.modifiers.platform && k.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                let bytes = paste_bytes(&text, mode(Mode::BRACKETED_PASTE));
                self.send(bytes);
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
        let rows = self.lines.iter().enumerate().map(|(y, runs)| {
            let mut row = div().flex().h(px(LINE_HEIGHT));
            for run in runs {
                let style = |text: String, inverse: bool| {
                    let mut span = div().flex_none().child(text);
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
                // Cells, not shaped text widths, decide where things go: a
                // wide glyph gets exactly two cells whatever its fallback
                // font measures, and the cursor is found by grid column.
                let cursor_col = cursor.filter(|&(_, cy)| cy as usize == y).map(|(cx, _)| cx);
                let mut narrow = String::new();
                for glyph in &run.glyphs {
                    let under = cursor_col
                        .is_some_and(|cx| cx >= glyph.col && cx < glyph.col + glyph.width);
                    if glyph.width == 1 && !under {
                        narrow.push_str(&glyph.text);
                        continue;
                    }
                    if !narrow.is_empty() {
                        row = row.child(style(std::mem::take(&mut narrow), false));
                    }
                    row = row.child(
                        style(glyph.text.clone(), under)
                            .w(px(CELL_WIDTH * glyph.width as f32))
                            .overflow_hidden(),
                    );
                }
                if !narrow.is_empty() {
                    row = row.child(style(narrow, false));
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
                    .w(px(CELL_WIDTH * COLS as f32 + 20.))
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
        let mut child = pty.slave.spawn_command(command).unwrap();
        drop(pty.slave);
        let process = Process {
            pid: child.process_id(),
            killer: Arc::new(Mutex::new(child.clone_killer())),
            reaped: Arc::new(AtomicBool::new(false)),
        };
        let reaped = process.reaped.clone();
        std::thread::spawn(move || {
            let _ = child.wait();
            reaped.store(true, Ordering::SeqCst);
        });
        std::thread::sleep(Duration::from_millis(200));
        let started = std::time::Instant::now();
        process.close();
        while !process.reaped.load(Ordering::SeqCst) {
            assert!(
                started.elapsed() < CLOSE_GRACE * 4,
                "the shell outlived closing"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        drop(pty.master);
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
