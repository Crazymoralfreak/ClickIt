//! Mouse and keyboard emulation through plain `SendInput`. Nothing here hides
//! or disguises the input: Windows marks these events as injected, as usual.

pub mod keys;

use std::mem::size_of;

use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC, MOUSEEVENTF_ABSOLUTE,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT,
    MOUSE_EVENT_FLAGS, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SetCursorPos, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN,
};

use crate::error::{AppError, AppResult};
use crate::model::scenario::{Modifiers, MouseButton};
use keys::{VirtualKey, VK_ALT_L, VK_CONTROL_L, VK_SHIFT_L, VK_WIN_L};

const WHEEL_DELTA: i32 = 120;
const VK_RETURN: u16 = 0x0D;
const VK_TAB: u16 = 0x09;

/// Cursor position in virtual-screen pixels.
pub fn cursor_pos() -> (i32, i32) {
    let mut point = POINT::default();
    // SAFETY: `point` is a valid, writable POINT for the duration of the call.
    let _ = unsafe { GetCursorPos(&mut point) };
    (point.x, point.y)
}

fn send(inputs: &[INPUT]) -> AppResult<()> {
    // SAFETY: the slice holds fully initialised INPUT structures and the size matches the type.
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(AppError::new("inputFailed"))
    }
}

fn mouse_input(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

fn key_input(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

/// Sends input and remembers what it left pressed, so that a stop or a pause
/// can release exactly that.
#[derive(Default)]
pub struct Input {
    held_buttons: Vec<MouseButton>,
    held_keys: Vec<VirtualKey>,
}

impl Input {
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves the cursor to a virtual-screen pixel.
    pub fn move_to(&mut self, x: i32, y: i32) -> AppResult<()> {
        // SAFETY: GetSystemMetrics has no preconditions.
        let (left, top, width, height) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        let scale = |value: i32, origin: i32, size: i32| -> i32 {
            let span = f64::from((size - 1).max(1));
            (f64::from(value - origin) * 65535.0 / span).round() as i32
        };
        let flags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
        send(&[mouse_input(flags, scale(x, left, width), scale(y, top, height), 0)])?;
        // The 0..65535 grid can land one pixel off; settle on the exact pixel.
        if cursor_pos() != (x, y) {
            // SAFETY: SetCursorPos has no preconditions.
            let _ = unsafe { SetCursorPos(x, y) };
        }
        Ok(())
    }

    pub fn button(&mut self, button: MouseButton, down: bool) -> AppResult<()> {
        let flags = match (button, down) {
            (MouseButton::Left, true) => MOUSEEVENTF_LEFTDOWN,
            (MouseButton::Left, false) => MOUSEEVENTF_LEFTUP,
            (MouseButton::Right, true) => MOUSEEVENTF_RIGHTDOWN,
            (MouseButton::Right, false) => MOUSEEVENTF_RIGHTUP,
            (MouseButton::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
            (MouseButton::Middle, false) => MOUSEEVENTF_MIDDLEUP,
        };
        send(&[mouse_input(flags, 0, 0, 0)])?;
        self.held_buttons.retain(|held| *held != button);
        if down {
            self.held_buttons.push(button);
        }
        Ok(())
    }

    /// One wheel notch; `up` scrolls away from the user.
    pub fn wheel(&mut self, up: bool) -> AppResult<()> {
        let delta = if up { WHEEL_DELTA } else { -WHEEL_DELTA };
        send(&[mouse_input(MOUSEEVENTF_WHEEL, 0, 0, delta)])
    }

    pub fn key(&mut self, key: VirtualKey, down: bool) -> AppResult<()> {
        // SAFETY: MapVirtualKeyW has no preconditions.
        let scan = unsafe { MapVirtualKeyW(u32::from(key.vk), MAPVK_VK_TO_VSC) } as u16;
        let mut flags = KEYBD_EVENT_FLAGS(0);
        if key.extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !down {
            flags |= KEYEVENTF_KEYUP;
        }
        send(&[key_input(key.vk, scan, flags)])?;
        self.held_keys.retain(|held| *held != key);
        if down {
            self.held_keys.push(key);
        }
        Ok(())
    }

    /// Presses or releases the modifiers of a key step, in a fixed order.
    pub fn modifiers(&mut self, modifiers: Modifiers, down: bool) -> AppResult<()> {
        let mut list = [
            (modifiers.ctrl, VK_CONTROL_L),
            (modifiers.alt, VK_ALT_L),
            (modifiers.shift, VK_SHIFT_L),
            (modifiers.win, VK_WIN_L),
        ];
        if !down {
            list.reverse();
        }
        for (wanted, vk) in list {
            if wanted {
                self.key(VirtualKey { vk, extended: vk == VK_WIN_L }, down)?;
            }
        }
        Ok(())
    }

    /// Types characters as Unicode input, independent of the keyboard layout.
    /// Line breaks and tabs go out as the Enter and Tab keys.
    pub fn text(&mut self, text: &str) -> AppResult<()> {
        let mut inputs = Vec::new();
        for ch in text.chars() {
            match ch {
                '\r' => {}
                '\n' | '\t' => {
                    let vk = if ch == '\n' { VK_RETURN } else { VK_TAB };
                    // SAFETY: MapVirtualKeyW has no preconditions.
                    let scan = unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_VSC) } as u16;
                    inputs.push(key_input(vk, scan, KEYBD_EVENT_FLAGS(0)));
                    inputs.push(key_input(vk, scan, KEYEVENTF_KEYUP));
                }
                _ => {
                    let mut units = [0u16; 2];
                    for unit in ch.encode_utf16(&mut units) {
                        inputs.push(key_input(0, *unit, KEYEVENTF_UNICODE));
                        inputs.push(key_input(0, *unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                    }
                }
            }
        }
        if inputs.is_empty() {
            Ok(())
        } else {
            send(&inputs)
        }
    }

    pub fn holds_anything(&self) -> bool {
        !self.held_buttons.is_empty() || !self.held_keys.is_empty()
    }

    /// Releases every button and key this instance pressed and did not release.
    pub fn release_all(&mut self) {
        for button in std::mem::take(&mut self.held_buttons) {
            let _ = self.button(button, false);
        }
        for key in std::mem::take(&mut self.held_keys).into_iter().rev() {
            let _ = self.key(key, false);
        }
    }
}
