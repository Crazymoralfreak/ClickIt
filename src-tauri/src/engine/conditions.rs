//! Geometry shared by the conditions and the capture tools.

use std::path::Path;
use std::time::Instant;

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::input;
use crate::model::scenario::{Area, Condition, CoordSpace, Position, ScenarioMode, WindowTarget};
use crate::model::validate::parse_color;
use crate::runtime::{CaptureResult, Rect};
use crate::screen::{self, Template};
use crate::target;

/// Thresholds "Check now" falls back to when an image is not found at its own one.
const LOWER_THRESHOLDS: [f64; 6] = [0.9, 0.85, 0.8, 0.7, 0.6, 0.5];

/// Search area of an image condition in virtual-screen pixels. `client` is the
/// client area of the target window, when the scenario has one. `None` means
/// the area cannot be resolved (it needs a target, or nothing of it is left
/// inside the window).
pub fn area_rect(area: &Area, screen: Rect, client: Option<Rect>) -> Option<Rect> {
    match area {
        Area::Screen => Some(screen),
        Area::Client => client,
        Area::Rect { space: CoordSpace::Absolute, x, y, width, height } => {
            Some(Rect { x: *x, y: *y, width: *width, height: *height })
        }
        Area::Rect { space: CoordSpace::Client, x, y, width, height } => {
            let client = client?;
            Rect { x: client.x + *x, y: client.y + *y, width: *width, height: *height }.intersect(&client)
        }
    }
}

/// What is under the cursor right now: position, colour and, when a window
/// target is given, the position inside its client area.
pub fn capture_point(target: Option<&WindowTarget>) -> CaptureResult {
    let (x, y) = input::cursor_pos();
    let mut result =
        CaptureResult { x, y, client_x: None, client_y: None, color: "#000000".to_string(), error: None };
    match screen::pixel(x, y) {
        Ok(color) => result.color = screen::color_text(color),
        Err(error) => result.error = Some(error),
    }
    if let Some(target) = target {
        match target::find_target(target).and_then(target::client_rect) {
            Some(client) if client.contains(x, y) => {
                result.client_x = Some(x - client.x);
                result.client_y = Some(y - client.y);
            }
            Some(_) => result.error = Some(AppError::new("captureOutsideTarget")),
            None => {
                result.error =
                    Some(AppError::new("targetNotFound").with("target", target::describe_target(target)));
            }
        }
    }
    result
}

/// What one look at a condition showed. Nothing is clicked.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    /// Whether the condition holds right now.
    pub holds: bool,
    /// For an image: whether it is on the screen, whatever the condition asks for.
    pub found: Option<bool>,
    /// Centre of the found image, in virtual-screen pixels.
    pub x: Option<i32>,
    pub y: Option<i32>,
    /// The same point inside the target window, in the "Window" mode.
    pub client_x: Option<i32>,
    pub client_y: Option<i32>,
    /// Similarity of the found image, 0..=1.
    pub score: Option<f64>,
    /// Set when the image is not found at its threshold but would be at this similarity.
    pub would_match_at: Option<f64>,
    /// For a pixel: the colour that is there now.
    pub color: Option<String>,
    /// How long the look took.
    pub ms: u64,
}

/// Whether the look needs the screen, so the caller takes the ClickIt window out of the way.
pub fn needs_screen(condition: &Condition) -> bool {
    match condition {
        Condition::Pixel { .. } | Condition::ImageFound { .. } | Condition::ImageGone { .. } => true,
        Condition::Window { .. } | Condition::Process { .. } => false,
        Condition::All { items } | Condition::Any { items } => items.iter().any(needs_screen),
    }
}

