//! Starts the program or the script of a "Command" step and looks after it
//! until it ends.
//!
//! - A program gets its arguments exactly as they are written in the step.
//!   Files Windows cannot start by itself go through their interpreter: a
//!   `.ps1` through PowerShell, the rest through `cmd`.
//! - The text of a script is written into a temporary file in UTF-8 and
//!   removed once the script has ended.
//! - A command that is waited for sits in a job object, so a stop or a
//!   timeout ends it together with everything it has started.
//! - A hidden command has no console window; the last part of what it
//!   printed is kept for the journal.

use std::fs;
use std::io::{self, Read};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Globalization::{MultiByteToWideChar, CP_OEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS};
use windows::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject};
use windows::Win32::System::Threading::{CREATE_NEW_CONSOLE, CREATE_NO_WINDOW};

use super::lock;
use crate::model::scenario::{new_id, CommandRun, CommandSpec, ShellKind};

/// How much of what a hidden command printed goes to the journal.
const TAIL_BYTES: usize = 2048;
/// A command may leave a child holding its output open; the readers are not waited for longer.
const READERS_WAIT: Duration = Duration::from_millis(300);
/// The exit code given to a command that is ended by a stop or a timeout.
const KILLED_CODE: u32 = 1;

/// A path or a folder the way people paste it: with spaces around and in quotes.
fn unquoted(text: &str) -> &str {
    text.trim().trim_matches('"')
}

fn via_cmd(line: &str) -> Command {
    let shell = std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into());
    let mut command = Command::new(shell);
    // With `/s` cmd takes the outer quotes off and runs what is between them as typed.
    command.raw_arg("/d /s /c").raw_arg(format!("\"{line}\""));
    command
}

fn via_powershell(file: &str, args: &str, interactive: bool) -> Command {
    let mut command = Command::new("powershell.exe");
    command.raw_arg("-NoProfile -ExecutionPolicy Bypass");
    if !interactive {
        command.raw_arg("-NonInteractive");
    }
    command.raw_arg(format!("-File \"{file}\" {args}"));
    command
}

fn program(path: &str, args: &str, interactive: bool) -> Command {
    let path = unquoted(path);
    let args = args.trim();
    let extension = Path::new(path).extension().and_then(|extension| extension.to_str()).map(str::to_ascii_lowercase);
    match extension.as_deref() {
        None | Some("exe") | Some("com") => {
            let mut command = Command::new(path);
            command.raw_arg(args);
            command
        }
        Some("ps1") => via_powershell(path, args, interactive),
        // Batch files and whatever is opened by the program its type belongs to.
        Some(_) => via_cmd(&format!("\"{path}\" {args}")),
    }
}

/// The file of a script text; removed when the value goes away.
struct ScriptFile(PathBuf);

impl ScriptFile {
    fn write(shell: ShellKind, text: &str) -> io::Result<Self> {
        let dir = std::env::temp_dir().join("ClickIt-scripts");
        fs::create_dir_all(&dir)?;
        let body = text.replace("\r\n", "\n").replace('\n', "\r\n");
        let (extension, content) = match shell {
            // cmd reads the rest of the file as UTF-8 once the code page is switched.
            ShellKind::Cmd => ("cmd", format!("@echo off\r\nchcp 65001>nul\r\n{body}\r\n")),
            // Windows PowerShell takes a file for UTF-8 only when it starts with the mark.
            ShellKind::PowerShell => ("ps1", format!("\u{feff}{body}\r\n")),
        };
        let path = dir.join(format!("{}.{extension}", new_id("script-")));
        fs::write(&path, content)?;
        Ok(Self(path))
    }

    fn path(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for ScriptFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// The folder the command starts in: the one set in the step, otherwise the
/// folder of the program, and the user's home folder for a script.
fn work_dir(spec: &CommandSpec) -> Option<PathBuf> {
    let set = unquoted(&spec.work_dir);
    if !set.is_empty() {
        return Some(PathBuf::from(set));
    }
    let dir = match &spec.run {
        CommandRun::Program { path, .. } => Path::new(unquoted(path)).parent().map(Path::to_path_buf),
        CommandRun::Script { .. } => std::env::var_os("USERPROFILE").map(PathBuf::from),
    };
    dir.filter(|dir| dir.is_dir())
}

/// A job object around the command: ends it with everything it has started.
struct Job(HANDLE);

impl Job {
    fn around(child: &Child) -> Option<Self> {
        // SAFETY: the process handle belongs to `child` and is open; the job handle is closed in `drop`.
        unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null()).ok()?;
            if AssignProcessToJobObject(job, HANDLE(child.as_raw_handle())).is_err() {
                let _ = CloseHandle(job);
                return None;
            }
            Some(Self(job))
        }
    }

