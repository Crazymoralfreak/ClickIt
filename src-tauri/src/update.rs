//! Updates from the GitHub releases of ClickIt. This is the only thing the
//! application ever asks the network for, and only when the setting allows
//! it or the user presses "Check for updates".
//!
//! A copy put in place by the installer downloads the new installer, checks
//! its signature and runs it once nothing is running or being recorded; the
//! installer starts ClickIt again. A portable copy cannot replace itself, so
//! it only tells that a newer version exists.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tauri::Emitter;
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::app::Core;
use crate::runtime::{UpdateState, UpdateStatus};
use crate::shell::text::text;
use crate::shell::{notify, window};

/// The check must not compete with the start of the application.
const STARTUP_DELAY: Duration = Duration::from_secs(5);
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
/// How often a downloaded update looks whether ClickIt has become idle.
const IDLE_POLL: Duration = Duration::from_secs(2);
/// Time for the main window to save what is being edited before the process ends.
const SAVE_DELAY: Duration = Duration::from_millis(1500);
/// The installer leaves this file next to the program; a portable copy has none.
const UNINSTALLER: &str = "uninstall.exe";

/// True for a copy that was put in place by the installer.
pub fn installed_copy() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|folder| folder.join(UNINSTALLER).is_file()))
        .unwrap_or(false)
}

/// What is known about updates in this session.
pub struct Tracker {
    status: Mutex<UpdateStatus>,
    /// One check at a time, whoever asked for it.
    busy: AtomicBool,
}

impl Tracker {
    pub fn new() -> Self {
        let status = UpdateStatus { state: UpdateState::Idle, version: None, portable: !installed_copy() };
        Self { status: Mutex::new(status), busy: AtomicBool::new(false) }
    }

    pub fn status(&self) -> UpdateStatus {
        self.status.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }

    fn set(&self, state: UpdateState, version: Option<&str>) -> UpdateStatus {
        let mut status = self.status.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        status.state = state;
        status.version = version.map(str::to_string);
        status.clone()
    }
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

/// The check at startup: after a short delay and only when the settings allow it.
pub fn check_at_startup(core: &Arc<Core>) {
    let core = Arc::clone(core);
    spawn(move || {
        thread::sleep(STARTUP_DELAY);
        if core.settings().check_updates {
            run(&core);
        }
    });
}

/// The check asked for by the user. The result arrives as `update:status` events.
pub fn check_now(core: &Arc<Core>) {
    let core = Arc::clone(core);
    spawn(move || run(&core));
}

fn spawn(work: impl FnOnce() + Send + 'static) {
    // Without a thread there is simply no check.
    let _ = thread::Builder::new().name("clickit-update".to_string()).spawn(work);
}

fn set(core: &Core, state: UpdateState, version: Option<&str>) {
    let status = core.updates.set(state, version);
    let _ = core.app().emit("update:status", &status);
}

fn run(core: &Arc<Core>) {
    if core.updates.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    check_and_install(core);
    core.updates.busy.store(false, Ordering::SeqCst);
}

fn find(core: &Core) -> tauri_plugin_updater::Result<Option<Update>> {
    let updater = core.app().updater_builder().timeout(CHECK_TIMEOUT).build()?;
    tauri::async_runtime::block_on(updater.check())
}

fn check_and_install(core: &Arc<Core>) {
    set(core, UpdateState::Checking, None);
    let mut update = match find(core) {
        Ok(Some(update)) => update,
        Ok(None) => {
            set(core, UpdateState::UpToDate, None);
            return;
        }
        // No network, or no release to compare with: not worth a journal entry at every start.
        Err(_) => {
            set(core, UpdateState::Failed, None);
            return;
        }
    };
    let version = update.version.clone();
    core.journal.add("updateFound", &[("version", version.clone())]);

    if core.updates.status().portable {
        set(core, UpdateState::Available, Some(&version));
        announce(core, "native.notify.updateAvailable", "native.notify.updateAvailableText", &version);
        return;
    }

    set(core, UpdateState::Downloading, Some(&version));
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    // The signature of the downloaded installer is checked here, before anything is run.
    let bytes = match tauri::async_runtime::block_on(update.download(|_, _| {}, || {})) {
        Ok(bytes) => bytes,
        Err(error) => {
            failed(core, &version, &error.to_string());
            return;
        }
    };

    set(core, UpdateState::Ready, Some(&version));
    // A run or a recording is never cut short by an update.
    while !core.begin_install() {
        if core.is_quitting() {
            return;
        }
        thread::sleep(IDLE_POLL);
    }

    set(core, UpdateState::Installing, Some(&version));
    core.journal.add("updateInstall", &[("version", version.clone())]);
    announce(core, "native.notify.updateInstalling", "native.notify.updateInstallingText", &version);
    thread::sleep(SAVE_DELAY);
    // On success this call does not return: the installer takes over and the process ends.
    if let Err(error) = update.install(&bytes) {
        core.cancel_install();
        failed(core, &version, &error.to_string());
    }
}

fn failed(core: &Core, version: &str, message: &str) {
    core.journal.add("updateFailed", &[("version", version.to_string()), ("message", message.to_string())]);
    set(core, UpdateState::Failed, Some(version));
}

/// The main window shows its own message; a notification is for when it is hidden.
fn announce(core: &Core, title_key: &str, body_key: &str, version: &str) {
    if window::main_visible(core.app()) {
        return;
    }
    let lang = core.lang();
    notify(core.app(), &text(&lang, title_key, &[]), &text(&lang, body_key, &[("version", version)]));
}
