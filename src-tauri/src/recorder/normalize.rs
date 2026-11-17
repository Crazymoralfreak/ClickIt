//! Turns raw mouse and keyboard events into scenario steps. Pure: no Windows
//! calls, so the rules of the recording can be tested.
//!
//! - the gap before an action becomes the pause after the previous step:
//!   under 50 ms it is 0, otherwise it is rounded to 10 ms;
//! - press and release of a button in one place is a click, in two places a drag;
//! - plain characters typed in a row are merged into one text step;
//! - combinations and special keys stay separate key steps;
//! - a modifier held during a mouse action is recorded as hold and release steps.

use crate::input::keys::{code_from_vk, modifier_of, ModifierKey};
use crate::model::scenario::{
    ClickAction, KeyAction, Modifiers, MouseButton, Pause, Position, ScrollDirection, Step, StepKind,
};

const MIN_PAUSE_MS: u64 = 50;
const PAUSE_ROUND_MS: u64 = 10;
/// A release this far from the press makes the pair a drag.
const DRAG_DISTANCE_PX: i32 = 5;
/// Characters typed with a longer break start a new text step.
const TEXT_MERGE_MS: u64 = 2000;
/// Wheel notches with a longer break start a new scroll step.
const WHEEL_MERGE_MS: u64 = 200;
const MAX_NOTCHES: i64 = 100;

#[derive(Debug, Clone, PartialEq)]
pub enum RawEvent {
    MouseDown { button: MouseButton, position: Position },
    MouseUp { button: MouseButton, position: Position },
    Wheel { up: bool, position: Position },
    /// `ch` is the character the key types in the current layout, when it is a
    /// plain one. `modifiers` is what was held at that moment.
    KeyDown { vk: u16, extended: bool, ch: Option<char>, modifiers: Modifiers },
    KeyUp { vk: u16, extended: bool },
}

struct HeldModifier {
    vk: u16,
    extended: bool,
    /// A "hold" step was written for it because a mouse action happened meanwhile.
    written: bool,
    /// Nothing else was pressed since it went down.
    alone: bool,
}

#[derive(Default)]
pub struct Normalizer {
    steps: Vec<Step>,
    /// When the last written action ended.
    last_end: Option<u64>,
    pressed: Option<(MouseButton, Position, u64)>,
    held: Vec<HeldModifier>,
    /// The last step is a text step that may still grow.
    text_open: bool,
    /// The last step is a scroll step that may still grow.
    wheel_open: bool,
}

fn coordinates(position: &Position) -> Option<(i32, i32)> {
    match position {
        Position::Cursor => None,
        Position::Absolute { x, y } | Position::Client { x, y } => Some((*x, *y)),
    }
}

fn moved_far(from: &Position, to: &Position) -> bool {
    match (coordinates(from), coordinates(to)) {
        (Some(a), Some(b)) => (a.0 - b.0).abs() >= DRAG_DISTANCE_PX || (a.1 - b.1).abs() >= DRAG_DISTANCE_PX,
        _ => false,
    }
}

/// Pause stored for a gap between two actions.
pub fn pause_for_gap(gap_ms: u64) -> u64 {
    if gap_ms < MIN_PAUSE_MS {
        0
    } else {
        (gap_ms + PAUSE_ROUND_MS / 2) / PAUSE_ROUND_MS * PAUSE_ROUND_MS
    }
}

