use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use gpui::{
    AnyWindowHandle, App, Bounds, Entity, HeadlessAppContext, Pixels, Render, Size, Window, point,
    px, size,
};
use image::{
    Rgba, RgbaImage,
    imageops::{self, FilterType},
};

use crate::{
    Theme,
    snapshot::{headless, open, snapshot_dir},
};

pub const FRAME: Duration = Duration::from_nanos(16_666_667);
const SHEET_EDGE: u32 = 1568;
const MARGIN: f32 = 8.0;
const GAP: Rgba<u8> = Rgba([96, 96, 96, 255]);
const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 7, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 1, 1, 1],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
];

pub struct Recording {
    pub frames: Vec<Frame>,
    pub sheet: PathBuf,
    pub summary: PathBuf,
    pub video: Option<PathBuf>,
}

pub struct Frame {
    pub path: PathBuf,
    pub at: Duration,
    pub changed: usize,
    pub bounds: Option<Bounds<Pixels>>,
}

impl Recording {
    pub fn settled_at(&self) -> Duration {
        self.frames
            .iter()
            .rfind(|frame| frame.changed > 0)
            .map_or(Duration::ZERO, |frame| frame.at)
    }
}

pub fn record<V: Render + 'static>(
    name: &str,
    size: Size<Pixels>,
    length: Duration,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    act: impl FnOnce(&mut HeadlessAppContext, AnyWindowHandle),
) -> Recording {
    assert!(
        Path::new(name).file_name() == Some(name.as_ref()),
        "{name:?} must be a single folder name"
    );
    let dir = snapshot_dir().join(name);
    match std::fs::remove_dir_all(&dir) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            panic!("clear {}: {error}", dir.display())
        }
        _ => {}
    }
    std::fs::create_dir_all(&dir).expect("create the recording folder");

    let mut cx = headless();
    let window = open(&mut cx, size, Theme::dark(), build);
    let mut images = vec![cx.capture_screenshot(window).expect("render the window")];
    act(&mut cx, window);
    for _ in 0..length.div_duration_f64(FRAME).ceil() as usize {
        cx.advance_clock(FRAME);
        cx.update_window(window, |_, window, cx| window.simulate_next_frame(cx))
            .expect("deliver the next frame");
        cx.run_until_parked();
        images.push(cx.capture_screenshot(window).expect("render the window"));
    }

    let scale = images[0].width() as f32 / f32::from(size.width);
    let changes: Vec<Option<Change>> = std::iter::once(None)
        .chain(images.windows(2).map(|pair| change(&pair[0], &pair[1])))
        .collect();
    let frames: Vec<Frame> = changes
        .iter()
        .enumerate()
        .map(|(index, change)| Frame {
            path: dir.join(format!("{index:03}.png")),
            at: FRAME * index as u32,
            changed: change.map_or(0, |change| change.pixels),
            bounds: change.map(|change| change.bounds(scale)),
        })
        .collect();
    for (image, frame) in images.iter().zip(&frames) {
        image.save(&frame.path).expect("write a frame");
    }

    let last = changes.iter().rposition(Option::is_some).unwrap_or(0);
    let shown = &images[..=(last + 1).min(images.len() - 1)];
    let crop = changes
        .iter()
        .flatten()
        .copied()
        .reduce(Change::union)
        .map_or(Change::whole(&images[0]), |change| {
            change.padded((MARGIN * scale) as u32, &images[0])
        });
    let (sheet, stride) = contact_sheet(shown, &frames, crop, scale.round().max(1.0) as u32);
    let sheet_path = dir.join("sheet.png");
    sheet.save(&sheet_path).expect("write the contact sheet");

    let summary = summarize(name, &frames, stride, crop, scale);
    let summary_path = dir.join("frames.txt");
    std::fs::write(&summary_path, summary).expect("write the frame summary");

    Recording {
        frames,
        sheet: sheet_path,
        summary: summary_path,
        video: encode(&dir),
    }
}

fn encode(dir: &Path) -> Option<PathBuf> {
    let video = dir.join("video.mp4");
    let output = Command::new("ffmpeg")
        .current_dir(dir)
        .args(["-loglevel", "error", "-framerate"])
        .arg(format!("{}", 1.0 / FRAME.as_secs_f64()))
        .args(["-i", "%03d.png", "-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2"])
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "12"])
        .arg(&video)
        .output();
    match output {
        Ok(output) if output.status.success() => Some(video),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Ok(output) => {
            let error = String::from_utf8_lossy(&output.stderr);
            eprintln!("skipped {}: {}", video.display(), error.trim());
            None
        }
        Err(error) => {
            eprintln!("skipped {}: {error}", video.display());
            None
        }
    }
}

#[derive(Clone, Copy)]
struct Change {
    pixels: usize,
    min: (u32, u32),
    max: (u32, u32),
}

impl Change {
    fn whole(image: &RgbaImage) -> Self {
        Self {
            pixels: 0,
            min: (0, 0),
            max: (image.width(), image.height()),
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            pixels: self.pixels + other.pixels,
            min: (self.min.0.min(other.min.0), self.min.1.min(other.min.1)),
            max: (self.max.0.max(other.max.0), self.max.1.max(other.max.1)),
        }
    }

