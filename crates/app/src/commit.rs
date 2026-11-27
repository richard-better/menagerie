//! Committing your checkout's uncommitted changes, so a sync can deploy
//! them.
//!
//! Tools often rewrite their own settings in the checkout, so a dirty tree
//! is normal. `changes` lists what would be committed, `template` makes a
//! message from the file names, and `draft` asks a command of your choosing
//! (usually a coding agent's CLI) for a better one. `commit_and_push`
//! commits exactly the listed files and pushes them.
//!
//! Everything here blocks; call it off the main thread.

use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a drafting command may take before the template is kept.
pub const DRAFT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long committing and pushing may take.
const COMMIT_TIMEOUT: Duration = Duration::from_secs(60);
/// The most diff text a drafting command is given.
const DRAFT_INPUT_MAX: usize = 200 * 1024;
/// New files larger than this are named, not included, in the draft input.
const NEW_FILE_MAX: u64 = 64 * 1024;

/// One changed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// What happened to it: "modified", "new", "deleted", ...
    pub kind: &'static str,
    pub path: String,
    /// Lines added and removed, when git can count them (not for binaries).
    pub lines: Option<(u32, u32)>,
}

/// The checkout's uncommitted changes, untracked files included.
pub fn changes(repo: &Path) -> Result<Vec<Change>, String> {
    let status = git(
        repo,
        &[
            "-c",
            "status.renames=false",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
        ],
    )?;
    let numstat = git(repo, &["diff", "HEAD", "--numstat", "--no-renames", "-z"])?;
    let counts: std::collections::HashMap<&str, (u32, u32)> = numstat
        .split('\0')
        .filter_map(|entry| {
            let mut parts = entry.splitn(3, '\t');
            let added = parts.next()?.parse().ok()?;
            let removed = parts.next()?.parse().ok()?;
            Some((parts.next()?, (added, removed)))
        })
        .collect();
    let mut out = Vec::new();
    for entry in status.split('\0').filter(|e| e.len() > 3) {
        let (code, path) = entry.split_at(3);
        let kind = match code.trim() {
            "??" => "new",
            c if c.contains('D') => "deleted",
            c if c.contains('A') => "new",
            c if c.contains('U') => "conflicted",
            _ => "modified",
        };
        let lines = match kind {
            "new" if code.starts_with("??") => line_count(&repo.join(path)).map(|n| (n, 0)),
            _ => counts.get(path).copied(),
        };
        out.push(Change {
            kind,
            path: path.to_owned(),
            lines,
        });
    }
    Ok(out)
}