impl Normalizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Ends the recording. A modifier that was written as "hold" and is still
    /// down gets its release, so that a replay never leaves a key pressed.
    pub fn finish(mut self) -> Vec<Step> {
        for held in std::mem::take(&mut self.held) {
            if held.written {
                if let Some(key) = code_from_vk(held.vk, held.extended) {
                    self.steps.push(Step::new(StepKind::Key {
                        key,
                        action: KeyAction::Up,
                        modifiers: Modifiers::default(),
                    }));
                }
            }
        }
        self.steps
    }

    /// Forgets a button that went down: its release happened where nothing is recorded.
    pub fn cancel_press(&mut self) {
        self.pressed = None;
    }

    /// The modifiers that are down belong to a combination that is not recorded
    /// (the stop hotkey), so releasing them must not leave a step.
    pub fn forget_combination(&mut self) {
        for held in &mut self.held {
            held.alone = false;
        }
    }

    fn held_modifiers(&self) -> Modifiers {
        let mut modifiers = Modifiers::default();
        for held in &self.held {
            match modifier_of(held.vk) {
                Some(ModifierKey::Ctrl) => modifiers.ctrl = true,
                Some(ModifierKey::Alt) => modifiers.alt = true,
                Some(ModifierKey::Shift) => modifiers.shift = true,
                Some(ModifierKey::Win) => modifiers.win = true,
                None => {}
            }
        }
        modifiers
    }

    /// Writes the pause since the previous action into the previous step.
    fn close_gap(&mut self, start: u64) {
        if let (Some(end), Some(previous)) = (self.last_end, self.steps.last_mut()) {
            previous.pause_after = Pause::exact(pause_for_gap(start.saturating_sub(end)) as i64);
        }
    }

    fn write(&mut self, kind: StepKind, start: u64, end: u64) {
        self.close_gap(start);
        self.steps.push(Step::new(kind));
        self.last_end = Some(end);
        self.text_open = false;
        self.wheel_open = false;
    }

    /// Before a mouse action: modifiers that are down become "hold" steps.
    fn write_held_modifiers(&mut self, at: u64) {
        for index in 0..self.held.len() {
            self.held[index].alone = false;
            if self.held[index].written {
                continue;
            }
            self.held[index].written = true;
            if let Some(key) = code_from_vk(self.held[index].vk, self.held[index].extended) {
                self.write(StepKind::Key { key, action: KeyAction::Down, modifiers: Modifiers::default() }, at, at);
            }
        }
    }

    pub fn push(&mut self, at: u64, event: RawEvent) {
        match event {
            RawEvent::MouseDown { button, position } => {
                self.write_held_modifiers(at);
                self.pressed = Some((button, position, at));
            }
            RawEvent::MouseUp { button, position } => {
                let Some((pressed_button, from, started)) = self.pressed.take() else {
                    return;
                };
                if pressed_button != button {
                    return;
                }
                let kind = if moved_far(&from, &position) {
                    StepKind::Drag { button, from, to: position }
                } else {
                    StepKind::Click { button, action: ClickAction::Click, position: from }
                };
                self.write(kind, started, at);
            }
            RawEvent::Wheel { up, position } => {
                self.write_held_modifiers(at);
                let direction = if up { ScrollDirection::Up } else { ScrollDirection::Down };
                let recent = self.last_end.is_some_and(|end| at.saturating_sub(end) < WHEEL_MERGE_MS);
                if self.wheel_open && recent {
                    if let Some(Step { kind: StepKind::Scroll { direction: last, notches, .. }, .. }) =
                        self.steps.last_mut()
                    {
                        if *last == direction && *notches < MAX_NOTCHES {
                            *notches += 1;
                            self.last_end = Some(at);
                            return;
                        }
                    }
                }
                self.write(StepKind::Scroll { direction, notches: 1, position }, at, at);
                self.wheel_open = true;
            }
            RawEvent::KeyDown { vk, extended, ch, modifiers } => {
                if modifier_of(vk).is_some() {
                    // Auto-repeat of a held modifier sends more "down" events.
                    if !self.held.iter().any(|held| held.vk == vk) {
                        // In Alt+Shift the last one pressed is the key, the rest are its modifiers.
                        self.forget_combination();
                        self.held.push(HeldModifier { vk, extended, written: false, alone: true });
                    }
                    return;
                }
                for held in &mut self.held {
                    held.alone = false;
                }
                let plain = !modifiers.ctrl && !modifiers.alt && !modifiers.win;
                match ch {
                    Some(ch) if plain => self.type_char(ch, at),
                    _ => {
                        if let Some(key) = code_from_vk(vk, extended) {
                            self.write(StepKind::Key { key, action: KeyAction::Press, modifiers }, at, at);
                        }
                    }
                }
            }
            RawEvent::KeyUp { vk, .. } => {
                let Some(index) = self.held.iter().position(|held| held.vk == vk) else {
                    return;
                };
                let held = self.held.remove(index);
                let Some(key) = code_from_vk(held.vk, held.extended) else {
                    return;
                };
                if held.written {
                    self.write(StepKind::Key { key, action: KeyAction::Up, modifiers: Modifiers::default() }, at, at);
                } else if held.alone {
                    // A modifier tapped on its own (the Win key) or as the last key
                    // of a combination of modifiers only (Alt+Shift).
                    let modifiers = self.held_modifiers();
                    self.write(StepKind::Key { key, action: KeyAction::Press, modifiers }, at, at);
                }
            }
        }
    }

    fn type_char(&mut self, ch: char, at: u64) {
        let recent = self.last_end.is_some_and(|end| at.saturating_sub(end) < TEXT_MERGE_MS);
        if self.text_open && recent {
            if let Some(Step { kind: StepKind::Text { text }, .. }) = self.steps.last_mut() {
                text.push(ch);
                self.last_end = Some(at);
                return;
            }
        }
        self.write(StepKind::Text { text: ch.to_string() }, at, at);
        self.text_open = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: MouseButton = MouseButton::Left;

    fn at(x: i32, y: i32) -> Position {
        Position::Absolute { x, y }
    }

    fn key(vk: u16, ch: Option<char>, modifiers: Modifiers) -> RawEvent {
        RawEvent::KeyDown { vk, extended: false, ch, modifiers }
    }

    #[test]
    fn gaps_become_pauses_of_the_previous_step() {
        assert_eq!(pause_for_gap(49), 0);
        assert_eq!(pause_for_gap(50), 50);
        assert_eq!(pause_for_gap(124), 120);
        assert_eq!(pause_for_gap(125), 130);

        let mut n = Normalizer::new();
        n.push(1000, RawEvent::MouseDown { button: LEFT, position: at(10, 10) });
        n.push(1040, RawEvent::MouseUp { button: LEFT, position: at(11, 10) });
        n.push(1363, RawEvent::MouseDown { button: LEFT, position: at(50, 50) });
        n.push(1400, RawEvent::MouseUp { button: LEFT, position: at(50, 50) });
        let steps = n.finish();
        assert_eq!(steps.len(), 2);
        assert!(matches!(steps[0].kind, StepKind::Click { position: Position::Absolute { x: 10, y: 10 }, .. }));
        // 1363 - 1040 = 323 ms, rounded to 320.
        assert_eq!(steps[0].pause_after, Pause::exact(320));
        assert_eq!(steps[1].pause_after, Pause::exact(0));
    }

    #[test]
    fn press_and_release_in_two_places_is_a_drag() {
        let mut n = Normalizer::new();
        n.push(0, RawEvent::MouseDown { button: LEFT, position: at(10, 10) });
        n.push(300, RawEvent::MouseUp { button: LEFT, position: at(200, 40) });
        let steps = n.finish();
        assert_eq!(steps.len(), 1);
        assert!(matches!(
            steps[0].kind,
            StepKind::Drag { from: Position::Absolute { x: 10, y: 10 }, to: Position::Absolute { x: 200, y: 40 }, .. }
        ));
    }

    #[test]
    fn characters_merge_into_text_and_special_keys_stay_apart() {
        let none = Modifiers::default();
        let mut n = Normalizer::new();
        n.push(0, key(0x48, Some('h'), none));
        n.push(90, key(0x49, Some('i'), none));
        n.push(200, key(0x20, Some(' '), none));
        n.push(300, key(0x0D, None, none));
        n.push(400, key(0x43, Some('c'), Modifiers { ctrl: true, ..none }));
        n.push(500, key(0x5A, Some('я'), none));
        let steps = n.finish();
        assert_eq!(steps.len(), 4);
        assert!(matches!(&steps[0].kind, StepKind::Text { text } if text == "hi "));
        assert!(matches!(&steps[1].kind, StepKind::Key { key, action: KeyAction::Press, .. } if key == "Enter"));
        assert!(
            matches!(&steps[2].kind, StepKind::Key { key, modifiers, .. } if key == "KeyC" && modifiers.ctrl)
        );
        assert!(matches!(&steps[3].kind, StepKind::Text { text } if text == "я"));
    }

    #[test]
    fn modifier_held_during_a_click_is_recorded_as_hold_and_release() {
        let mut n = Normalizer::new();
        n.push(0, key(0xA2, None, Modifiers::default()));
        n.push(100, RawEvent::MouseDown { button: LEFT, position: at(5, 5) });
        n.push(130, RawEvent::MouseUp { button: LEFT, position: at(5, 5) });
        n.push(200, RawEvent::KeyUp { vk: 0xA2, extended: false });
        let steps = n.finish();
        assert_eq!(steps.len(), 3);
        assert!(matches!(&steps[0].kind, StepKind::Key { key, action: KeyAction::Down, .. } if key == "ControlLeft"));
        assert!(matches!(steps[1].kind, StepKind::Click { .. }));
        assert!(matches!(&steps[2].kind, StepKind::Key { key, action: KeyAction::Up, .. } if key == "ControlLeft"));
    }

    #[test]
    fn modifier_of_a_combination_leaves_no_step_but_a_lone_tap_does() {
        let mut n = Normalizer::new();
        n.push(0, key(0xA2, None, Modifiers::default()));
        n.push(50, key(0x56, Some('v'), Modifiers { ctrl: true, ..Modifiers::default() }));
        n.push(100, RawEvent::KeyUp { vk: 0xA2, extended: false });
        n.push(500, key(0x5B, None, Modifiers::default()));
        n.push(560, RawEvent::KeyUp { vk: 0x5B, extended: true });
        let steps = n.finish();
        assert_eq!(steps.len(), 2);
        assert!(matches!(&steps[0].kind, StepKind::Key { key, .. } if key == "KeyV"));
        assert!(matches!(&steps[1].kind, StepKind::Key { key, action: KeyAction::Press, .. } if key == "MetaLeft"));
    }

    #[test]
    fn combination_of_modifiers_only_is_one_key_step() {
        let mut n = Normalizer::new();
        n.push(0, key(0xA4, None, Modifiers::default()));
        n.push(40, key(0xA0, None, Modifiers::default()));
        n.push(90, RawEvent::KeyUp { vk: 0xA0, extended: false });
        n.push(130, RawEvent::KeyUp { vk: 0xA4, extended: false });
        let steps = n.finish();
        assert_eq!(steps.len(), 1);
        assert!(matches!(
            &steps[0].kind,
            StepKind::Key { key, action: KeyAction::Press, modifiers }
                if key == "ShiftLeft" && modifiers.alt && !modifiers.shift
        ));
    }

    #[test]
    fn stop_hotkey_modifiers_and_unfinished_holds_leave_nothing_pressed() {
        let mut n = Normalizer::new();
        n.push(0, key(0xA2, None, Modifiers::default()));
        n.forget_combination();
        n.push(80, RawEvent::KeyUp { vk: 0xA2, extended: false });
        assert!(n.is_empty());

        n.push(200, key(0xA0, None, Modifiers::default()));
        n.push(300, RawEvent::MouseDown { button: LEFT, position: at(5, 5) });
        n.push(320, RawEvent::MouseUp { button: LEFT, position: at(5, 5) });
        let steps = n.finish();
        assert_eq!(steps.len(), 3);
        assert!(matches!(&steps[2].kind, StepKind::Key { key, action: KeyAction::Up, .. } if key == "ShiftLeft"));
    }

    #[test]
    fn wheel_notches_in_one_direction_add_up() {
        let mut n = Normalizer::new();
        for (time, up) in [(0, false), (40, false), (90, false), (150, true)] {
            n.push(time, RawEvent::Wheel { up, position: at(1, 1) });
        }
        let steps = n.finish();
        assert_eq!(steps.len(), 2);
        assert!(matches!(steps[0].kind, StepKind::Scroll { direction: ScrollDirection::Down, notches: 3, .. }));
        assert!(matches!(steps[1].kind, StepKind::Scroll { direction: ScrollDirection::Up, notches: 1, .. }));
    }
}