/// Looks at a condition once, the way a run does. `dir` is the folder with
/// the template images. In the "Window" mode the target window has to exist.
pub fn check_now(mode: ScenarioMode, window: &WindowTarget, condition: &Condition, dir: &Path) -> AppResult<CheckResult> {
    let started = Instant::now();
    let client = match mode {
        ScenarioMode::Screen => None,
        ScenarioMode::Window => Some(
            target::find_target(window)
                .and_then(target::client_rect)
                .ok_or_else(|| AppError::new("targetNotFound").with("target", target::describe_target(window)))?,
        ),
    };
    let mut result = CheckResult::default();
    match condition {
        Condition::Pixel { point, color, tolerance } => {
            let (x, y) = match (point, client) {
                (Position::Absolute { x, y }, _) => (*x, *y),
                (Position::Client { x, y }, Some(client)) => (client.x + *x, client.y + *y),
                (Position::Client { .. }, None) => return Err(AppError::new("invalid")),
                (Position::Cursor, _) => input::cursor_pos(),
            };
            let actual = screen::pixel(x, y)?;
            let wanted = parse_color(color).ok_or_else(|| AppError::new("invalid"))?;
            result.holds = screen::color_within(actual, wanted, (*tolerance).clamp(0, 255) as u8);
            result.color = Some(screen::color_text(actual));
            result.x = Some(x);
            result.y = Some(y);
        }
        Condition::ImageFound { template, area, threshold } | Condition::ImageGone { template, area, threshold } => {
            let template = Template::load(&dir.join(template))?;
            let rect = area_rect(area, screen::virtual_screen(), client).ok_or_else(|| AppError::new("captureFailed"))?;
            let frame = screen::capture(rect)?;
            let found = screen::find_template_scored(&frame, &template, *threshold);
            if let Some(((left, top), score)) = found {
                let x = frame.rect.x + left + template.width / 2;
                let y = frame.rect.y + top + template.height / 2;
                result.x = Some(x);
                result.y = Some(y);
                result.client_x = client.map(|client| x - client.x);
                result.client_y = client.map(|client| y - client.y);
                result.score = Some(score);
            } else {
                // Tell at which similarity the picture would have been found.
                result.would_match_at = LOWER_THRESHOLDS
                    .iter()
                    .filter(|lower| **lower < *threshold)
                    .find_map(|lower| screen::find_template_scored(&frame, &template, *lower))
                    .map(|(_, score)| score);
            }
            result.found = Some(found.is_some());
            result.holds = found.is_some() == matches!(condition, Condition::ImageFound { .. });
        }
        Condition::Window { title, title_match } => result.holds = target::window_exists(title, *title_match),
        Condition::Process { name } => result.holds = target::process_running(name),
        Condition::All { items } | Condition::Any { items } => {
            let all = matches!(condition, Condition::All { .. });
            let mut holds = all;
            for item in items {
                if all != holds {
                    break;
                }
                let part = check_now(mode, window, item, dir)?;
                holds = part.holds;
                if part.found.is_some() {
                    result.found = part.found;
                    result.x = part.x;
                    result.y = part.y;
                    result.client_x = part.client_x;
                    result.client_y = part.client_y;
                    result.score = part.score;
                    result.would_match_at = part.would_match_at;
                }
                if part.color.is_some() {
                    result.color = part.color;
                }
            }
            result.holds = holds && !items.is_empty();
        }
    }
    result.ms = started.elapsed().as_millis() as u64;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect { x: -1920, y: 0, width: 3840, height: 1080 };
    const CLIENT: Rect = Rect { x: 100, y: 200, width: 800, height: 600 };

    #[test]
    fn client_rectangles_move_with_the_window_and_stay_inside_it() {
        let area = Area::Rect { space: CoordSpace::Client, x: 700, y: 10, width: 300, height: 50 };
        assert_eq!(
            area_rect(&area, SCREEN, Some(CLIENT)),
            Some(Rect { x: 800, y: 210, width: 100, height: 50 })
        );
        assert_eq!(area_rect(&area, SCREEN, None), None);
        assert_eq!(area_rect(&Area::Client, SCREEN, Some(CLIENT)), Some(CLIENT));
        assert_eq!(area_rect(&Area::Screen, SCREEN, None), Some(SCREEN));
    }
}
