//! Running a command as root on this Mac, behind the system's own
//! administrator prompt.
//!
//! sudo can't show Touch ID to an app (its `pam_tid` prompt needs a real
//! terminal), and an askpass helper can only draw its own dialog. Apple's
//! Authorization Services show the real one: "Menagerie wants to make
//! changes", with Touch ID when the app is notarized and a password field
//! otherwise. `AuthorizationExecuteWithPrivileges` then starts the command
//! as root. It is deprecated, but it still works (macOS 27); the supported
//! replacement is a privileged helper registered with `SMAppService`.
//!
//! The command runs inside a small root wrapper, so that the app, which
//! can't signal root processes, can still stop it: the wrapper reads the
//! pipe the app holds, and when the app closes its end (Cancel, or the app
//! going away) it ends the command's whole process group. The wrapper also
//! reports its PID, for reaping, and the command's exit status, which the
//! API doesn't.

use std::ffi::{CString, c_char, c_void};
use std::io::{BufRead, BufReader};
use std::os::fd::{FromRawFd, RawFd};
use std::ptr;

type OSStatus = i32;
type AuthorizationRef = *mut c_void;

#[repr(C)]
struct AuthorizationItem {
    name: *const c_char,
    value_length: usize,
    value: *mut c_void,
    flags: u32,
}

#[repr(C)]
struct AuthorizationItemSet {
    count: u32,
    items: *mut AuthorizationItem,
}

const INTERACTION_ALLOWED: u32 = 1 << 0;
const EXTEND_RIGHTS: u32 = 1 << 1;
const PRE_AUTHORIZE: u32 = 1 << 4;
const DESTROY_RIGHTS: u32 = 1 << 3;
const ERR_CANCELED: OSStatus = -60006;

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn AuthorizationCreate(
        rights: *const AuthorizationItemSet,
        environment: *const AuthorizationItemSet,
        flags: u32,
        authorization: *mut AuthorizationRef,
    ) -> OSStatus;
    fn AuthorizationCopyRights(
        authorization: AuthorizationRef,
        rights: *const AuthorizationItemSet,
        environment: *const AuthorizationItemSet,
        flags: u32,
        authorized: *mut *mut AuthorizationItemSet,
    ) -> OSStatus;
    fn AuthorizationExecuteWithPrivileges(
        authorization: AuthorizationRef,
        path: *const c_char,
        options: u32,
        arguments: *const *const c_char,
        pipe: *mut *mut libc::FILE,
    ) -> OSStatus;
    fn AuthorizationFree(authorization: AuthorizationRef, flags: u32) -> OSStatus;
}

/// Prefixes of the wrapper's own lines, which never reach the log.
const PID_MARK: &str = "menagerie-admin-pid: ";
const EXIT_MARK: &str = "menagerie-admin-exit: ";

/// Runs `"$@"` as root in its own process group, as if under sudo, and
/// ends that group when the app closes its end of the pipe. The trampoline
/// only sets the effective user, so perl makes the real user and group
/// root too, as sudo would. Perl runs tainted while the two users differ,
/// so it is given a fixed PATH (like sudo's `secure_path`) and the
/// arguments are untainted: they come from the app, which asked for these
/// rights. `set +m` keeps job reports out of the output.
const WRAPPER: &str = r#"exec 2>&1
echo "menagerie-admin-pid: $$"
set -m
/usr/bin/perl -e '
  $ENV{PATH} = "/usr/bin:/bin:/usr/sbin:/sbin"; delete @ENV{qw(IFS CDPATH ENV BASH_ENV)};
  my @argv = map { /\A(.*)\z/s; $1 } @ARGV;
  $< = $> = 0; $( = $) = "0 0";
  exec { $argv[0] } @argv or die "$argv[0]: $!\n"' "$@" < /dev/null &
