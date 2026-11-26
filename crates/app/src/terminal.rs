//! An interactive shell on a machine, drawn in the window.
//!
//! The shell runs on a PTY. Its output goes through libghostty-vt (Ghostty's
//! terminal emulation library), which keeps the screen state; after each
//! chunk the visible grid is copied into plain rows of styled runs, which is
//! all the view draws. Keys are translated to the bytes a terminal sends and
//! written back to the PTY.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use gpui::{
    Context, FocusHandle, Focusable, FontWeight, IntoElement, KeyDownEvent, ParentElement, Render,
    Rgba, Styled, Task, Window, div, prelude::*, px, rgb,
};
use libghostty_vt::render::{CellIterator, RowIterator};
use libghostty_vt::{RenderState, Terminal};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::sync::Place;
use crate::theme;

const FONT_SIZE: f32 = 12.;
const LINE_HEIGHT: f32 = 16.;
pub const COLS: u16 = 100;
pub const ROWS: u16 = 22;

/// A run of cells that share a style.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub text: String,
    pub fg: Option<(u8, u8, u8)>,
    pub bg: Option<(u8, u8, u8)>,
    pub bold: bool,
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
    /// position reports and the like), to write back to the PTY.
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
        let mut text = String::new();
        while let Some(row) = rows.next() {
            let mut runs: Vec<Run> = Vec::new();
            if let Ok(mut cells) = self.cells.update(row) {
                while let Some(cell) = cells.next() {
                    text.clear();
                    if cell.graphemes_utf8(&mut text).is_err() || text.is_empty() {
                        text.push(' ');
                    }
                    let fg = cell.fg_color().ok().flatten().map(|c| (c.r, c.g, c.b));
                    let bg = cell.bg_color().ok().flatten().map(|c| (c.r, c.g, c.b));
                    let bold = cell.style().map(|s| s.bold).unwrap_or(false);
                    match runs.last_mut() {
                        Some(run) if run.fg == fg && run.bg == bg && run.bold == bold => {
                            run.text.push_str(&text)
                        }
                        _ => runs.push(Run {
                            text: text.clone(),
                            fg,
                            bg,
                            bold,
                        }),
                    }
                }
            }
            out.push(runs);
        }
        (out, cursor)
    }
}

