//! Batch image conversion, one file per CPU core at a time.

use crate::decode::{self, Item};
use image::{imageops::FilterType, DynamicImage, ImageFormat};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutFormat {
    Jpeg,
    Png,
    Webp,
    Bmp,
    Tiff,
}

impl OutFormat {
    pub const ALL: [OutFormat; 5] = [Self::Jpeg, Self::Png, Self::Webp, Self::Bmp, Self::Tiff];
    pub fn label(self) -> &'static str {
        match self {
            Self::Jpeg => "JPG",
            Self::Png => "PNG",
            Self::Webp => "WebP (무손실)",
            Self::Bmp => "BMP",
            Self::Tiff => "TIFF",
        }
    }
    fn ext(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Bmp => "bmp",
            Self::Tiff => "tif",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resize {
    None,
    KeepRatio,
    FitWidth,
    FitHeight,
}

impl Resize {
    pub const ALL: [Resize; 4] = [Self::None, Self::KeepRatio, Self::FitWidth, Self::FitHeight];
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "크기 변경 안 함",
            Self::KeepRatio => "원본 비율 유지 (가로×세로 안에)",
            Self::FitWidth => "폭 맞춤",
            Self::FitHeight => "높이 맞춤",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub format: OutFormat,
    pub quality: u8,
    pub resize: Resize,
    pub width: u32,
    pub height: u32,
    pub out_dir: PathBuf,
}

#[derive(Default)]
pub struct Progress {
    pub total: AtomicUsize,
    pub done: AtomicUsize,
    pub failed: AtomicUsize,
    pub cancel: AtomicBool,
    pub finished: AtomicBool,
    pub seconds: Mutex<f32>,
    pub errors: Mutex<Vec<String>>,
}

pub fn start(items: Vec<Item>, s: Settings) -> Arc<Progress> {
    let p = Arc::new(Progress::default());
    p.total.store(items.len(), Ordering::Relaxed);
    let p2 = p.clone();
    std::thread::spawn(move || {
        let t0 = Instant::now();
        let _ = std::fs::create_dir_all(&s.out_dir);
        items.par_iter().for_each(|it| {
            if p2.cancel.load(Ordering::Relaxed) {
                return;
            }
            match convert_one(it, &s) {
                Ok(()) => p2.done.fetch_add(1, Ordering::Relaxed),
                Err(e) => {
                    p2.errors.lock().unwrap().push(format!("{}: {e}", it.name()));
                    p2.failed.fetch_add(1, Ordering::Relaxed)
                }
            };
        });
        *p2.seconds.lock().unwrap() = t0.elapsed().as_secs_f32();
        p2.finished.store(true, Ordering::Relaxed);
    });
    p
}

pub fn target_size(w: u32, h: u32, s: &Settings) -> (u32, u32) {
    let (tw, th) = (s.width.max(1), s.height.max(1));
    let r = |x: f64| (x.round() as u32).max(1);
    match s.resize {
        Resize::None => (w, h),
        Resize::KeepRatio => {
            let k = (tw as f64 / w as f64).min(th as f64 / h as f64);
            (r(w as f64 * k), r(h as f64 * k))
        }
        Resize::FitWidth => (tw, r(h as f64 * tw as f64 / w as f64)),
        Resize::FitHeight => (r(w as f64 * th as f64 / h as f64), th),
    }
}

fn convert_one(it: &Item, s: &Settings) -> Result<(), String> {
    let d = decode::decode(it)?;
    let (w, h) = target_size(d.img.width(), d.img.height(), s);
    let mut img = DynamicImage::ImageRgba8(d.img);
    if (w, h) != (img.width(), img.height()) {
        img = img.resize_exact(w, h, FilterType::Triangle);
    }
    let stem = Path::new(&it.name()).file_stem().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
    let out = free_name(&s.out_dir, &stem, s.format.ext());
    let mut f = std::io::BufWriter::new(std::fs::File::create(&out).map_err(|e| e.to_string())?);
    match s.format {
        OutFormat::Jpeg => {
            let rgb = img.to_rgb8(); // JPEG has no alpha
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut f, s.quality.clamp(1, 100))
                .encode_image(&rgb)
                .map_err(|e| e.to_string())
        }
        OutFormat::Png => img.write_to(&mut f, ImageFormat::Png).map_err(|e| e.to_string()),
        OutFormat::Webp => img.write_to(&mut f, ImageFormat::WebP).map_err(|e| e.to_string()),
        OutFormat::Bmp => img.write_to(&mut f, ImageFormat::Bmp).map_err(|e| e.to_string()),
        OutFormat::Tiff => img.write_to(&mut f, ImageFormat::Tiff).map_err(|e| e.to_string()),
    }
}

/// "name.jpg", or "name (2).jpg" when taken (CHK_OVERWRITE_RENAME behaviour).
fn free_name(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..).map(|n| dir.join(format!("{stem} ({n}).{ext}"))).find(|p| !p.exists()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sizes() {
        let mut s = Settings {
            format: OutFormat::Png,
            quality: 90,
            resize: Resize::KeepRatio,
            width: 100,
            height: 100,
            out_dir: PathBuf::new(),
        };
        assert_eq!(target_size(400, 200, &s), (100, 50));
        s.resize = Resize::FitWidth;
        assert_eq!(target_size(400, 200, &s), (100, 50));
        s.resize = Resize::FitHeight;
        assert_eq!(target_size(400, 200, &s), (200, 100));
    }

    #[test]
    fn batch_converts_in_parallel() {
        let dir = std::env::temp_dir().join(format!("bv_conv_test_{}", std::process::id()));
        let out = dir.join("out");
        std::fs::create_dir_all(&dir).unwrap();
        let items: Vec<Item> = (0..8)
            .map(|i| {
                let p = dir.join(format!("{i}.png"));
                image::RgbaImage::from_pixel(64, 32, image::Rgba([i * 30, 100, 200, 255])).save(&p).unwrap();
                Item::File(p)
            })
            .collect();
        let s = Settings { format: OutFormat::Jpeg, quality: 80, resize: Resize::FitWidth, width: 16, height: 0, out_dir: out.clone() };
        let p = start(items, s);
        while !p.finished.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(p.done.load(Ordering::Relaxed), 8);
        assert_eq!(image::image_dimensions(out.join("3.jpg")).unwrap(), (16, 8));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