    fn terminate(&self) -> bool {
        // SAFETY: the handle is open until `drop`.
        unsafe { TerminateJobObject(self.0, KILLED_CODE) }.is_ok()
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle was opened in `around` and is closed once. Closing
        // it leaves the processes of the job running.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// The last bytes a command printed.
#[derive(Default)]
struct Tail {
    bytes: Vec<u8>,
    /// Something in front of `bytes` was dropped.
    cut: bool,
}

fn collect(mut pipe: impl Read + Send + 'static, tail: Arc<Mutex<Tail>>) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0u8; 1024];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    let mut tail = lock(&tail);
                    tail.bytes.extend_from_slice(&buffer[..count]);
                    if tail.bytes.len() > TAIL_BYTES * 2 {
                        let extra = tail.bytes.len() - TAIL_BYTES;
                        tail.bytes.drain(..extra);
                        tail.cut = true;
                    }
                }
            }
        }
    })
}

/// Console programs print in UTF-8 or in the code page of the console.
fn decode(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    // SAFETY: both buffers are slices that live through the calls.
    let length = unsafe { MultiByteToWideChar(CP_OEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, None) };
    if length <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide = vec![0u16; length as usize];
    // SAFETY: as above; `wide` has the length the first call asked for.
    let written = unsafe { MultiByteToWideChar(CP_OEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, Some(&mut wide)) };
    String::from_utf16_lossy(&wide[..written.max(0) as usize])
}

/// The text of a tail. A tail that lost its beginning may start in the middle of a character.
fn tail_text(bytes: &[u8], cut: bool) -> String {
    let mut bytes = bytes;
    let mut cut = cut;
    if bytes.len() > TAIL_BYTES {
        bytes = &bytes[bytes.len() - TAIL_BYTES..];
        cut = true;
    }
    if cut {
        let broken = bytes.iter().take(3).take_while(|byte| (0x80..0xC0).contains(*byte)).count();
        bytes = &bytes[broken..];
    }
    let text = decode(bytes);
    let text = text.trim();
    if cut && !text.is_empty() {
        format!("…{text}")
    } else {
        text.to_string()
    }
}

/// A command that has been started.
pub struct Launch {
    child: Child,
    job: Option<Job>,
    readers: Vec<JoinHandle<()>>,
    tail: Arc<Mutex<Tail>>,
    script: Option<ScriptFile>,
}

