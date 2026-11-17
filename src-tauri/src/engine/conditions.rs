//! Geometry shared by the conditions and the capture tools.

use crate::error::AppError;
use crate::input;
use crate::model::scenario::{Area, CoordSpace, WindowTarget};
use crate::runtime::{CaptureResult, Rect};
use crate::screen;
use crate::target;

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