/// A small regular file's text. Never through a symlink, which could
/// point anywhere, such as at an SSH key.
fn small_text(path: &Path) -> Option<String> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > NEW_FILE_MAX {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Lines in a small text file.
fn line_count(path: &Path) -> Option<u32> {
    Some(small_text(path)?.lines().count() as u32)
}

/// A message made from the changed files' names, used until (or instead
/// of) a drafted one: `chore: update hex/settings.json`.
pub fn template(changes: &[Change]) -> String {
    let mut names: Vec<String> = Vec::new();
    for change in changes {
        let path = Path::new(&change.path);
        let file = path.file_name().map(|f| f.to_string_lossy().into_owned());
        let dir = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|d| d.to_string_lossy().into_owned());
        let name = match (dir, file) {
            (Some(dir), Some(file)) => format!("{dir}/{file}"),
            (None, Some(file)) => file,
            _ => change.path.clone(),
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let shown = names.len().min(3);
    let mut list = names[..shown].join(", ");
    if names.len() > shown {
        list.push_str(&format!(" and {} more", names.len() - shown));
    }
    if list.is_empty() {
        "chore: update configuration".to_owned()
    } else {
        format!("chore: update {list}")
    }
}

/// Whether a file looks like it holds secrets, so its contents are never
/// handed to a drafting command.
pub fn looks_secret(path: &str) -> bool {
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    name.starts_with(".env")
        || name.starts_with("id_")
        || [".key", ".pem", ".p12", ".pfx", ".age", ".gpg", ".kdbx"]
            .iter()
            .any(|ext| name.ends_with(ext))
        || ["secret", "token", "credential", "password", "private"]
            .iter()
            .any(|word| name.contains(word))
}

/// What a drafting command is given on stdin: the diff of every change
/// that doesn't look secret, and the contents of small new files. Returns
/// the text and the paths left out.
pub fn draft_input(repo: &Path, changes: &[Change]) -> Result<(String, Vec<String>), String> {
    let (withheld, shared): (Vec<&Change>, Vec<&Change>) =
        changes.iter().partition(|c| looks_secret(&c.path));
    let (tracked, untracked): (Vec<&Change>, Vec<&Change>) =
        shared.into_iter().partition(|c| is_tracked(repo, &c.path));
    let tracked: Vec<&str> = tracked.iter().map(|c| c.path.as_str()).collect();
    let mut text = String::new();
    if !tracked.is_empty() {
        let mut args = vec!["diff", "HEAD", "--no-renames", "--no-color", "--"];
        args.extend(tracked.iter().copied());
        text.push_str(&git(repo, &args)?);
    }
    for change in untracked {
        match small_text(&repo.join(&change.path)) {
            Some(body) => text.push_str(&format!("\nNew file {}:\n{body}\n", change.path)),
            None => text.push_str(&format!("\nNew file {} (not shown)\n", change.path)),
        }
    }
    if text.len() > DRAFT_INPUT_MAX {
        let mut end = DRAFT_INPUT_MAX;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n[diff truncated]\n");
    }
    Ok((text, withheld.into_iter().map(|c| c.path.clone()).collect()))
}

fn is_tracked(repo: &Path, path: &str) -> bool {
    git(repo, &["ls-files", "--error-unmatch", "--", path]).is_ok()
}

/// Run `command` in `repo` with `input` on stdin, and take the first line
/// it prints as a commit message. Gives up after `timeout`, ending
/// everything the command started.
pub fn draft(command: &str, repo: &Path, input: &str, timeout: Duration) -> Result<String, String> {
    let out = run_with_timeout(
        Command::new("/bin/sh")
            .args(["-lc", command])
            .current_dir(repo),
        Some(input.as_bytes().to_vec()),
        timeout,
    )?;
    if !out.success {
        let why = last_line(&out.stderr).unwrap_or_else(|| "it failed".to_owned());
        return Err(format!("The drafting command failed: {why}"));
    }
    clean_message(&out.stdout).ok_or_else(|| "The drafting command printed nothing".to_owned())
}

/// The first non-empty line, without the quotes or code fences models like
/// to wrap messages in.
fn clean_message(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("```"))
        .map(|l| {
            l.trim_matches(|c| matches!(c, '"' | '\'' | '`'))
                .trim()
                .to_owned()
        })
        .filter(|l| !l.is_empty())
}

/// Commit exactly `paths` with `message`, then push to `branch`. The
/// checkout must be on `branch`. Returns the new commit's short hash.
pub fn commit_and_push(
    repo: &Path,
    branch: &str,
    paths: &[String],
    message: &str,
) -> Result<String, String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("Write a commit message first.".to_owned());
    }
    if paths.is_empty() {
        return Err("There is nothing to commit.".to_owned());
    }
    let head = git(repo, &["symbolic-ref", "--quiet", "HEAD"]).unwrap_or_default();
    if head.trim() != format!("refs/heads/{branch}") {
        return Err(format!(
            "Your checkout isn't on {branch}. Switch to it first."
        ));
    }
    let mut add = vec!["add", "-A", "--"];
    add.extend(paths.iter().map(String::as_str));
    git(repo, &add)?;
    // Only these paths, even if something else was staged meanwhile.
    let mut commit = vec!["commit", "-q", "-m", message, "--only", "--"];
    commit.extend(paths.iter().map(String::as_str));
    git_slow(repo, &commit)?;
    let hash = git(repo, &["rev-parse", "--short", "HEAD"])?;
    git_slow(
        repo,
        &["push", "-q", "origin", &format!("HEAD:refs/heads/{branch}")],
    )
    .map_err(|e| format!("Committed {}, but the push failed: {e}", hash.trim()))?;
    Ok(hash.trim().to_owned())
}