/// Starts the command. The error is the reason Windows gave.
pub fn start(spec: &CommandSpec) -> Result<Launch, String> {
    let interactive = !spec.hidden;
    let mut script = None;
    let mut command = match &spec.run {
        CommandRun::Program { path, args } => program(path, args, interactive),
        CommandRun::Script { shell, text } => {
            let file = ScriptFile::write(*shell, text).map_err(|error| error.to_string())?;
            let path = file.path();
            script = Some(file);
            match shell {
                ShellKind::Cmd => via_cmd(&format!("\"{path}\"")),
                ShellKind::PowerShell => via_powershell(&path, "", interactive),
            }
        }
    };
    if let Some(dir) = work_dir(spec) {
        command.current_dir(dir);
    }
    if spec.hidden {
        command.creation_flags(CREATE_NO_WINDOW.0).stdin(Stdio::null());
        if spec.wait {
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
        } else {
            // Nobody reads what a command that is left to itself prints.
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }
    } else {
        // A console program gets a window of its own, whatever ClickIt was started from.
        command.creation_flags(CREATE_NEW_CONSOLE.0);
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    // A command nobody waits for outlives the run, so nothing is kept to end it with.
    let job = if spec.wait { Job::around(&child) } else { None };
    let tail = Arc::new(Mutex::new(Tail::default()));
    let mut readers = Vec::new();
    if let Some(pipe) = child.stdout.take() {
        readers.push(collect(pipe, Arc::clone(&tail)));
    }
    if let Some(pipe) = child.stderr.take() {
        readers.push(collect(pipe, Arc::clone(&tail)));
    }
    Ok(Launch { child, job, readers, tail, script })
}

impl Launch {
    /// The exit code once the command has ended.
    pub fn poll(&mut self) -> Result<Option<i32>, String> {
        match self.child.try_wait() {
            Ok(status) => Ok(status.map(|status| status.code().unwrap_or(-1))),
            Err(error) => Err(error.to_string()),
        }
    }

    /// Ends the command and everything it has started.
    pub fn kill(&mut self) {
        let ended = self.job.as_ref().is_some_and(Job::terminate);
        if !ended {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }

    /// The last part of what a hidden command printed. Called after its end.
    pub fn output(&mut self) -> String {
        let deadline = Instant::now() + READERS_WAIT;
        while self.readers.iter().any(|reader| !reader.is_finished()) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let tail = lock(&self.tail);
        tail_text(&tail.bytes, tail.cut)
    }

    /// Leaves the command to itself. The file of a script stays until the script ends.
    pub fn detach(self) {
        let Launch { mut child, script, .. } = self;
        if let Some(script) = script {
            thread::spawn(move || {
                let _ = child.wait();
                drop(script);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(command: &Command) -> String {
        let args: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        format!("{} {}", command.get_program().to_string_lossy(), args.join(" "))
    }

    #[test]
    fn a_program_gets_its_arguments_as_they_are_written() {
        let command = program(" \"C:\\Tools\\My Tool\\tool.exe\" ", " --name \"two words\" /q ", true);
        assert_eq!(command.get_program().to_string_lossy(), "C:\\Tools\\My Tool\\tool.exe");
        assert_eq!(line(&command), "C:\\Tools\\My Tool\\tool.exe --name \"two words\" /q");
    }

    #[test]
    fn script_files_go_through_their_interpreter() {
        let batch = line(&program("C:\\My Scripts\\run.bat", "one two", true));
        assert!(batch.ends_with("/d /s /c \"\"C:\\My Scripts\\run.bat\" one two\""), "{batch}");

        let shown = line(&program("C:\\My Scripts\\run.ps1", "-Fast", true));
        assert_eq!(shown, "powershell.exe -NoProfile -ExecutionPolicy Bypass -File \"C:\\My Scripts\\run.ps1\" -Fast");
        let hidden = line(&program("C:\\My Scripts\\run.ps1", "", false));
        assert!(hidden.contains("-NonInteractive"), "{hidden}");
    }

    #[test]
    fn a_script_text_lives_in_a_file_until_it_is_dropped() {
        let file = ScriptFile::write(ShellKind::Cmd, "echo one\necho two").unwrap();
        let path = PathBuf::from(file.path());
        assert_eq!(fs::read_to_string(&path).unwrap(), "@echo off\r\nchcp 65001>nul\r\necho one\r\necho two\r\n");
        drop(file);
        assert!(!path.exists());

        let file = ScriptFile::write(ShellKind::PowerShell, "Write-Output 'Привет'").unwrap();
        assert!(file.path().ends_with(".ps1"));
        assert!(fs::read(file.path()).unwrap().starts_with(&[0xEF, 0xBB, 0xBF]));
    }

    #[test]
    fn the_folder_set_in_the_step_wins_over_the_folder_of_the_program() {
        let temp = std::env::temp_dir();
        let tool = temp.join("tool.exe").to_string_lossy().into_owned();
        let spec = |work_dir: &str| CommandSpec {
            run: CommandRun::Program { path: format!("\"{tool}\""), args: String::new() },
            work_dir: work_dir.to_string(),
            wait: true,
            timeout_ms: 0,
            hidden: true,
        };
        assert_eq!(work_dir(&spec("")), Some(temp));
        assert_eq!(work_dir(&spec(" \"D:\\Work\" ")), Some(PathBuf::from("D:\\Work")));

        let bare = CommandSpec { run: CommandRun::Program { path: "notepad".into(), args: String::new() }, ..spec("") };
        assert_eq!(work_dir(&bare), None);
    }

    #[test]
    fn a_long_output_keeps_its_end() {
        assert_eq!(tail_text("  готово\r\n".as_bytes(), false), "готово");
        // The cut falls into the middle of a letter; the half that is left is dropped.
        let long = format!("{}!", "я".repeat(TAIL_BYTES));
        assert_eq!(tail_text(long.as_bytes(), false), format!("…{}!", "я".repeat(TAIL_BYTES / 2 - 1)));
    }
}
