//! Windows and processes: finding the target of the "Window" mode, focus,
//! client-area geometry and the list of running executables.

use std::collections::HashMap;
use std::mem::size_of;

use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetAncestor, GetClientRect, GetForegroundWindow, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, SetForegroundWindow, ShowWindow,
    WindowFromPoint, GA_ROOT, SW_RESTORE,
};

use crate::model::scenario::{TitleMatch, WindowTarget};
use crate::runtime::Rect;

/// Window handle as a plain number, so it can cross threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hwnd(pub isize);

impl Hwnd {
    fn raw(self) -> HWND {
        HWND(self.0 as *mut core::ffi::c_void)
    }

    fn from_raw(hwnd: HWND) -> Self {
        Hwnd(hwnd.0 as isize)
    }

    pub fn is_null(self) -> bool {
        self.0 == 0
    }
}

#[derive(Debug, Clone)]
pub struct TopWindow {
    pub hwnd: Hwnd,
    pub title: String,
    pub pid: u32,
    /// Executable name such as `notepad.exe`; empty when it could not be read.
    pub process: String,
}

/// Case-insensitive comparison of a window title with a pattern.
pub fn title_matches(title: &str, pattern: &str, mode: TitleMatch) -> bool {
    let title = title.to_lowercase();
    let pattern = pattern.trim().to_lowercase();
    match mode {
        TitleMatch::Contains => title.contains(&pattern),
        TitleMatch::Exact => title.trim() == pattern,
    }
}

/// Compares executable names without case; `.exe` may be left out in the pattern.
pub fn process_matches(process: &str, pattern: &str) -> bool {
    let process = process.to_lowercase();
    let pattern = pattern.trim().to_lowercase();
    !pattern.is_empty() && (process == pattern || process == format!("{pattern}.exe"))
}

fn window_title(hwnd: HWND) -> String {
    // SAFETY: `hwnd` comes from the system; a stale handle only makes the calls return 0.
    unsafe {
        let length = GetWindowTextLengthW(hwnd);
        if length <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let copied = GetWindowTextW(hwnd, &mut buffer);
        String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
    }
}

fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    // SAFETY: `pid` is a valid out pointer for the duration of the call.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// Executable names of all running processes by process id.
fn process_table() -> HashMap<u32, String> {
    let mut table = HashMap::new();
    // SAFETY: the snapshot handle is checked, used only here and closed before returning;
    // `entry` is a properly sized PROCESSENTRY32W.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return table;
        };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let length = entry.szExeFile.iter().position(|unit| *unit == 0).unwrap_or(entry.szExeFile.len());
                table.insert(entry.th32ProcessID, String::from_utf16_lossy(&entry.szExeFile[..length]));
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }
    table
}

/// Executable names of all running processes, lower case.
pub fn running_processes() -> Vec<String> {
    process_table().into_values().map(|name| name.to_lowercase()).collect()
}

/// The first name from `names` that is running right now.
pub fn first_running(names: &[String]) -> Option<String> {
    if names.iter().all(|name| name.trim().is_empty()) {
        return None;
    }
    let running = running_processes();
    names
        .iter()
        .find(|name| running.iter().any(|process| process_matches(process, name)))
        .map(|name| name.trim().to_string())
}

pub fn process_running(name: &str) -> bool {
    running_processes().iter().any(|process| process_matches(process, name))
}

unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the address of the Vec owned by `top_windows`, alive for the whole enumeration.
    let list = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
    list.push(hwnd);
    BOOL(1)
}

/// Visible top-level windows with a title, front to back, without our own windows.
pub fn top_windows() -> Vec<TopWindow> {
    let mut handles: Vec<HWND> = Vec::new();
    // SAFETY: the callback only pushes into `handles`, which outlives the call.
    let _ = unsafe { EnumWindows(Some(collect_window), LPARAM(&mut handles as *mut Vec<HWND> as isize)) };
    let processes = process_table();
    let own_pid = std::process::id();
    handles
        .into_iter()
        // SAFETY: IsWindowVisible accepts any handle value.
        .filter(|hwnd| unsafe { IsWindowVisible(*hwnd) }.as_bool())
        .filter_map(|hwnd| {
            let title = window_title(hwnd);
            let pid = window_pid(hwnd);
            if title.is_empty() || pid == own_pid {
                return None;
            }
            let process = processes.get(&pid).cloned().unwrap_or_default();
            Some(TopWindow { hwnd: Hwnd::from_raw(hwnd), title, pid, process })
        })
        .collect()
}