/// The bytes a terminal sends for a key press, or `None` for keys it
/// doesn't send (Command shortcuts belong to the app).
pub fn key_bytes(
    key: &str,
    key_char: Option<&str>,
    control: bool,
    alt: bool,
    platform: bool,
) -> Option<Vec<u8>> {
    if platform {
        return None;
    }
    let named: Option<&[u8]> = match key {
        "enter" => Some(b"\r"),
        "backspace" => Some(b"\x7f"),
        "tab" => Some(b"\t"),
        "escape" => Some(b"\x1b"),
        "up" => Some(b"\x1b[A"),
        "down" => Some(b"\x1b[B"),
        "right" => Some(b"\x1b[C"),
        "left" => Some(b"\x1b[D"),
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

/// The live half of a terminal: the PTY and the process on it.
struct Session {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    _master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    _reader: Task<()>,
}

/// A shell on a machine, as a view.
pub struct TerminalView {
    screen: Option<Screen>,
    lines: Vec<Vec<Run>>,
    cursor: Option<(u16, u16)>,
    session: Option<Session>,
    focus: FocusHandle,
    exited: bool,
    error: Option<String>,
}

impl TerminalView {
    /// Start a login shell here (in `dir`), or an SSH session to `place`.
    /// A failure to start shows in the view instead.
    pub fn new(place: &Place, dir: Option<std::path::PathBuf>, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        match Self::start(place, dir, cx) {
            Ok((screen, session)) => Self {
                screen: Some(screen),
                lines: Vec::new(),
                cursor: None,
                session: Some(session),
                focus,
                exited: false,
                error: None,
            },
            Err(error) => Self {
                screen: None,
                lines: Vec::new(),
                cursor: None,
                session: None,
                focus,
                exited: true,
                error: Some(error),
            },
        }
    }

    fn start(
        place: &Place,
        dir: Option<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) -> Result<(Screen, Session), String> {
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
        let mut command = match place {
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

        let mut reader = pty.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(
            pty.master.take_writer().map_err(|e| e.to_string())?,
        ));

        let reply_writer = writer.clone();
        let screen = Screen::new(COLS, ROWS, move |data| {
            if let Ok(mut w) = reply_writer.lock() {
                let _ = w.write_all(data);
            }
        })?;

        // The reader blocks, so it gets a thread; chunks cross into gpui
        // through a channel and are fed to the screen on the main thread
        // (libghostty-vt handles are single-threaded).
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => {
                        if tx.unbounded_send(buf[..n].to_vec()).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        let reader_task = cx.spawn(async move |this, cx| {
            while let Some(mut chunk) = rx.next().await {
                // Take everything already waiting, then draw once.
                while let Ok(more) = rx.try_recv() {
                    chunk.extend_from_slice(&more);
                }
                if this.update(cx, |view, cx| view.feed(&chunk, cx)).is_err() {
                    return;
                }
            }
            let _ = this.update(cx, |view, cx| {
                view.exited = true;
                cx.notify();
            });
        });

        Ok((
            screen,
            Session {
                writer,
                _master: pty.master,
                child,
                _reader: reader_task,
            },
        ))
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

    fn send(&mut self, bytes: &[u8]) {
        let Some(session) = self.session.as_ref().filter(|_| !self.exited) else {
            return;
        };
        let result = session
            .writer
            .lock()
            .map_err(|e| e.to_string())
            .and_then(|mut w| {
                w.write_all(bytes)
                    .and_then(|_| w.flush())
                    .map_err(|e| e.to_string())
            });
        if let Err(e) = result {
            self.error = Some(e);
        }
    }

    pub fn exited(&self) -> bool {
        self.exited
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if k.modifiers.platform && k.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.send(text.as_bytes());
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
        ) {
            self.send(&bytes);
            cx.stop_propagation();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
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
            let mut col = 0u16;
            let mut row = div().flex().h(px(LINE_HEIGHT)).overflow_hidden();
            for run in runs {
                let len = run.text.chars().count() as u16;
                let cursor_here = cursor
                    .filter(|&(cx, cy)| cy as usize == y && cx >= col && cx < col + len)
                    .map(|(cx, _)| (cx - col) as usize);
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
                match cursor_here {
                    Some(at) => {
                        let chars: Vec<char> = run.text.chars().collect();
                        let before: String = chars[..at].iter().collect();
                        let under: String = chars[at..=at].iter().collect();
                        let after: String = chars[at + 1..].iter().collect();
                        row = row
                            .child(style(before, false))
                            .child(style(under, true))
                            .child(style(after, false));
                    }
                    None => row = row.child(style(run.text.clone(), false)),
                }
                col += len;
            }
            row
        });
        div()
            .id("terminal")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_click(cx.listener(|this, _, window, _| this.focus.focus(window)))
            .w_full()
            .h(px(LINE_HEIGHT * ROWS as f32 + 20.))
            .p(px(10.))
            .rounded(px(9.))
            .bg(rgb(0x0f0f11))
            .border_1()
            .border_color(if focused {
                rgb(0x3a3a46)
            } else {
                theme::line()
            })
            .overflow_hidden()
            .font_family(theme::MONO)
            .text_size(px(FONT_SIZE))
            .line_height(px(LINE_HEIGHT))
            .children(rows)
            .when(self.exited, |el| {
                el.child(
                    div()
                        .mt(px(4.))
                        .text_color(theme::mute())
                        .child("[session ended]"),
                )
            })
            .when_some(self.error.clone(), |el, e| {
                el.child(div().text_color(rgb(0xe5534b)).child(e))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_become_terminal_bytes() {
        assert_eq!(
            key_bytes("enter", None, false, false, false),
            Some(b"\r".to_vec())
        );
        assert_eq!(key_bytes("c", Some("c"), true, false, false), Some(vec![3]));
        assert_eq!(
            key_bytes("up", None, false, false, false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_bytes("a", Some("a"), false, false, false),
            Some(b"a".to_vec())
        );
        assert_eq!(
            key_bytes("b", Some("b"), false, true, false),
            Some(b"\x1bb".to_vec())
        );
        assert_eq!(key_bytes("c", None, false, false, true), None);
        assert_eq!(
            key_bytes("2", Some("é"), false, false, false),
            Some("é".as_bytes().to_vec())
        );
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
                .map(|r| r.iter().map(|run| run.text.as_str()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut typed = false;
        while std::time::Instant::now() < deadline {
            if let Ok(chunk) = rx.recv_timeout(std::time::Duration::from_millis(100)) {
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
        let line: String = rows[0].iter().map(|r| r.text.as_str()).collect();
        assert_eq!(line.trim_end(), "abred");
        let red = rows[0].iter().find(|r| r.text.starts_with("red")).unwrap();
        assert!(red.fg.is_some_and(|(r, g, _)| r > g), "{:?}", red.fg);
        assert_eq!(cursor, Some((4, 1)));
        assert_eq!(replies.borrow().as_slice(), b"\x1b[2;5R");
    }
}