job=$!
set +m
( read -r _; kill -TERM -- -"$job"; sleep 3; kill -KILL -- -"$job" ) <&0 >/dev/null 2>&1 &
watcher=$!
wait "$job" 2>/dev/null
status=$?
kill "$watcher" 2>/dev/null
echo "menagerie-admin-exit: $status"
"#;

/// A command running as root.
pub struct Running {
    pipe: *mut libc::FILE,
    fd: RawFd,
    pid: Option<libc::pid_t>,
    auth: AuthorizationRef,
}

// The FILE and the authorization are only touched by the thread that owns
// `Running`, apart from `stopper`, which uses the descriptor alone.
unsafe impl Send for Running {}

/// Stops a running command from another thread. It holds its own
/// descriptor for the pipe's socket, so it stays valid after the command
/// ends and its pipe is closed.
#[derive(Debug, Clone)]
pub struct Stopper(std::sync::Arc<std::os::fd::OwnedFd>);

impl Stopper {
    /// Close the app's end for writing: the wrapper reads end-of-file and
    /// ends the command.
    pub fn stop(&self) {
        unsafe {
            libc::shutdown(std::os::fd::AsRawFd::as_raw_fd(&*self.0), libc::SHUT_WR);
        }
    }
}

/// Ask for administrator rights with `prompt` (shown under "Menagerie
/// wants to make changes"), then start `program` with `args` as root.
/// `env` is exported to it. `Err(None)` means the person cancelled.
pub fn start(
    prompt: &str,
    program: &str,
    args: &[String],
    env: &[(String, String)],
) -> Result<Running, Option<String>> {
    let mut auth: AuthorizationRef = ptr::null_mut();
    let status = unsafe { AuthorizationCreate(ptr::null(), ptr::null(), 0, &mut auth) };
    if status != 0 {
        return Err(Some(format!("Couldn't start authorization ({status})")));
    }
    let free = |auth| unsafe {
        AuthorizationFree(auth, DESTROY_RIGHTS);
    };

    let right = CString::new("system.privilege.admin").expect("no NUL");
    let mut right_item = AuthorizationItem {
        name: right.as_ptr(),
        value_length: 0,
        value: ptr::null_mut(),
        flags: 0,
    };
    let rights = AuthorizationItemSet {
        count: 1,
        items: &mut right_item,
    };
    let prompt_key = CString::new("prompt").expect("no NUL");
    let prompt_text = CString::new(prompt.replace('\0', "")).expect("no NUL");
    let mut prompt_item = AuthorizationItem {
        name: prompt_key.as_ptr(),
        value_length: prompt_text.as_bytes().len(),
        value: prompt_text.as_ptr() as *mut c_void,
        flags: 0,
    };
    let environment = AuthorizationItemSet {
        count: 1,
        items: &mut prompt_item,
    };
    let status = unsafe {
        AuthorizationCopyRights(
            auth,
            &rights,
            &environment,
            INTERACTION_ALLOWED | EXTEND_RIGHTS | PRE_AUTHORIZE,
            ptr::null_mut(),
        )
    };
    if status == ERR_CANCELED {
        free(auth);
        return Err(None);
    }
    if status != 0 {
        free(auth);
        return Err(Some(format!("Not authorized ({status})")));
    }

    // sh -c WRAPPER sh env NAME=value... program args...
    let mut argv: Vec<CString> = vec![
        CString::new("-c").expect("no NUL"),
        CString::new(WRAPPER).expect("no NUL"),
        CString::new("sh").expect("no NUL"),
        CString::new("/usr/bin/env").expect("no NUL"),
    ];
    let cstr = |text: &str| {
        CString::new(text).map_err(|_| Some("An argument contains a NUL byte".to_owned()))
    };
    for (name, value) in env {
        argv.push(cstr(&format!("{name}={value}"))?);
    }
    argv.push(cstr(program)?);
    for arg in args {
        argv.push(cstr(arg)?);
    }
    let mut argp: Vec<*const c_char> = argv.iter().map(|a| a.as_ptr()).collect();
    argp.push(ptr::null());
    let sh = CString::new("/bin/sh").expect("no NUL");
    let mut pipe: *mut libc::FILE = ptr::null_mut();
    let status = unsafe {
        AuthorizationExecuteWithPrivileges(auth, sh.as_ptr(), 0, argp.as_ptr(), &mut pipe)
    };
    if status != 0 || pipe.is_null() {
        free(auth);
        return Err(Some(format!("Couldn't run as administrator ({status})")));
    }
    let fd = unsafe { libc::fileno(pipe) };
    Ok(Running {
        pipe,
        fd,
        pid: None,
        auth,
    })
}

