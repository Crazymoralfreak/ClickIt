//! Timer precision, random pauses and the "cursor in the corner" stop.

use windows::Win32::Foundation::POINT;
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::model::scenario::Pause;

/// Holds the 1 ms multimedia timer period for the duration of a run.
pub struct TimerResolution;

impl TimerResolution {
    pub fn enter() -> Self {
        // SAFETY: paired with timeEndPeriod in Drop; nested calls are allowed by Windows.
        unsafe {
            let _ = timeBeginPeriod(1);
        }
        Self
    }
}

impl Drop for TimerResolution {
    fn drop(&mut self) {
        // SAFETY: matches the Begin call of this instance.
        unsafe {
            let _ = timeEndPeriod(1);
        }
    }
}

/// Picks a whole number of milliseconds from `from..=to`.
pub fn pick_ms(pause: Pause) -> u64 {
    let from = pause.from_ms.max(0) as u64;
    let to = pause.to_ms.max(0) as u64;
    if to <= from {
        from
    } else {
        from + fastrand::u64(..=(to - from))
    }
}

/// Top-left corner of the primary monitor is (0, 0) in the virtual desktop.
pub fn cursor_in_corner() -> bool {
    let mut point = POINT::default();
    // SAFETY: GetCursorPos has no preconditions.
    unsafe {
        if GetCursorPos(&mut point).is_err() {
            return false;
        }
    }
    (0..3).contains(&point.x) && (0..3).contains(&point.y)
}