    fn padded(self, margin: u32, image: &RgbaImage) -> Self {
        Self {
            min: (
                self.min.0.saturating_sub(margin),
                self.min.1.saturating_sub(margin),
            ),
            max: (
                (self.max.0 + margin).min(image.width()),
                (self.max.1 + margin).min(image.height()),
            ),
            ..self
        }
    }

    fn width(&self) -> u32 {
        self.max.0 - self.min.0
    }

    fn height(&self) -> u32 {
        self.max.1 - self.min.1
    }

    fn bounds(&self, scale: f32) -> Bounds<Pixels> {
        Bounds::new(
            point(px(self.min.0 as f32 / scale), px(self.min.1 as f32 / scale)),
            size(
                px(self.width() as f32 / scale),
                px(self.height() as f32 / scale),
            ),
        )
    }
}

fn change(before: &RgbaImage, after: &RgbaImage) -> Option<Change> {
    before
        .enumerate_pixels()
        .zip(after.pixels())
        .filter(|((_, _, a), b)| a != b)
        .map(|((x, y, _), _)| Change {
            pixels: 1,
            min: (x, y),
            max: (x + 1, y + 1),
        })
        .reduce(Change::union)
}

fn contact_sheet(
    images: &[RgbaImage],
    frames: &[Frame],
    crop: Change,
    scale: u32,
) -> (RgbaImage, usize) {
    let dot = 2 * scale;
    let label = 7 * dot;
    let gap = 2 * scale;
    let room = (SHEET_EDGE - 2 * gap) / 3;
    let shrink = crop
        .width()
        .div_ceil(room)
        .max(crop.height().div_ceil(room - label))
        .max(1);
    let shot = (crop.width() / shrink, crop.height() / shrink);
    let tile = (shot.0, label + shot.1);
    let fit = |edge: u32, length: u32| ((SHEET_EDGE + gap) / (length + gap)).clamp(1, edge);
    let columns = fit(images.len() as u32, tile.0);
    let rows = fit(images.len() as u32, tile.1);
    let stride = images.len().div_ceil((columns * rows) as usize);
    let mut picked: Vec<usize> = (0..images.len()).step_by(stride).collect();
    if let Some(last) = picked.last_mut() {
        *last = images.len() - 1;
    }
    let columns = columns.min(picked.len() as u32);
    let rows = (picked.len() as u32).div_ceil(columns);
    let mut sheet = RgbaImage::from_pixel(
        columns * (tile.0 + gap) - gap,
        rows * (tile.1 + gap) - gap,
        GAP,
    );
    for (slot, &index) in picked.iter().enumerate() {
        let x = (slot as u32 % columns) * (tile.0 + gap);
        let y = (slot as u32 / columns) * (tile.1 + gap);
        for dy in 0..label {
            for dx in 0..tile.0 {
                sheet.put_pixel(x + dx, y + dy, Rgba([0, 0, 0, 255]));
            }
        }
        let ms = format!("{:.0}", frames[index].at.as_secs_f64() * 1000.0);
        for (n, digit) in ms.bytes().enumerate() {
            let rows = DIGITS[usize::from(digit - b'0')];
            for (row, bits) in rows.iter().enumerate() {
                for column in 0..3 {
                    if bits & (4 >> column) == 0 {
                        continue;
                    }
                    let left = x + dot + n as u32 * 4 * dot + column * dot;
                    let top = y + dot + row as u32 * dot;
                    for py in top..top + dot {
                        for px in left..(left + dot).min(x + tile.0) {
                            sheet.put_pixel(px, py, Rgba([255, 255, 255, 255]));
                        }
                    }
                }
            }
        }
        let cropped = imageops::crop_imm(
            &images[index],
            crop.min.0,
            crop.min.1,
            crop.width(),
            crop.height(),
        )
        .to_image();
        let shot = if shrink > 1 {
            imageops::resize(&cropped, shot.0, shot.1, FilterType::Triangle)
        } else {
            cropped
        };
        imageops::replace(&mut sheet, &shot, i64::from(x), i64::from(y + label));
    }
    (sheet, stride)
}

fn summarize(name: &str, frames: &[Frame], stride: usize, crop: Change, scale: f32) -> String {
    let crop = crop.bounds(scale);
    let mut out = format!(
        "{name}: {} frames at {:.1} ms, bounds in points as x,y wxh\n\
         sheet.png crops to {} and shows every {} frame(s) and the last, labelled in ms\n\
         frame 0 is the window before the action, frame N renders N frames after it\n\n",
        frames.len(),
        FRAME.as_secs_f64() * 1000.0,
        describe(crop),
        stride,
    );
    for (index, frame) in frames.iter().enumerate() {
        let _ = write!(
            out,
            "{index:>4} {:>6.0} ms ",
            frame.at.as_secs_f64() * 1000.0
        );
        let _ = match frame.bounds {
            None if index == 0 => writeln!(out, "before"),
            None => writeln!(out, "still"),
            Some(bounds) => writeln!(
                out,
                "{:>7} px changed in {}",
                frame.changed,
                describe(bounds)
            ),
        };
    }
    out
}

fn describe(bounds: Bounds<Pixels>) -> String {
    format!(
        "{},{} {}x{}",
        f32::from(bounds.origin.x),
        f32::from(bounds.origin.y),
        f32::from(bounds.size.width),
        f32::from(bounds.size.height)
    )
}
