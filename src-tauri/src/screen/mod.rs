//! Screen capture and matching: the colour of a pixel and a PNG template.
//! Capture goes through GDI `BitBlt`; matching is done in our own memory.

use std::mem::size_of;
use std::path::Path;
use std::thread;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use image::RgbaImage;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HGDIOBJ, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use crate::error::{AppError, AppResult};
use crate::runtime::Rect;

/// Searches with more positions than this are split between threads.
const PARALLEL_FROM_POSITIONS: usize = 400_000;
/// Around the first match, the best position within this many pixels is taken.
const REFINE_RADIUS: i32 = 2;

/// Pixels of a screen rectangle: BGRA, row by row from the top.
pub struct Frame {
    /// Where the pixels came from, in virtual-screen coordinates.
    pub rect: Rect,
    data: Vec<u8>,
}

impl Frame {
    /// Colour of a pixel given in frame coordinates, as (r, g, b).
    pub fn rgb(&self, x: i32, y: i32) -> Option<(u8, u8, u8)> {
        if x < 0 || y < 0 || x >= self.rect.width || y >= self.rect.height {
            return None;
        }
        let at = (y as usize * self.rect.width as usize + x as usize) * 4;
        Some((self.data[at + 2], self.data[at + 1], self.data[at]))
    }

    pub fn save_png(&self, path: &Path) -> AppResult<()> {
        let mut rgba = Vec::with_capacity(self.data.len());
        for pixel in self.data.chunks_exact(4) {
            rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
        }
        let image = RgbaImage::from_raw(self.rect.width as u32, self.rect.height as u32, rgba)
            .ok_or_else(|| AppError::new("captureFailed"))?;
        image.save(path).map_err(|error| AppError::new("io").with("message", error))
    }
}