impl Running {
    pub fn stopper(&self) -> Result<Stopper, String> {
        let fd = unsafe { std::os::fd::BorrowedFd::borrow_raw(self.fd) }
            .try_clone_to_owned()
            .map_err(|e| e.to_string())?;
        Ok(Stopper(std::sync::Arc::new(fd)))
    }

    /// Hand each line of output to `line` until the command ends, then
    /// return its exit status.
    pub fn wait(mut self, mut line: impl FnMut(String)) -> Result<i32, String> {
        // A duplicate to read through, so the FILE keeps sole ownership of
        // the original for `fclose`.
        let dup = unsafe { libc::dup(self.fd) };
        if dup < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let reader = BufReader::new(unsafe { std::fs::File::from_raw_fd(dup) });
        let mut exit = None;
        for text in reader.lines().map_while(Result::ok) {
            if let Some(pid) = text.strip_prefix(PID_MARK) {
                self.pid = pid.trim().parse().ok();
            } else if let Some(code) = text.strip_prefix(EXIT_MARK) {
                exit = code.trim().parse().ok();
            } else {
                line(text);
            }
        }
        exit.ok_or_else(|| "The command ended without reporting its status".to_owned())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        unsafe {
            libc::fclose(self.pipe);
            // The trampoline that became the wrapper is our child.
            if let Some(pid) = self.pid {
                let mut status = 0;
                libc::waitpid(pid, &mut status, 0);
            }
            AuthorizationFree(self.auth, DESTROY_RIGHTS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs a person at the Mac to authorize:
    /// `cargo test -p menagerie admin -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn runs_as_root_streams_and_stops() {
        let env = vec![("SUDO_USER".to_owned(), "someone".to_owned())];
        let script =
            "id -u; id -ru; echo \"user=$SUDO_USER\"; echo first; sleep 1; echo second; exit 3";
        let running = start(
            "Menagerie is testing its administrator prompt.",
            "/bin/sh",
            &["-c".into(), script.into()],
            &env,
        )
        .expect("authorized");
        let mut lines = Vec::new();
        let status = running.wait(|l| lines.push(l)).unwrap();
        println!("{lines:?} -> {status}");
        assert_eq!(lines[..5], ["0", "0", "user=someone", "first", "second"]);
        assert_eq!(status, 3);

        // Stopping ends a command that would run for a minute, children
        // included.
        let running = start(
            "Menagerie is testing stopping.",
            "/bin/sh",
            &[
                "-c".into(),
                "echo started; sleep 60 & sleep 60; echo never".into(),
            ],
            &[],
        )
        .expect("authorized");
        let stopper = running.stopper().unwrap();
        let started = std::time::Instant::now();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            stopper.stop();
        });
        let mut lines = Vec::new();
        let status = running.wait(|l| lines.push(l)).unwrap();
        println!("{lines:?} -> {status} after {:?}", started.elapsed());
        assert_eq!(lines, ["started"]);
        assert_ne!(status, 0);
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        let left = std::process::Command::new("pgrep")
            .args(["-f", "sleep 60"])
            .output()
            .unwrap();
        assert!(left.stdout.is_empty(), "left running: {:?}", left);
    }
}