/// A quick git command's stdout, or its last line of stderr.
fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = git_command(repo)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(last_line(&String::from_utf8_lossy(&out.stderr))
            .unwrap_or_else(|| format!("git {} failed", args[0])))
    }
}

/// A git command that may run hooks or reach the network, with a deadline.
fn git_slow(repo: &Path, args: &[&str]) -> Result<(), String> {
    let out = run_with_timeout(git_command(repo).args(args), None, COMMIT_TIMEOUT)?;
    if out.success {
        Ok(())
    } else {
        Err(last_line(&out.stderr)
            .or_else(|| last_line(&out.stdout))
            .unwrap_or_else(|| format!("git {} failed", args[0])))
    }
}

fn git_command(repo: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repo)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        // File names are names, never pathspec magic such as `:(glob)*`.
        .env("GIT_LITERAL_PATHSPECS", "1");
    command
}

fn last_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .map(str::to_owned)
}

struct Output {
    success: bool,
    stdout: String,
    stderr: String,
}

/// Run `command` in its own process group, feeding it `input`, and kill
/// the whole group if it outlives `timeout`.
fn run_with_timeout(
    command: &mut Command,
    input: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<Output, String> {
    let mut child = command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| e.to_string())?;
    let group = child.id() as i32;
    if let (Some(mut stdin), Some(input)) = (child.stdin.take(), input) {
        // A command that doesn't read its input mustn't block us.
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_string(&mut text);
            }
            text
        })
    };
    let stdout = read(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stderr = read(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if started.elapsed() > timeout {
            unsafe {
                libc::kill(-group, libc::SIGKILL);
            }
            let _ = child.wait();
            return Err(format!("It took longer than {}s", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    // Anything it left running in its group goes too, or the pipes would
    // stay open.
    unsafe {
        libc::kill(-group, libc::SIGKILL);
    }
    Ok(Output {
        success: status.success(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    /// A checkout of a bare origin, with one commit holding `a.txt`.
    fn checkout() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("origin.git");
        let work = dir.path().join("work");
        let o = origin.to_str().unwrap();
        run_git(dir.path(), &["init", "-q", "--bare", "-b", "main", o]);
        run_git(dir.path(), &["clone", "-q", o, work.to_str().unwrap()]);
        run_git(&work, &["config", "commit.gpgsign", "false"]);
        run_git(&work, &["config", "user.name", "Test"]);
        run_git(&work, &["config", "user.email", "test@example.com"]);
        run_git(&work, &["switch", "-q", "-c", "main"]);
        std::fs::create_dir_all(work.join("config/hex")).unwrap();
        std::fs::write(work.join("config/hex/settings.json"), "{}\n").unwrap();
        run_git(&work, &["add", "-A"]);
        run_git(&work, &["commit", "-q", "-m", "one"]);
        run_git(&work, &["push", "-q", "origin", "HEAD:main"]);
        (dir, origin, work)
    }

    #[test]
    fn lists_changes_with_line_counts() {
        let (_dir, _origin, work) = checkout();
        std::fs::write(work.join("config/hex/settings.json"), "{\n  \"a\": 1\n}\n").unwrap();
        std::fs::write(work.join("new.nix"), "{ }\n# two\n").unwrap();
        let changes = changes(&work).unwrap();
        assert_eq!(
            changes,
            vec![
                Change {
                    kind: "modified",
                    path: "config/hex/settings.json".into(),
                    lines: Some((3, 1)),
                },
                Change {
                    kind: "new",
                    path: "new.nix".into(),
                    lines: Some((2, 0)),
                },
            ]
        );
        assert_eq!(
            template(&changes),
            "chore: update hex/settings.json, new.nix"
        );
    }

    #[test]
    fn secrets_are_named_but_never_shared() {
        let (_dir, _origin, work) = checkout();
        std::fs::write(work.join("config/hex/settings.json"), "{\"model\": 2}\n").unwrap();
        std::fs::write(work.join(".env"), "TOKEN=hunter2\n").unwrap();
        std::fs::write(work.join("notes.md"), "hello\n").unwrap();
        let (input, withheld) = draft_input(&work, &changes(&work).unwrap()).unwrap();
        assert!(input.contains("\"model\": 2"), "{input}");
        assert!(input.contains("New file notes.md:\nhello"), "{input}");
        assert!(!input.contains("hunter2"), "{input}");
        assert_eq!(withheld, vec![".env".to_owned()]);
        // A new symlink is named, never followed.
        std::os::unix::fs::symlink(work.join(".env"), work.join("innocent.txt")).unwrap();
        let (input, _) = draft_input(&work, &changes(&work).unwrap()).unwrap();
        assert!(
            input.contains("New file innocent.txt (not shown)"),
            "{input}"
        );
        assert!(!input.contains("hunter2"), "{input}");
        // Names are literal, not pathspec magic.
        std::fs::write(work.join(":(glob)*"), "x\n").unwrap();
        let listed = changes(&work).unwrap();
        assert!(listed.iter().any(|c| c.path == ":(glob)*"), "{listed:?}");
        assert!(looks_secret("secrets/api-token.txt"));
        assert!(looks_secret("keys/id_ed25519"));
        assert!(!looks_secret("config/hex/settings.json"));
    }

    #[test]
    fn drafts_take_the_first_line_and_give_up_in_time() {
        let dir = tempfile::tempdir().unwrap();
        let echo = "cat >/dev/null; printf '\\n```\\n\"feat(hex): use the large model\"\\n```\\n'";
        assert_eq!(
            draft(echo, dir.path(), "diff", DRAFT_TIMEOUT).unwrap(),
            "feat(hex): use the large model"
        );
        // It reads the diff on stdin.
        assert_eq!(
            draft(
                "head -1",
                dir.path(),
                "chore: from stdin\nmore",
                DRAFT_TIMEOUT
            )
            .unwrap(),
            "chore: from stdin"
        );
        assert!(
            draft("echo nope >&2; exit 1", dir.path(), "", DRAFT_TIMEOUT)
                .unwrap_err()
                .contains("nope")
        );
        let started = Instant::now();
        let err = draft(
            "sleep 30 & wait",
            dir.path(),
            "",
            Duration::from_millis(300),
        )
        .unwrap_err();
        assert!(err.contains("longer than"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn commits_only_the_listed_files_and_pushes() {
        let (_dir, origin, work) = checkout();
        std::fs::write(work.join("config/hex/settings.json"), "{\"b\": 1}\n").unwrap();
        std::fs::write(work.join("new.nix"), "{ }\n").unwrap();
        std::fs::write(work.join("later.txt"), "not listed\n").unwrap();
        let listed = vec!["config/hex/settings.json".to_owned(), "new.nix".to_owned()];
        let hash = commit_and_push(&work, "main", &listed, "chore: update hex").unwrap();
        let log = Command::new("git")
            .arg("-C")
            .arg(&origin)
            .args(["show", "--format=%h %s", "--name-only", "main"])
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&log.stdout);
        assert!(
            log.starts_with(&format!("{hash} chore: update hex")),
            "{log}"
        );
        assert!(
            log.contains("new.nix") && !log.contains("later.txt"),
            "{log}"
        );
        // The unlisted file is still there, uncommitted.
        let left = changes(&work).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].path, "later.txt");

        assert!(commit_and_push(&work, "main", &listed, "  ").is_err());
        run_git(&work, &["switch", "-q", "-c", "other"]);
        let err = commit_and_push(&work, "main", &["later.txt".into()], "x").unwrap_err();
        assert!(err.contains("isn't on main"), "{err}");
    }
}