/// `#RRGGBB` text of a colour.
pub fn color_text((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// True when every channel differs by at most `tolerance`.
pub fn color_within(actual: (u8, u8, u8), wanted: (u8, u8, u8), tolerance: u8) -> bool {
    actual.0.abs_diff(wanted.0) <= tolerance
        && actual.1.abs_diff(wanted.1) <= tolerance
        && actual.2.abs_diff(wanted.2) <= tolerance
}

/// Bounds of the virtual desktop: all monitors together.
pub fn virtual_screen() -> Rect {
    // SAFETY: GetSystemMetrics has no preconditions.
    unsafe {
        Rect {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

/// Copies the part of `rect` that lies on the virtual screen.
pub fn capture(rect: Rect) -> AppResult<Frame> {
    let failed = || AppError::new("captureFailed");
    let area = rect.intersect(&virtual_screen()).ok_or_else(failed)?;
    let mut data = vec![0u8; area.width as usize * area.height as usize * 4];
    // SAFETY: every GDI object created here is released on every path before returning;
    // `data` is exactly width * height * 4 bytes, which is what GetDIBits writes for a
    // 32-bit top-down bitmap of that size.
    let ok = unsafe {
        let screen_dc = GetDC(None);
        if screen_dc.is_invalid() {
            return Err(failed());
        }
        let memory_dc = CreateCompatibleDC(Some(screen_dc));
        let bitmap = CreateCompatibleBitmap(screen_dc, area.width, area.height);
        let mut ok = !memory_dc.is_invalid() && !bitmap.is_invalid();
        if ok {
            let previous = SelectObject(memory_dc, HGDIOBJ(bitmap.0));
            ok = BitBlt(
                memory_dc,
                0,
                0,
                area.width,
                area.height,
                Some(screen_dc),
                area.x,
                area.y,
                SRCCOPY | CAPTUREBLT,
            )
            .is_ok();
            // The bitmap must not be selected into a DC while GetDIBits reads it.
            SelectObject(memory_dc, previous);
            if ok {
                let mut info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: area.width,
                        biHeight: -area.height,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let rows = GetDIBits(
                    memory_dc,
                    bitmap,
                    0,
                    area.height as u32,
                    Some(data.as_mut_ptr().cast()),
                    &mut info,
                    DIB_RGB_COLORS,
                );
                ok = rows == area.height;
            }
        }
        if !bitmap.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        if !memory_dc.is_invalid() {
            let _ = DeleteDC(memory_dc);
        }
        ReleaseDC(None, screen_dc);
        ok
    };
    if ok {
        Ok(Frame { rect: area, data })
    } else {
        Err(failed())
    }
}

/// Colour of one virtual-screen pixel.
pub fn pixel(x: i32, y: i32) -> AppResult<(u8, u8, u8)> {
    capture(Rect { x, y, width: 1, height: 1 })?.rgb(0, 0).ok_or_else(|| AppError::new("captureFailed"))
}

/// Template image: RGB, row by row from the top.
pub struct Template {
    pub width: i32,
    pub height: i32,
    data: Vec<u8>,
}

impl Template {
    pub fn load(path: &Path) -> AppResult<Self> {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_string();
        let unreadable = || AppError::new("templateUnreadable").with("file", &name);
        let image = image::open(path).map_err(|_| unreadable())?.to_rgb8();
        let (width, height) = (image.width() as i32, image.height() as i32);
        if width < 1 || height < 1 {
            return Err(unreadable());
        }
        Ok(Self { width, height, data: image.into_raw() })
    }
}

struct Matcher<'a> {
    frame: &'a Frame,
    template: &'a Template,
    /// Template pixels checked first, as (dx, dy).
    samples: Vec<(i32, i32)>,
    /// Largest sum of channel differences over the samples that still goes on to the full check.
    sample_budget: u64,
    /// Largest sum of channel differences over the whole template that still counts as a match.
    full_budget: u64,
}

impl Matcher<'_> {
    fn new<'a>(frame: &'a Frame, template: &'a Template, threshold: f64) -> Matcher<'a> {
        let allowed_mean = (1.0 - threshold.clamp(0.0, 1.0)) * 255.0;
        let samples = sample_points(template.width, template.height);
        let channels = template.width as f64 * template.height as f64 * 3.0;
        // The samples are a small part of the picture, so they get slack: a real
        // match may be worse than average exactly where the samples sit.
        let sample_budget = ((allowed_mean * 3.0 + 12.0) * samples.len() as f64 * 3.0) as u64;
        Matcher { frame, template, samples, sample_budget, full_budget: (allowed_mean * channels) as u64 }
    }

    fn difference_at(&self, left: i32, top: i32, dx: i32, dy: i32) -> u64 {
        let f = ((top + dy) as usize * self.frame.rect.width as usize + (left + dx) as usize) * 4;
        let t = (dy as usize * self.template.width as usize + dx as usize) * 3;
        let frame = &self.frame.data[f..f + 3];
        let template = &self.template.data[t..t + 3];
        u64::from(frame[2].abs_diff(template[0]))
            + u64::from(frame[1].abs_diff(template[1]))
            + u64::from(frame[0].abs_diff(template[2]))
    }

    /// Sum of channel differences at a position, `None` once it is over the budget.
    fn score(&self, left: i32, top: i32) -> Option<u64> {
        let mut sum = 0u64;
        for &(dx, dy) in &self.samples {
            sum += self.difference_at(left, top, dx, dy);
            if sum > self.sample_budget {
                return None;
            }
        }
        let width = self.template.width as usize;
        let mut sum = 0u64;
        for dy in 0..self.template.height as usize {
            let f = ((top as usize + dy) * self.frame.rect.width as usize + left as usize) * 4;
            let t = dy * width * 3;
            let frame_row = &self.frame.data[f..f + width * 4];
            let template_row = &self.template.data[t..t + width * 3];
            for (frame, template) in frame_row.chunks_exact(4).zip(template_row.chunks_exact(3)) {
                sum += u64::from(frame[2].abs_diff(template[0]))
                    + u64::from(frame[1].abs_diff(template[1]))
                    + u64::from(frame[0].abs_diff(template[2]));
            }
            if sum > self.full_budget {
                return None;
            }
        }
        Some(sum)
    }

    /// First match in rows `rows`, scanning left to right, top to bottom.
    fn first_in_rows(&self, rows: std::ops::Range<i32>, last_left: i32) -> Option<(i32, i32)> {
        rows.flat_map(|top| (0..=last_left).map(move |left| (left, top)))
            .find(|&(left, top)| self.score(left, top).is_some())
    }
}

/// A grid of up to 5x5 points spread over the template.
fn sample_points(width: i32, height: i32) -> Vec<(i32, i32)> {
    let axis = |size: i32| -> Vec<i32> {
        let count = size.min(5);
        (0..count).map(|i| if count == 1 { 0 } else { i * (size - 1) / (count - 1) }).collect()
    };
    let (xs, ys) = (axis(width), axis(height));
    ys.iter().flat_map(|&y| xs.iter().map(move |&x| (x, y))).collect()
}

/// Looks for `template` inside `frame`. Similarity of a position is
/// `1 - mean absolute channel difference / 255`; the first position that reaches
/// `threshold` is refined to the best one nearby. Returns the top-left corner
/// of the match in frame coordinates.
pub fn find_template(frame: &Frame, template: &Template, threshold: f64) -> Option<(i32, i32)> {
    let last_left = frame.rect.width - template.width;
    let last_top = frame.rect.height - template.height;
    if last_left < 0 || last_top < 0 {
        return None;
    }
    let matcher = Matcher::new(frame, template, threshold);
    let rows = last_top + 1;
    let positions = rows as usize * (last_left as usize + 1);
    let workers = thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(rows as usize);

    let first = if positions < PARALLEL_FROM_POSITIONS || workers < 2 {
        matcher.first_in_rows(0..rows, last_left)
    } else {
        let chunk = (rows as usize).div_ceil(workers) as i32;
        thread::scope(|scope| {
            let handles: Vec<_> = (0..workers as i32)
                .map(|worker| {
                    let range = (worker * chunk)..((worker + 1) * chunk).min(rows);
                    let matcher = &matcher;
                    scope.spawn(move || matcher.first_in_rows(range, last_left))
                })
                .collect();
            // Chunks are in row order, so the first chunk with a match holds the topmost one.
            handles.into_iter().filter_map(|handle| handle.join().ok().flatten()).next()
        })
    }?;

    let mut best = (first, matcher.score(first.0, first.1)?);
    for top in (first.1 - REFINE_RADIUS).max(0)..=(first.1 + REFINE_RADIUS).min(last_top) {
        for left in (first.0 - REFINE_RADIUS).max(0)..=(first.0 + REFINE_RADIUS).min(last_left) {
            if let Some(score) = matcher.score(left, top) {
                if score < best.1 {
                    best = ((left, top), score);
                }
            }
        }
    }
    Some(best.0)
}

/// Reads a PNG into a data URL for the preview in the editor.
pub fn png_data_url(path: &Path) -> AppResult<String> {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_string();
    let bytes = std::fs::read(path).map_err(|_| AppError::new("templateUnreadable").with("file", name))?;
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic noise so that only one position can match.
    fn noise(width: i32, height: i32, seed: u64) -> Vec<(u8, u8, u8)> {
        let mut rng = fastrand::Rng::with_seed(seed);
        (0..width * height).map(|_| (rng.u8(..), rng.u8(..), rng.u8(..))).collect()
    }

    fn frame_of(width: i32, height: i32, pixels: &[(u8, u8, u8)]) -> Frame {
        let data = pixels.iter().flat_map(|&(r, g, b)| [b, g, r, 255]).collect();
        Frame { rect: Rect { x: 0, y: 0, width, height }, data }
    }

    fn cut(pixels: &[(u8, u8, u8)], frame_width: i32, rect: Rect) -> Template {
        let mut data = Vec::new();
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                let (r, g, b) = pixels[(y * frame_width + x) as usize];
                data.extend_from_slice(&[r, g, b]);
            }
        }
        Template { width: rect.width, height: rect.height, data }
    }

    #[test]
    fn exact_template_is_found_where_it_was_cut() {
        let pixels = noise(120, 90, 7);
        let frame = frame_of(120, 90, &pixels);
        let template = cut(&pixels, 120, Rect { x: 37, y: 41, width: 20, height: 16 });
        assert_eq!(find_template(&frame, &template, 0.9), Some((37, 41)));
        assert_eq!(find_template(&frame, &template, 1.0), Some((37, 41)));
    }

    #[test]
    fn slightly_changed_template_still_matches_but_a_foreign_one_does_not() {
        let pixels = noise(100, 80, 11);
        let frame = frame_of(100, 80, &pixels);
        let mut template = cut(&pixels, 100, Rect { x: 60, y: 10, width: 24, height: 24 });
        for value in template.data.iter_mut() {
            *value = value.saturating_add(6);
        }
        assert_eq!(find_template(&frame, &template, 0.9), Some((60, 10)));
        assert_eq!(find_template(&frame, &template, 1.0), None);

        let foreign_pixels = noise(24, 24, 99);
        let foreign = cut(&foreign_pixels, 24, Rect { x: 0, y: 0, width: 24, height: 24 });
        assert_eq!(find_template(&frame, &foreign, 0.9), None);
    }

    #[test]
    fn template_larger_than_the_area_is_not_found() {
        let pixels = noise(10, 10, 3);
        let frame = frame_of(10, 10, &pixels);
        let big = Template { width: 11, height: 4, data: vec![0; 11 * 4 * 3] };
        assert_eq!(find_template(&frame, &big, 0.5), None);
    }

    #[test]
    fn colours_are_compared_per_channel() {
        assert!(color_within((100, 100, 100), (110, 95, 100), 10));
        assert!(!color_within((100, 100, 100), (111, 100, 100), 10));
        assert_eq!(color_text((255, 0, 16)), "#FF0010");
    }
}