fn window_fits(window: &TopWindow, target: &WindowTarget) -> bool {
    let title_ok = target.title.trim().is_empty() || title_matches(&window.title, &target.title, target.title_match);
    let process_ok = target.process.trim().is_empty() || process_matches(&window.process, &target.process);
    title_ok && process_ok
}

/// The matching window that was active most recently (the first one in Z order).
pub fn find_target(target: &WindowTarget) -> Option<Hwnd> {
    if target.is_empty() {
        return None;
    }
    top_windows().into_iter().find(|window| window_fits(window, target)).map(|window| window.hwnd)
}

/// Text for messages: what the user asked to find.
pub fn describe_target(target: &WindowTarget) -> String {
    let title = target.title.trim();
    let process = target.process.trim();
    match (title.is_empty(), process.is_empty()) {
        (false, false) => format!("«{title}» ({process})"),
        (false, true) => format!("«{title}»"),
        (true, _) => process.to_string(),
    }
}

pub fn window_exists(title: &str, mode: TitleMatch) -> bool {
    top_windows().iter().any(|window| title_matches(&window.title, title, mode))
}

pub fn is_window(hwnd: Hwnd) -> bool {
    // SAFETY: IsWindow accepts any handle value.
    unsafe { IsWindow(Some(hwnd.raw())) }.as_bool()
}

pub fn foreground() -> Hwnd {
    // SAFETY: no preconditions.
    Hwnd::from_raw(unsafe { GetForegroundWindow() })
}

/// Restores a minimised window and asks Windows to bring it to the foreground.
/// Windows may refuse; the caller checks `foreground()` afterwards.
pub fn request_focus(hwnd: Hwnd) {
    // SAFETY: the calls accept any handle value and fail harmlessly on a stale one.
    unsafe {
        if IsIconic(hwnd.raw()).as_bool() {
            let _ = ShowWindow(hwnd.raw(), SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd.raw());
    }
}

/// Whether the window under the point belongs to ClickIt itself (the badge, the main window).
pub fn own_window_at(x: i32, y: i32) -> bool {
    // SAFETY: neither call has preconditions; null handles are checked.
    let top = unsafe {
        let hwnd = WindowFromPoint(POINT { x, y });
        if hwnd.0.is_null() {
            return false;
        }
        // The point is over a child window of the web view, and that one belongs to
        // the WebView2 process. The top-level window above it is the one ClickIt made.
        GetAncestor(hwnd, GA_ROOT)
    };
    !top.0.is_null() && window_pid(top) == std::process::id()
}

/// Client area of a window in virtual-screen pixels.
pub fn client_rect(hwnd: Hwnd) -> Option<Rect> {
    let mut rect = RECT::default();
    let mut origin = POINT::default();
    // SAFETY: both out pointers are valid for the calls; failures are reported by the return values.
    unsafe {
        GetClientRect(hwnd.raw(), &mut rect).ok()?;
        if !ClientToScreen(hwnd.raw(), &mut origin).as_bool() {
            return None;
        }
    }
    Some(Rect { x: origin.x, y: origin.y, width: rect.right - rect.left, height: rect.bottom - rect.top })
}

/// Outer rectangle of a window in virtual-screen pixels.
pub fn window_rect(hwnd: Hwnd) -> Option<Rect> {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a valid out pointer for the call.
    unsafe { GetWindowRect(hwnd.raw(), &mut rect).ok()? };
    Some(Rect { x: rect.left, y: rect.top, width: rect.right - rect.left, height: rect.bottom - rect.top })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_and_process_names_are_compared_without_case() {
        assert!(title_matches("Untitled - Notepad", "notepad", TitleMatch::Contains));
        assert!(!title_matches("Untitled - Notepad", "notepad", TitleMatch::Exact));
        assert!(title_matches("Notepad", " notepad ", TitleMatch::Exact));
        assert!(process_matches("Notepad.EXE", "notepad.exe"));
        assert!(process_matches("notepad.exe", "Notepad"));
        assert!(!process_matches("notepad.exe", ""));
        assert!(!process_matches("notepad2.exe", "notepad"));
    }
}
