//! TailView: an ad-free, cross-platform (Windows/macOS) image and comic viewer.
//! Rules and numbers come from MECHANICS.md.

mod convert;
mod decode;

use decode::{Item, SortBy};
use eframe::egui::{self, Color32, Key, Modifiers, Pos2, Rect, Vec2};
use image::RgbaImage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

// ---- numbers from MECHANICS.md -------------------------------------------------------------
const ZOOM_100: i64 = 10000; // FUN_1400f9950
const ZOOM_MIN: i64 = 500; // FUN_14013bcc0
const ZOOM_MIN_FINE: i64 = 100;
const ZOOM_MAX: i64 = 2_000_000;
const ZOOM_STEP: i64 = 1000; // 10 %
const ZOOM_STEP_FINE: i64 = 100; // 1 %
const WEBTOON_PRESETS: [u32; 3] = [690, 760, 800]; // data 0x1405ff4c0
const WEBTOON_SPACE: f32 = 10.0; // webtoonSpace, FUN_14007a090
const LONG_SCROLL: f32 = 0.9; // estimated
const ARROW_SCROLL: f32 = 0.15; // estimated
const SLIDESHOW_TICK_MS: u64 = 50; // FUN_14010df90
const GAMMA_EXP: f32 = 0.45; // viewMode.gamma1000 = 450, FUN_1400796e0
const WEBTOON_DECODE_WIDTH: u32 = 1600; // memory cap per strip, estimated
const TILE: u32 = 4096;
const BG: Color32 = Color32::from_rgb(0x20, 0x20, 0x20); // clrBkgnd 0x202020, FUN_14007a090
const BAR: Color32 = Color32::from_rgb(36, 37, 40);
const ACCENT: Color32 = Color32::from_rgb(0xF4, 0x7A, 0x32);
const ICON: &[u8] = include_bytes!("../assets/tailview.png");

/// One zoom command. `z` and `step` are in 1/100 % units (10000 = 100 %).
/// Same steps as FUN_14013bcc0: 10 % up to 300 %, 20 % up to 500 %, then 50 %.
fn zoom_step(z: i64, step: i64) -> i64 {
    if step.abs() == ZOOM_STEP_FINE {
        return (z + step).clamp(ZOOM_MIN_FINE, ZOOM_MAX);
    }
    if step > 0 && z < step {
        return step.max(ZOOM_MIN); // below 10 %: jump to 10 % (FUN_1400f9950 case 2)
    }
    let s = step.abs();
    let n = if step > 0 {
        let inc = if z < 30000 { 1000 } else if z < 50000 { 2000 } else { 5000 };
        ((z + inc) as f64 / s as f64).round() as i64 * s
    } else {
        let dec = if z <= 30000 { 1000 } else if z <= 50000 { 2000 } else { 5000 };
        ((z - dec) as f64 / s as f64).round() as i64 * s
    };
    n.clamp(ZOOM_MIN, ZOOM_MAX)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ViewMode {
    Original,
    FitWindow,
    FitWidth,
    FitHeight,
    TwoL2R,
    TwoR2L,
    Webtoon,
}

impl ViewMode {
    const ALL: [ViewMode; 7] = [
        Self::Original,
        Self::FitWindow,
        Self::FitWidth,
        Self::FitHeight,
        Self::TwoL2R,
        Self::TwoR2L,
        Self::Webtoon,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Original => "원본 크기(100%)로 보기   0",
            Self::FitWindow => "꽉차게 보기   9",
            Self::FitWidth => "꽉차게 보기(폭맞춤)   8",
            Self::FitHeight => "꽉차게 보기(높이 맞춤)",
            Self::TwoL2R => "스마트 두장 보기(왼쪽→오른쪽)   7",
            Self::TwoR2L => "스마트 두장 보기(왼쪽←오른쪽)   6",
            Self::Webtoon => "웹툰 보기   5",
        }
    }
    fn short(self) -> &'static str {
        match self {
            Self::Original => "원본",
            Self::FitWindow => "꽉차게",
            Self::FitWidth => "폭맞춤",
            Self::FitHeight => "높이맞춤",
            Self::TwoL2R => "두장 →",
            Self::TwoR2L => "두장 ←",
            Self::Webtoon => "웹툰",
        }
    }
    fn toast(self) -> &'static str {
        match self {
            Self::Original => "보기 모드: 원본 크기로 보기",
            Self::FitWindow => "보기 모드: 꽉차게 보기",
            Self::FitWidth => "보기 모드: 꽉차게 보기(폭맞춤)",
            Self::FitHeight => "보기 모드: 꽉차게 보기(높이 맞춤)",
            Self::TwoL2R => "보기 모드: 스마트 두장 보기(왼쪽⇨오른쪽)",
            Self::TwoR2L => "보기 모드: 스마트 두장 보기(왼쪽⇦오른쪽)",
            Self::Webtoon => "보기 모드: 웹툰 모드",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Default, Debug)]
struct Effects {
    rotate: u8, // quarter turns clockwise
    flip: bool,
    mirror: bool,
    gray: bool,
    sepia: bool,
    negative: bool,
    gamma: bool,
    brightness: i32,
    contrast: i32,
    saturation: i32,
}

impl Effects {
    fn any_pixel(&self) -> bool {
        self.gray || self.sepia || self.negative || self.gamma || self.brightness != 0 || self.contrast != 0 || self.saturation != 0
    }
}

fn apply_effects(mut img: RgbaImage, fx: &Effects) -> RgbaImage {
    use image::imageops;
    img = match fx.rotate % 4 {
        1 => imageops::rotate90(&img),
        2 => imageops::rotate180(&img),
        3 => imageops::rotate270(&img),
        _ => img,
    };
    if fx.flip {
        imageops::flip_vertical_in_place(&mut img);
    }
    if fx.mirror {
        imageops::flip_horizontal_in_place(&mut img);
    }
    if !fx.any_pixel() {
        return img;
    }
    use rayon::prelude::*;
    let (b, c, s) = (fx.brightness as f32 * 2.55, 1.0 + fx.contrast as f32 / 100.0, 1.0 + fx.saturation as f32 / 100.0);
    let fx = *fx;
    let buf: &mut [u8] = &mut img;
    buf.par_chunks_mut(4).for_each(|p| {
        let mut v = [p[0] as f32, p[1] as f32, p[2] as f32];
        for x in &mut v {
            *x = (*x - 128.0) * c + 128.0 + b;
        }
        let l = 0.299 * v[0] + 0.587 * v[1] + 0.114 * v[2];
        for x in &mut v {
            *x = l + (*x - l) * s;
        }
        if fx.gray {
            v = [l, l, l];
        }
        if fx.sepia {
            let [r, g, bb] = v;
            v = [
                0.393 * r + 0.769 * g + 0.189 * bb,
                0.349 * r + 0.686 * g + 0.168 * bb,
                0.272 * r + 0.534 * g + 0.131 * bb,
            ];
        }
        for (i, x) in v.iter().enumerate() {
            let mut y = x.clamp(0.0, 255.0);
            if fx.gamma {
                y = 255.0 * (y / 255.0).powf(GAMMA_EXP);
            }
            if fx.negative {
                y = 255.0 - y;
            }
            p[i] = y as u8;
        }
    });
    img
}

/// A decoded page split into GPU textures (any size fits).
struct Pic {
    size: [u32; 2],
    orig: [u32; 2],
    tiles: Vec<(egui::TextureHandle, Rect)>, // rect in 0..1 of the whole picture
    format: String,
    ultra_hdr: bool,
}

enum Slot {
    Loading,
    Ready(Pic),
    Failed(String),
}

type LoadResult = Result<(RgbaImage, [u32; 2], String, bool), String>;

struct Loaded {
    gen: u64,
    idx: usize,
    result: LoadResult,
}

struct ConvertUi {
    settings: convert::Settings,
    items: Vec<Item>,
    progress: Option<Arc<convert::Progress>>,
}

struct App {
    items: Vec<Item>,
    cur: usize,
    container: Option<PathBuf>,
    sort: SortBy,
    descending: bool,
    mode: ViewMode,
    stretch_small: bool,
    smooth: bool,
    zoom: Option<i64>,
    pan: Vec2,
    pan_range: (Vec2, Vec2),
    last_img_rect: Rect,
    last_scale: f32,
    wheel_acc: f32,
    fx: Effects,
    pending_fx: Option<Effects>,
    cache: HashMap<usize, Slot>,
    gen: u64,
    tx: Sender<Loaded>,
    rx: Receiver<Loaded>,
    dims: Vec<Option<[u32; 2]>>,
    dims_job: Option<(Arc<AtomicUsize>, Receiver<Vec<Option<[u32; 2]>>>)>,
    webtoon_apply_width: bool,
    webtoon_width: u32,
    webtoon_spacing: bool,
    webtoon_scroll: f32,
    webtoon_jump: Option<usize>,
    slideshow: Option<(u64, Instant)>,
    slide_loop: bool,
    slide_random: bool,
    slide_timer: bool,
    show_info: bool,
    show_effects: bool,
    show_webtoon_settings: bool,
    show_help: bool,
    toast: Option<(String, Instant)>,
    convert: Option<ConvertUi>,
    fullscreen: bool,
    title: String,
    logo: Option<egui::TextureHandle>,
    rng: u64,
    screenshot: Option<PathBuf>,
    frame: u32,
}

fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    let mut screenshot = None;
    let mut open = None;
    let mut webtoon = false;
    while let Some(a) = args.next() {
        if a == "--webtoon" {
            webtoon = true;
        } else if a == "--screenshot" {
            screenshot = args.next().map(PathBuf::from);
        } else {
            open = Some(PathBuf::from(a));
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TailView")
            .with_icon(eframe::icon_data::from_png_bytes(ICON).expect("valid TailView icon"))
            .with_inner_size([1000.0, 800.0]) // FUN_14007a090: 1000x800 at 96 dpi
            .with_min_inner_size([480.0, 320.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "TailView",
        options,
        Box::new(move |cc| {
            setup_style(&cc.egui_ctx);
            let mut app = App::new(screenshot);
            if webtoon {
                app.mode = ViewMode::Webtoon;
            }
            if let Some(p) = open {
                app.open_path(&p);
            }
            Ok(Box::new(app))
        }),
    )
}

fn setup_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidates = [
        "C:\\Windows\\Fonts\\malgun.ttf",
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
        "/Library/Fonts/AppleGothic.ttf",
        "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
    ];
    if let Some(data) = candidates.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert("korean".into(), Arc::new(egui::FontData::from_owned(data)));
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts.families.entry(fam).or_default().push("korean".into());
        }
    }
    ctx.set_fonts(fonts);
    // dark always, whatever the OS theme
    ctx.options_mut(|o| o.theme_preference = egui::ThemePreference::Dark);
    let mut visuals = egui::Visuals::dark();
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke.color = Color32::BLACK;
    visuals.hyperlink_color = ACCENT;
    visuals.widgets.hovered.bg_stroke.color = ACCENT;
    visuals.widgets.active.bg_fill = ACCENT;
    visuals.widgets.active.fg_stroke.color = Color32::BLACK;
    ctx.set_visuals(visuals);
}

impl App {
    fn new(screenshot: Option<PathBuf>) -> Self {
        let (tx, rx) = channel();
        Self {
            items: vec![],
            cur: 0,
            container: None,
            sort: SortBy::Name,
            descending: false,
            mode: ViewMode::FitWindow,
            stretch_small: false,
            smooth: true,
            zoom: None,
            pan: Vec2::ZERO,
            pan_range: (Vec2::ZERO, Vec2::ZERO),
            last_img_rect: Rect::NOTHING,
            last_scale: 1.0,
            wheel_acc: 0.0,
            fx: Effects::default(),
            pending_fx: None,
            cache: HashMap::new(),
            gen: 0,
            tx,
            rx,
            dims: vec![],
            dims_job: None,
            webtoon_apply_width: false, // FUN_1400796e0
            webtoon_width: WEBTOON_PRESETS[0],
            webtoon_spacing: false,
            webtoon_scroll: 0.0,
            webtoon_jump: None,
            slideshow: None,
            slide_loop: true,
            slide_random: false,
            slide_timer: false, // FUN_14007a090
            show_info: false,
            show_effects: false,
            show_webtoon_settings: false,
            show_help: false,
            toast: None,
            convert: None,
            fullscreen: false,
            title: String::new(),
            logo: None,
            rng: 0x2545F4914F6CDD1D,
            screenshot,
            frame: 0,
        }
    }

    fn toast(&mut self, s: impl Into<String>) {
        self.toast = Some((s.into(), Instant::now()));
    }

    // ---- opening ----------------------------------------------------------------------------

    fn open_path(&mut self, p: &Path) {
        let (container, select) = if p.is_dir() || decode::is_archive(p) {
            (p.to_path_buf(), None)
        } else {
            (p.parent().map(Path::to_path_buf).unwrap_or_default(), p.file_name().map(|n| n.to_string_lossy().into_owned()))
        };
        let mut items = decode::list_container(&container);
        if items.is_empty() && select.is_some() {
            items = vec![Item::File(p.to_path_buf())]; // unknown extension: try anyway
        }
        decode::sort_items(&mut items, self.sort, self.descending);
        if items.is_empty() {
            self.toast(if decode::is_archive(p) { "압축 파일에 이미지가 없습니다." } else { "폴더에 파일이 없습니다." });
            return;
        }
        self.cur = select.and_then(|n| items.iter().position(|i| i.name() == n)).unwrap_or(0);
        self.items = items;
        self.container = Some(container);
        self.after_list_change();
        self.webtoon_jump = Some(self.cur);
        self.zoom = None;
        self.pan = Vec2::ZERO;
    }

    fn after_list_change(&mut self) {
        self.reset_cache();
        self.dims = vec![None; self.items.len()];
        self.dims_job = None;
    }

    fn reset_cache(&mut self) {
        self.gen += 1;
        self.cache.clear();
    }

    fn resort(&mut self) {
        let name = self.items.get(self.cur).map(Item::name);
        decode::sort_items(&mut self.items, self.sort, self.descending);
        self.cur = name.and_then(|n| self.items.iter().position(|i| i.name() == n)).unwrap_or(0);
        self.after_list_change();
        self.webtoon_jump = Some(self.cur);
        let label = match self.sort {
            SortBy::Name => "파일명",
            SortBy::Modified => "수정한 날짜",
            SortBy::Size => "크기",
            SortBy::Ext => "확장자",
            SortBy::Created => "만든 날짜",
        };
        self.toast(format!("정렬 방식: {label}{}", if self.descending { " (역순)" } else { "" }));
    }

    fn pick_file(&mut self) {
        let exts = [
            "jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "tga", "ico", "hdr", "exr", "qoi", "dds", "psd", "psb",
            "clip", "heic", "heif", "hif", "avif", "jxl", "dng", "cr2", "cr3", "nef", "arw", "orf", "zip", "cbz",
        ];
        if let Some(p) = rfd::FileDialog::new().add_filter("이미지 파일", &exts).add_filter("모든 파일", &["*"]).pick_file() {
            self.open_path(&p);
        }
    }

    fn pick_folder(&mut self) {
        if let Some(p) = rfd::FileDialog::new().pick_folder() {
            self.open_path(&p);
        }
    }

    fn move_container(&mut self, dir: i32) {
        let Some(c) = self.container.clone() else {
            self.toast("폴더 이동: 이동할 수 없습니다 (파일을 열어주세요)");
            return;
        };
        let sib = decode::sibling_containers(&c);
        let Some(pos) = sib.iter().position(|p| p == &c) else { return };
        let mut i = pos as i32 + dir;
        while i >= 0 && (i as usize) < sib.len() {
            let p = sib[i as usize].clone();
            if !decode::list_container(&p).is_empty() {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.open_path(&p);
                self.toast(format!("{}: {name}", if dir > 0 { "다음 폴더" } else { "이전 폴더" }));
                return;
            }
            i += dir;
        }
        self.toast(if dir > 0 { "다음 폴더: 이동할 수 없습니다" } else { "이전 폴더: 이동할 수 없습니다" });
    }

    // ---- navigation -------------------------------------------------------------------------

    fn two_page(&self) -> bool {
        matches!(self.mode, ViewMode::TwoL2R | ViewMode::TwoR2L)
    }

    fn page_step(&self) -> usize {
        if self.two_page() && self.pair_at(self.cur) {
            2
        } else {
            1
        }
    }

    fn go(&mut self, to: usize) {
        if self.items.is_empty() {
            return;
        }
        let to = to.min(self.items.len() - 1);
        if to == self.cur {
            return;
        }
        self.cur = to;
        self.zoom = None;
        self.pan = Vec2::ZERO;
        if self.mode == ViewMode::Webtoon {
            self.webtoon_jump = Some(to);
        }
    }

    fn next(&mut self, n: usize) {
        if self.items.is_empty() {
            return;
        }
        if self.cur + n >= self.items.len() {
            self.toast("마지막 이미지입니다.");
            return;
        }
        self.go(self.cur + n);
    }

    fn prev(&mut self, n: usize) {
        if self.cur == 0 {
            if !self.items.is_empty() {
                self.toast("첫번째 이미지입니다.");
            }
            return;
        }
        let n = if n == 1 && self.two_page() && self.cur >= 2 && self.pair_at(self.cur - 2) { 2 } else { n };
        self.go(self.cur.saturating_sub(n));
    }

    fn random_page(&mut self) {
        if self.items.len() < 2 {
            return;
        }
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        let mut to = (self.rng % self.items.len() as u64) as usize;
        if to == self.cur {
            to = (to + 1) % self.items.len();
        }
        self.go(to);
    }

    // ---- loading ----------------------------------------------------------------------------

    fn request(&mut self, ctx: &egui::Context, idx: usize) {
        if idx >= self.items.len() || self.cache.contains_key(&idx) {
            return;
        }
        self.cache.insert(idx, Slot::Loading);
        let (item, fx, gen, tx, ctx) = (self.items[idx].clone(), self.fx, self.gen, self.tx.clone(), ctx.clone());
        let cap_w = if self.mode == ViewMode::Webtoon { WEBTOON_DECODE_WIDTH } else { 16384 };
        rayon::spawn(move || {
            let result = decode::decode(&item).map(|d| {
                let orig = [d.img.width(), d.img.height()];
                let mut img = d.img;
                let big = img.width().max(img.height());
                if img.width() > cap_w || big > 16384 {
                    let k = (cap_w as f32 / img.width() as f32).min(16384.0 / big as f32);
                    let (w, h) = (((img.width() as f32 * k) as u32).max(1), ((img.height() as f32 * k) as u32).max(1));
                    img = image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle);
                }
                (apply_effects(img, &fx), orig, d.format, d.ultra_hdr)
            });
            let _ = tx.send(Loaded { gen, idx, result });
            ctx.request_repaint();
        });
    }

    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(l) = self.rx.try_recv() {
            if l.gen != self.gen {
                continue;
            }
            let slot = match l.result {
                Ok((img, orig, format, ultra_hdr)) => {
                    let opts = if self.smooth {
                        egui::TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..egui::TextureOptions::LINEAR }
                    } else {
                        egui::TextureOptions::NEAREST
                    };
                    if let Some(d) = self.dims.get_mut(l.idx) {
                        *d = Some(orig);
                    }
                    Slot::Ready(make_pic(ctx, &img, orig, format, ultra_hdr, opts))
                }
                Err(e) => Slot::Failed(e),
            };
            self.cache.insert(l.idx, slot);
        }
        if let Some((_, rx)) = &self.dims_job {
            if let Ok(d) = rx.try_recv() {
                for (slot, v) in self.dims.iter_mut().zip(d) {
                    if slot.is_none() {
                        *slot = v;
                    }
                }
                self.dims_job = None;
            }
        }
    }

    fn evict(&mut self, keep: std::ops::RangeInclusive<usize>) {
        self.cache.retain(|k, _| keep.contains(k));
    }

    fn start_dims_job(&mut self, ctx: &egui::Context) {
        if self.dims_job.is_some() || self.dims.iter().all(Option::is_some) {
            return;
        }
        let items = self.items.clone();
        let counter = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = channel();
        let (c2, ctx) = (counter.clone(), ctx.clone());
        std::thread::spawn(move || {
            use rayon::prelude::*;
            let v: Vec<_> = items
                .par_iter()
                .map(|i| {
                    let d = decode::dimensions(i);
                    c2.fetch_add(1, Ordering::Relaxed);
                    d
                })
                .collect();
            let _ = tx.send(v);
            ctx.request_repaint();
        });
        self.dims_job = Some((counter, rx));
    }

    /// Size of page `i` after rotation, if known.
    fn page_size(&self, i: usize) -> Option<Vec2> {
        if let Some(Slot::Ready(p)) = self.cache.get(&i) {
            return Some(Vec2::new(p.size[0] as f32, p.size[1] as f32));
        }
        let [w, h] = self.dims.get(i).copied().flatten()?;
        Some(if self.fx.rotate % 2 == 1 { Vec2::new(h as f32, w as f32) } else { Vec2::new(w as f32, h as f32) })
    }

    /// Smart double page: show `i` and `i+1` together unless either is landscape.
    fn pair_at(&self, i: usize) -> bool {
        if i + 1 >= self.items.len() {
            return false;
        }
        let portrait = |j| self.page_size(j).map(|s| s.x <= s.y).unwrap_or(true);
        portrait(i) && portrait(i + 1)
    }

    // ---- zoom -------------------------------------------------------------------------------

    fn base_scale(&self, img: Vec2, avail: Vec2, ppp: f32) -> f32 {
        let one = 1.0 / ppp;
        let s = match self.mode {
            ViewMode::Original => return one,
            ViewMode::FitWidth => avail.x / img.x,
            ViewMode::FitHeight => avail.y / img.y,
            _ => (avail.x / img.x).min(avail.y / img.y),
        };
        if self.stretch_small {
            s
        } else {
            s.min(one)
        }
    }

    /// Zoom from the current shown scale (a fit mode becomes the starting zoom, FUN_14013bcc0).
    fn do_zoom(&mut self, step: i64, current_scale: f32, ppp: f32, anchor: Option<Pos2>) {
        let z0 = match self.zoom {
            Some(z) => z,
            None if self.mode == ViewMode::Webtoon => ZOOM_100,
            None => (current_scale * ppp * ZOOM_100 as f32).round() as i64,
        };
        self.set_zoom(zoom_step(z0, step), current_scale, ppp, anchor);
    }

    fn set_zoom(&mut self, z1: i64, current_scale: f32, ppp: f32, anchor: Option<Pos2>) {
        if self.mode == ViewMode::Webtoon {
            let k = z1 as f32 / self.zoom.unwrap_or(ZOOM_100) as f32;
            self.webtoon_scroll *= k;
        } else {
            // Keep the picture point under the cursor (or the centre) still (FUN_14013c100).
            let s1 = z1 as f32 / ZOOM_100 as f32 / ppp;
            let k = s1 / current_scale.max(1e-6);
            let r = self.last_img_rect;
            if r.is_positive() {
                let c = anchor.unwrap_or(r.center());
                let new_min = c - (c - r.min) * k;
                // pan is the picture's top-left relative to the view
                let view_min = r.min - self.pan;
                self.pan = new_min - view_min;
            }
        }
        self.zoom = Some(z1);
        self.toast(format!("{}%", z1 / 100));
    }

    // ---- webtoon ----------------------------------------------------------------------------

    /// Width of page `i` in the webtoon column: the set webtoon width, or (when off) the
    /// picture's own width shrunk to fit the window (estimated). Zoom scales both.
    fn webtoon_page_width(&self, i: usize, avail_w: f32, ppp: f32) -> f32 {
        let base = if self.webtoon_apply_width {
            self.webtoon_width as f32 / ppp
        } else {
            self.dims.get(i).copied().flatten().map(|d| d[0] as f32 / ppp).unwrap_or(avail_w).min(avail_w)
        };
        base * self.zoom.unwrap_or(ZOOM_100) as f32 / ZOOM_100 as f32
    }

    /// Top and width of every page, and the total height.
    fn webtoon_layout(&self, avail_w: f32, ppp: f32) -> (Vec<f32>, Vec<f32>, f32) {
        let gap = if self.webtoon_spacing { WEBTOON_SPACE } else { 0.0 };
        let mut y = 0.0;
        let (mut tops, mut widths) = (vec![], vec![]);
        for i in 0..self.items.len() {
            let w = self.webtoon_page_width(i, avail_w, ppp);
            let s = self.page_size(i).unwrap_or(Vec2::new(1.0, 1.4));
            tops.push(y);
            widths.push(w);
            y += w * s.y / s.x + gap;
        }
        (tops, widths, y)
    }

    // ---- input ------------------------------------------------------------------------------

    fn handle_keys(&mut self, ctx: &egui::Context, view: Vec2, ppp: f32, scale: f32) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let k = |m: Modifiers, key: Key| ctx.input_mut(|i| i.consume_key(m, key));
        let none = Modifiers::NONE;
        let cmd = Modifiers::COMMAND;
        let alt = Modifiers::ALT;
        let shift = Modifiers::SHIFT;
        let webtoon = self.mode == ViewMode::Webtoon;

        if k(cmd, Key::O) {
            self.pick_file();
        }
        if k(cmd, Key::T) {
            self.open_convert();
        }
        if k(none, Key::F) {
            self.pick_folder();
        }
        if k(none, Key::Tab) {
            self.show_info = !self.show_info;
        }
        if k(none, Key::F1) {
            self.show_help = !self.show_help;
        }
        if k(none, Key::X) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if k(none, Key::F11) || k(alt, Key::Enter) {
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
        }
        if k(none, Key::Escape) {
            if self.fullscreen {
                self.fullscreen = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            } else {
                self.show_info = false;
                self.show_effects = false;
                self.show_help = false;
            }
        }
        // effects (shift combos first so they are not taken by the plain ones)
        let mut fx = self.fx;
        if k(cmd | shift, Key::S) {
            fx.sepia = !fx.sepia;
        }
        if k(cmd, Key::I) {
            fx.negative = !fx.negative;
        }
        if k(cmd, Key::G) {
            fx.gamma = !fx.gamma;
        }
        if k(cmd, Key::F) {
            fx.flip = !fx.flip;
        }
        if k(cmd, Key::M) {
            fx.mirror = !fx.mirror;
        }
        if k(cmd, Key::Y) {
            fx.gray = !fx.gray;
        }
        if k(alt, Key::ArrowUp) {
            fx.rotate = 0;
        }
        if k(alt, Key::ArrowRight) {
            fx.rotate = (fx.rotate + 1) % 4;
        }
        if k(alt, Key::ArrowLeft) {
            fx.rotate = (fx.rotate + 3) % 4;
        }
        if k(alt, Key::ArrowDown) {
            fx.rotate = 2;
        }
        self.set_fx(fx);
        // slideshow: Ctrl+1..9, Ctrl+0 stop
        let nums = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
        for (n, key) in nums.into_iter().enumerate() {
            if k(cmd, key) {
                self.start_slideshow((n as u64 + 1) * 1000);
            }
        }
        if k(cmd, Key::Num0) {
            self.stop_slideshow();
        }
        // view modes: 0 9 8 7 6 5 (menu accelerators), Z = stretch small
        for (key, m) in [
            (Key::Num0, ViewMode::Original),
            (Key::Num9, ViewMode::FitWindow),
            (Key::Num8, ViewMode::FitWidth),
            (Key::Num7, ViewMode::TwoL2R),
            (Key::Num6, ViewMode::TwoR2L),
            (Key::Num5, ViewMode::Webtoon),
        ] {
            if k(none, key) {
                self.set_mode(m);
            }
        }
        if k(none, Key::Z) {
            self.stretch_small = !self.stretch_small;
            self.toast(if self.stretch_small { "작은 그림도 꽉차게 보기: 켜짐" } else { "작은 그림도 꽉차게 보기: 꺼짐" });
        }
        // zoom
        if k(shift, Key::Plus) {
            self.do_zoom(ZOOM_STEP_FINE, scale, ppp, None);
        }
        if k(shift, Key::Minus) {
            self.do_zoom(-ZOOM_STEP_FINE, scale, ppp, None);
        }
        if k(cmd, Key::Plus) || k(cmd, Key::Equals) || k(none, Key::Plus) || k(none, Key::Equals) {
            self.do_zoom(ZOOM_STEP, scale, ppp, None);
        }
        if k(cmd, Key::Minus) || k(none, Key::Minus) {
            self.do_zoom(-ZOOM_STEP, scale, ppp, None);
        }
        if k(none, Key::Slash) {
            self.zoom = None;
            self.pan = Vec2::ZERO;
            self.toast("확대/축소: 꺼짐");
        }
        if k(none, Key::Backtick) {
            self.set_zoom(ZOOM_100, scale, ppp, None);
        }
        // navigation
        if k(none, Key::OpenBracket) {
            self.move_container(-1);
        }
        if k(none, Key::CloseBracket) {
            self.move_container(1);
        }
        if k(none, Key::Home) {
            self.go(0);
        }
        if k(none, Key::End) {
            self.go(self.items.len().saturating_sub(1));
        }
        if k(cmd | alt, Key::PageDown) || k(cmd | alt, Key::PageUp) {
            self.random_page();
        }
        if k(cmd, Key::PageDown) {
            self.next(10.min(self.items.len().saturating_sub(1 + self.cur)).max(1));
        }
        if k(cmd, Key::PageUp) {
            self.prev(10.min(self.cur).max(1));
        }
        if webtoon {
            if k(shift, Key::Space) || k(none, Key::PageUp) {
                self.webtoon_scroll -= view.y * LONG_SCROLL;
            }
            if k(none, Key::Space) || k(none, Key::PageDown) {
                self.webtoon_scroll += view.y * LONG_SCROLL;
            }
            if k(none, Key::ArrowDown) {
                self.webtoon_scroll += view.y * ARROW_SCROLL;
            }
            if k(none, Key::ArrowUp) {
                self.webtoon_scroll -= view.y * ARROW_SCROLL;
            }
            if k(none, Key::ArrowRight) {
                self.next(1);
            }
            if k(none, Key::ArrowLeft) {
                self.prev(1);
            }
        } else {
            let step = self.page_step();
            let r2l = self.mode == ViewMode::TwoR2L;
            if k(none, Key::ArrowRight) && !self.scroll_by(Vec2::new(-view.x * ARROW_SCROLL, 0.0)) {
                if r2l {
                    self.prev(1)
                } else {
                    self.next(step)
                }
            }
            if k(none, Key::ArrowLeft) && !self.scroll_by(Vec2::new(view.x * ARROW_SCROLL, 0.0)) {
                if r2l {
                    self.next(step)
                } else {
                    self.prev(1)
                }
            }
            if (k(none, Key::ArrowDown) || k(none, Key::Space)) && !self.scroll_by(Vec2::new(0.0, -view.y * ARROW_SCROLL * 2.0)) {
                self.next(step);
            }
            if (k(none, Key::ArrowUp) || k(none, Key::Backspace)) && !self.scroll_by(Vec2::new(0.0, view.y * ARROW_SCROLL * 2.0)) {
                self.prev(1);
            }
            if k(none, Key::PageDown) {
                self.next(step);
            }
            if k(none, Key::PageUp) {
                self.prev(1);
            }
        }
    }

    /// Scroll a picture bigger than the window; false when already at that edge.
    fn scroll_by(&mut self, d: Vec2) -> bool {
        let before = self.pan;
        self.pan += d;
        self.clamp_pan(before)
    }

    fn clamp_pan(&mut self, before: Vec2) -> bool {
        let (lo, hi) = self.pan_range;
        self.pan = self.pan.clamp(lo, hi);
        (self.pan - before).length() > 0.5
    }

    fn set_fx(&mut self, fx: Effects) {
        if fx != self.fx {
            self.fx = fx;
            self.reset_cache();
        }
    }

    fn set_mode(&mut self, m: ViewMode) {
        if m == self.mode {
            return;
        }
        let webtoon_change = self.mode == ViewMode::Webtoon || m == ViewMode::Webtoon;
        self.mode = m;
        self.zoom = None;
        self.pan = Vec2::ZERO;
        if m == ViewMode::Webtoon {
            self.webtoon_jump = Some(self.cur);
        }
        if webtoon_change {
            self.reset_cache(); // webtoon decodes at a smaller width
        }
        self.toast(m.toast());
    }

    fn start_slideshow(&mut self, ms: u64) {
        self.slideshow = Some((ms, Instant::now()));
        let t = if ms % 1000 == 0 { format!("{}", ms / 1000) } else { format!("{:.1}", ms as f32 / 1000.0) };
        self.toast(format!("슬라이드 쇼 시작: {t}초"));
    }

    fn stop_slideshow(&mut self) {
        if self.slideshow.take().is_some() {
            self.toast("슬라이드 쇼 중지");
        }
    }

    fn tick_slideshow(&mut self, ctx: &egui::Context) {
        let Some((ms, t0)) = self.slideshow else { return };
        ctx.request_repaint_after(Duration::from_millis(SLIDESHOW_TICK_MS));
        if t0.elapsed() < Duration::from_millis(ms) {
            return;
        }
        self.slideshow = Some((ms, Instant::now()));
        let step = self.page_step();
        if self.slide_random {
            self.random_page();
        } else if self.cur + step < self.items.len() {
            self.go(self.cur + step);
        } else if self.slide_loop {
            self.go(0);
        } else {
            self.stop_slideshow();
        }
    }

    fn open_convert(&mut self) {
        let out_dir = self
            .container
            .as_ref()
            .map(|c| if c.is_dir() { c.join("converted") } else { c.with_extension("converted") })
            .unwrap_or_else(|| std::env::temp_dir().join("converted"));
        self.convert = Some(ConvertUi {
            settings: convert::Settings {
                format: convert::OutFormat::Jpeg,
                quality: 90,
                resize: convert::Resize::None,
                width: 1920,
                height: 1080,
                out_dir,
            },
            items: self.items.clone(),
            progress: None,
        });
    }

    // ---- panels -----------------------------------------------------------------------------

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("파일", |ui| {
                if ui.button("열기...   Ctrl+O").clicked() {
                    self.pick_file();
                }
                if ui.button("폴더 열기...   F").clicked() {
                    self.pick_folder();
                }
                ui.separator();
                if ui.button("이미지 변환...   Ctrl+T").clicked() {
                    self.open_convert();
                }
                ui.separator();
                if ui.button("종료   X").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("보기", |ui| {
                for m in ViewMode::ALL {
                    if ui.radio(self.mode == m, m.label()).clicked() {
                        self.set_mode(m);
                    }
                }
                ui.separator();
                ui.checkbox(&mut self.stretch_small, "작은 그림도 꽉차게 보기   Z");
                if ui.checkbox(&mut self.smooth, "부드럽게 (보간 필터)").changed() {
                    self.reset_cache();
                }
                if ui.button("웹툰 폭 설정...").clicked() {
                    self.show_webtoon_settings = true;
                }
                ui.separator();
                ui.checkbox(&mut self.show_info, "파일 정보 보기   TAB");
                if ui.button("전체 화면   F11").clicked() {
                    self.fullscreen = !self.fullscreen;
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
                }
            });
            ui.menu_button("이동", |ui| {
                if ui.button("다음 이미지   →").clicked() {
                    let s = self.page_step();
                    self.next(s);
                }
                if ui.button("이전 이미지   ←").clicked() {
                    self.prev(1);
                }
                if ui.button("처음 이미지   Home").clicked() {
                    self.go(0);
                }
                if ui.button("마지막 이미지   End").clicked() {
                    self.go(self.items.len().saturating_sub(1));
                }
                if ui.button("랜덤 순서 다음 이미지   Ctrl+Alt+PgDn").clicked() {
                    self.random_page();
                }
                ui.separator();
                if ui.button("다음 압축 파일/폴더   ]").clicked() {
                    self.move_container(1);
                }
                if ui.button("이전 압축 파일/폴더   [").clicked() {
                    self.move_container(-1);
                }
                ui.separator();
                ui.menu_button("정렬 기준", |ui| {
                    let mut changed = false;
                    for (s, l) in [
                        (SortBy::Name, "이름"),
                        (SortBy::Modified, "수정한 날짜"),
                        (SortBy::Size, "크기"),
                        (SortBy::Ext, "확장자"),
                        (SortBy::Created, "만든 날짜"),
                    ] {
                        if ui.radio(self.sort == s, l).clicked() {
                            self.sort = s;
                            changed = true;
                        }
                    }
                    ui.separator();
                    changed |= ui.checkbox(&mut self.descending, "역순").changed();
                    if changed {
                        self.resort();
                    }
                });
            });
            ui.menu_button("이미지", |ui| {
                let mut fx = self.fx;
                ui.menu_button("이미지 돌려보기", |ui| {
                    for (r, l) in [(0, "돌려보지 않기   Alt+↑"), (3, "왼쪽으로 돌려보기   Alt+←"), (1, "오른쪽으로 돌려보기   Alt+→"), (2, "거꾸로 돌려보기   Alt+↓")] {
                        if ui.radio(fx.rotate == r, l).clicked() {
                            fx.rotate = r;
                        }
                    }
                });
                ui.checkbox(&mut fx.negative, "색상 반전   Ctrl+I");
                ui.checkbox(&mut fx.gamma, "감마 보정   Ctrl+G");
                ui.checkbox(&mut fx.flip, "상하 반전   Ctrl+F");
                ui.checkbox(&mut fx.mirror, "좌우 반전   Ctrl+M");
                ui.checkbox(&mut fx.gray, "흑백   Ctrl+Y");
                ui.checkbox(&mut fx.sepia, "세피아   Ctrl+Shift+S");
                ui.separator();
                if ui.button("밝기 · 대비 · 채도...").clicked() {
                    self.show_effects = true;
                }
                if ui.button("초기화").clicked() {
                    fx = Effects::default();
                }
                self.set_fx(fx);
            });
            ui.menu_button("슬라이드 쇼", |ui| {
                if ui.button("슬라이드 쇼 끄기   Ctrl+0").clicked() {
                    self.stop_slideshow();
                }
                for n in 1..=9u64 {
                    let on = self.slideshow.map(|s| s.0) == Some(n * 1000);
                    if ui.radio(on, format!("{n} 초   Ctrl+{n}")).clicked() {
                        self.start_slideshow(n * 1000);
                    }
                }
                ui.separator();
                ui.checkbox(&mut self.slide_loop, "슬라이드 쇼 반복하기");
                ui.checkbox(&mut self.slide_random, "무작위 페이지 보기");
                ui.checkbox(&mut self.slide_timer, "진행 표시줄 보기");
            });
            ui.menu_button("도움말", |ui| {
                if ui.button("단축키 도움말   F1").clicked() {
                    self.show_help = true;
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new("광고 없음").color(Color32::from_gray(130)).small());
            });
        });
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui, scale: f32, ppp: f32) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if ui.button("열기").on_hover_text("열기 (Ctrl+O)").clicked() {
                self.pick_file();
            }
            if ui.button("⏮").on_hover_text("처음 이미지 (Home)").clicked() {
                self.go(0);
            }
            if ui.button("◀").on_hover_text("이전 이미지 (←)").clicked() {
                self.prev(1);
            }
            let n = self.items.len();
            ui.label(egui::RichText::new(if n == 0 { "0 / 0".into() } else { format!("{} / {}", self.cur + 1, n) }).monospace());
            if ui.button("▶").on_hover_text("다음 이미지 (→)").clicked() {
                let s = self.page_step();
                self.next(s);
            }
            if ui.button("⏭").on_hover_text("마지막 이미지 (End)").clicked() {
                self.go(n.saturating_sub(1));
            }
            if n > 1 {
                let mut p = self.cur + 1;
                ui.spacing_mut().slider_width = (ui.available_width() - 560.0).clamp(80.0, 600.0);
                if ui.add(egui::Slider::new(&mut p, 1..=n).show_value(false)).changed() {
                    self.go(p - 1);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("변환").on_hover_text("이미지 일괄 변환 (Ctrl+T)").clicked() {
                    self.open_convert();
                }
                let playing = self.slideshow.is_some();
                if ui.selectable_label(playing, if playing { "⏸ 슬라이드 쇼" } else { "▶ 슬라이드 쇼" }).clicked() {
                    if playing {
                        self.stop_slideshow()
                    } else {
                        self.start_slideshow(3000)
                    }
                }
                if ui.button("⟳").on_hover_text("오른쪽으로 돌려보기 (Alt+→)").clicked() {
                    let mut fx = self.fx;
                    fx.rotate = (fx.rotate + 1) % 4;
                    self.set_fx(fx);
                }
                if ui.button("⟲").on_hover_text("왼쪽으로 돌려보기 (Alt+←)").clicked() {
                    let mut fx = self.fx;
                    fx.rotate = (fx.rotate + 3) % 4;
                    self.set_fx(fx);
                }
                ui.separator();
                if ui.button("+").on_hover_text("확대 (+)").clicked() {
                    self.do_zoom(ZOOM_STEP, scale, ppp, None);
                }
                let pct = match (self.zoom, self.mode) {
                    (Some(z), _) => z / 100,
                    (None, ViewMode::Webtoon) => 100,
                    (None, _) => (scale * ppp * 100.0).round() as i64,
                };
                if ui.selectable_label(self.zoom.is_some(), format!("{pct}%")).on_hover_text("클릭: 확대/축소 끄기 (/)").clicked() {
                    self.zoom = None;
                    self.pan = Vec2::ZERO;
                }
                if ui.button("−").on_hover_text("축소 (−)").clicked() {
                    self.do_zoom(-ZOOM_STEP, scale, ppp, None);
                }
                ui.separator();
                let mut m = self.mode;
                egui::ComboBox::from_id_salt("mode").selected_text(m.short()).width(90.0).show_ui(ui, |ui| {
                    for v in ViewMode::ALL {
                        ui.selectable_value(&mut m, v, v.label());
                    }
                });
                self.set_mode(m);
            });
        });
    }

    // ---- picture area -----------------------------------------------------------------------

    fn central(&mut self, ui: &mut egui::Ui) -> f32 {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let avail = ui.max_rect();
        let resp = ui.allocate_rect(avail, egui::Sense::click_and_drag());
        let painter = ui.painter_at(avail);
        painter.rect_filled(avail, 0.0, BG);

        if self.items.is_empty() {
            self.start_screen(ui, avail);
            return 1.0 / ppp;
        }

        // wheel (Ctrl+wheel and pinch arrive as zoom)
        let (wheel, zoom_delta, hover) = ctx.input(|i| {
            let mut w = 0.0;
            for e in &i.events {
                if let egui::Event::MouseWheel { delta, unit, modifiers, .. } = e {
                    if !modifiers.command {
                        w += match unit {
                            egui::MouseWheelUnit::Point => delta.y,
                            egui::MouseWheelUnit::Line => delta.y * 40.0,
                            egui::MouseWheelUnit::Page => delta.y * avail.height(),
                        };
                    }
                }
            }
            (w, i.zoom_delta(), i.pointer.hover_pos())
        });
        let wheel = if resp.hovered() { wheel } else { 0.0 };

        let scale = if self.mode == ViewMode::Webtoon {
            self.paint_webtoon(&ctx, &painter, avail, ppp, wheel, &resp)
        } else {
            self.paint_pages(&ctx, &painter, avail, ppp, wheel, &resp)
        };

        if resp.hovered() && (zoom_delta - 1.0).abs() > 1e-3 {
            let step = if zoom_delta > 1.0 { ZOOM_STEP } else { -ZOOM_STEP };
            self.do_zoom(step, scale, ppp, hover.filter(|p| avail.contains(*p)));
        }
        if resp.double_clicked() {
            self.fullscreen = !self.fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
        }

        if let (Some((ms, t0)), true) = (self.slideshow, self.slide_timer) {
            let f = (t0.elapsed().as_secs_f32() * 1000.0 / ms as f32).min(1.0);
            let r = Rect::from_min_size(Pos2::new(avail.min.x, avail.max.y - 3.0), Vec2::new(avail.width() * f, 3.0));
            painter.rect_filled(r, 0.0, ACCENT);
        }
        if self.show_info {
            self.info_overlay(&painter, avail, scale, ppp);
        }
        scale
    }

    fn paint_pages(&mut self, ctx: &egui::Context, painter: &egui::Painter, avail: Rect, ppp: f32, wheel: f32, resp: &egui::Response) -> f32 {
        let pair = self.two_page() && self.pair_at(self.cur);
        let mut pages = vec![self.cur];
        if pair {
            pages.push(self.cur + 1);
        }
        for i in [self.cur, self.cur + 1, self.cur + 2, self.cur.saturating_sub(1)] {
            self.request(ctx, i);
        }
        self.evict(self.cur.saturating_sub(2)..=self.cur + 3);

        let sizes: Vec<Vec2> = pages.iter().map(|&i| self.page_size(i).unwrap_or(Vec2::new(800.0, 600.0))).collect();
        // Two pages share one height; together they are laid out as one picture.
        let h = sizes.iter().map(|s| s.y).fold(0.0, f32::max);
        let widths: Vec<f32> = sizes.iter().map(|s| s.x * h / s.y).collect();
        let unit = Vec2::new(widths.iter().sum(), h);
        let base = self.base_scale(unit, avail.size(), ppp);
        let scale = self.zoom.map(|z| z as f32 / ZOOM_100 as f32 / ppp).unwrap_or(base);
        let size = unit * scale;

        // the picture can move until its edge meets the window edge
        let lo = Vec2::new((avail.width() - size.x).min(0.0), (avail.height() - size.y).min(0.0));
        self.pan_range = (lo, Vec2::ZERO);
        if resp.dragged() {
            self.pan += resp.drag_delta();
        }
        if wheel != 0.0 {
            let before = self.pan;
            self.pan.y += wheel;
            if size.y <= avail.height() + 0.5 || !self.clamp_pan(before) {
                self.pan = before;
                self.wheel_nav(wheel);
            }
        }
        self.pan = self.pan.clamp(lo, Vec2::ZERO);
        let min = Pos2::new(
            if size.x <= avail.width() { avail.center().x - size.x / 2.0 } else { avail.min.x + self.pan.x },
            if size.y <= avail.height() { avail.center().y - size.y / 2.0 } else { avail.min.y + self.pan.y },
        );
        let rect = Rect::from_min_size(min, size);
        self.last_img_rect = rect;
        // keep pan consistent with where a centred picture really is (for zoom anchoring)
        self.pan = rect.min - avail.min;

        let mut order: Vec<(usize, f32)> = pages.iter().copied().zip(widths.iter().copied()).collect();
        if self.mode == ViewMode::TwoR2L {
            order.reverse();
        }
        let mut x = rect.min.x;
        for (i, w) in order {
            let r = Rect::from_min_size(Pos2::new(x, rect.min.y), Vec2::new(w * scale, size.y));
            x += w * scale;
            self.paint_slot(painter, i, r);
        }
        scale
    }

    fn wheel_nav(&mut self, wheel: f32) {
        self.wheel_acc += wheel;
        if self.wheel_acc <= -40.0 {
            self.wheel_acc = 0.0;
            let s = self.page_step();
            self.next(s);
        } else if self.wheel_acc >= 40.0 {
            self.wheel_acc = 0.0;
            self.prev(1);
        }
    }

    fn paint_slot(&self, painter: &egui::Painter, i: usize, r: Rect) {
        match self.cache.get(&i) {
            Some(Slot::Ready(p)) => paint_pic(painter, p, r),
            Some(Slot::Failed(e)) => {
                painter.rect_filled(r.shrink(2.0), 4.0, Color32::from_rgb(52, 36, 36));
                painter.text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("{}\n이미지를 열 수 없습니다\n{e}", self.items[i].name()),
                    egui::FontId::proportional(15.0),
                    Color32::from_gray(200),
                );
            }
            _ => {
                painter.rect_filled(r.shrink(1.0), 0.0, Color32::from_gray(32));
                painter.text(r.center(), egui::Align2::CENTER_CENTER, "불러오는 중…", egui::FontId::proportional(14.0), Color32::from_gray(140));
            }
        }
    }

    fn paint_webtoon(&mut self, ctx: &egui::Context, painter: &egui::Painter, avail: Rect, ppp: f32, wheel: f32, resp: &egui::Response) -> f32 {
        self.start_dims_job(ctx);
        let (tops, widths, total) = self.webtoon_layout(avail.width(), ppp);
        // jump once the sizes above the target page are known, so the layout no longer shifts
        if let Some(n) = self.webtoon_jump {
            self.webtoon_scroll = tops.get(n).copied().unwrap_or(0.0);
            if (0..n).all(|i| self.page_size(i).is_some()) {
                self.webtoon_jump = None;
            }
        }
        if resp.dragged() {
            self.webtoon_scroll -= resp.drag_delta().y;
        }
        self.webtoon_scroll -= wheel;
        self.webtoon_scroll = self.webtoon_scroll.clamp(0.0, (total - avail.height()).max(0.0));
        let y0 = self.webtoon_scroll;

        let first = tops.partition_point(|&t| t <= y0).saturating_sub(1);
        let mut last = first;
        while last + 1 < tops.len() && tops[last + 1] < y0 + avail.height() {
            last += 1;
        }
        let centre = tops.partition_point(|&t| t <= y0 + avail.height() / 2.0).saturating_sub(1);
        self.cur = centre.min(self.items.len() - 1);
        for i in first..=(last + 2).min(self.items.len() - 1) {
            self.request(ctx, i);
        }
        self.evict(first.saturating_sub(3)..=last + 5);
        let gap = if self.webtoon_spacing { WEBTOON_SPACE } else { 0.0 };
        for i in first..=last {
            let h = tops.get(i + 1).copied().unwrap_or(total) - tops[i] - gap;
            let r = Rect::from_min_size(Pos2::new(avail.center().x - widths[i] / 2.0, avail.min.y + tops[i] - y0), Vec2::new(widths[i], h));
            self.paint_slot(painter, i, r);
        }
        let col_w = widths.get(self.cur).copied().unwrap_or(avail.width());
        self.last_img_rect = Rect::from_center_size(avail.center(), Vec2::new(col_w, avail.height()));
        if let Some((c, _)) = &self.dims_job {
            let msg = format!("웹툰 모드용 이미지 정보 읽는 중… {}/{}", c.load(Ordering::Relaxed), self.items.len());
            painter.text(avail.center_top() + Vec2::new(0.0, 30.0), egui::Align2::CENTER_CENTER, msg, egui::FontId::proportional(14.0), Color32::WHITE);
        }
        if total > avail.height() {
            let bar = Rect::from_min_size(
                Pos2::new(avail.max.x - 6.0, avail.min.y + y0 / total * avail.height()),
                Vec2::new(4.0, (avail.height() * avail.height() / total).max(20.0)),
            );
            painter.rect_filled(bar, 2.0, Color32::from_white_alpha(70));
        }
        self.page_size(self.cur).map(|s| col_w / s.x).unwrap_or(1.0 / ppp)
    }

    fn info_overlay(&self, painter: &egui::Painter, avail: Rect, scale: f32, ppp: f32) {
        let Some(item) = self.items.get(self.cur) else { return };
        let mut lines = vec![format!("파일 이름   {}", item.name())];
        if let Some(Slot::Ready(p)) = self.cache.get(&self.cur) {
            lines.push(format!("이미지 크기   {} × {}   ({})", p.orig[0], p.orig[1], p.format));
            if p.ultra_hdr {
                lines.push("HDR   Ultra HDR (게인맵 포함) · 표준 밝기 층으로 표시".into());
            }
        }
        if let Some(m) = item.file_meta() {
            lines.push(format!("파일 크기   {}", human_size(m.len())));
            if let Ok(t) = m.modified() {
                lines.push(format!("수정한 날짜   {}", fmt_time(t)));
            }
            if let Ok(t) = m.created() {
                lines.push(format!("만든 날짜   {}", fmt_time(t)));
            }
        }
        if let Item::Zip(z, _) = item {
            lines.push(format!("압축 파일   {}", z.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
        }
        lines.push(format!("확대/축소   {:.0}%", scale * ppp * 100.0));
        lines.push(format!("회전   {}°", self.fx.rotate as u32 * 90));
        let galley = painter.layout_no_wrap(lines.join("\n"), egui::FontId::proportional(14.0), Color32::from_gray(230));
        let r = Rect::from_min_size(avail.min + Vec2::new(12.0, 12.0), galley.size() + Vec2::splat(20.0));
        painter.rect_filled(r, 6.0, Color32::from_black_alpha(190));
        painter.galley(r.min + Vec2::splat(10.0), galley, Color32::WHITE);
    }

    fn start_screen(&mut self, ui: &mut egui::Ui, avail: Rect) {
        let painter = ui.painter_at(avail);
        let c = avail.center() - Vec2::new(0.0, 90.0);
        let logo = self.logo.get_or_insert_with(|| {
            let icon = eframe::icon_data::from_png_bytes(ICON).expect("valid TailView icon");
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [icon.width as usize, icon.height as usize], &icon.rgba,
            );
            ui.ctx().load_texture("tailview-logo", image, egui::TextureOptions::LINEAR)
        });
        painter.image(logo.id(), Rect::from_center_size(c, Vec2::splat(150.0)),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        painter.text(c + Vec2::new(0.0, 100.0), egui::Align2::CENTER_CENTER, "TailView", egui::FontId::proportional(28.0), Color32::WHITE);
        painter.text(
            c + Vec2::new(0.0, 135.0),
            egui::Align2::CENTER_CENTER,
            "빠르고 가벼운 이미지 · 만화 · 웹툰 뷰어 — 광고 없음",
            egui::FontId::proportional(15.0),
            Color32::from_gray(170),
        );
        let btn_rect = Rect::from_center_size(c + Vec2::new(0.0, 185.0), Vec2::new(320.0, 40.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(btn_rect), |ui| {
            ui.horizontal_centered(|ui| {
                if ui.add_sized([150.0, 34.0], egui::Button::new(egui::RichText::new("파일 열기  Ctrl+O").color(Color32::BLACK)).fill(ACCENT)).clicked() {
                    self.pick_file();
                }
                if ui.add_sized([150.0, 34.0], egui::Button::new("폴더 열기  F")).clicked() {
                    self.pick_folder();
                }
            });
        });
        painter.text(
            c + Vec2::new(0.0, 235.0),
            egui::Align2::CENTER_CENTER,
            "이미지 · 폴더 · ZIP/CBZ 파일을 여기로 끌어다 놓으세요",
            egui::FontId::proportional(13.0),
            Color32::from_gray(130),
        );
        painter.text(
            c + Vec2::new(0.0, 260.0),
            egui::Align2::CENTER_CENTER,
            "JPG PNG GIF WebP BMP TIFF TGA HDR EXR PSD CLIP HEIC AVIF RAW ZIP CBZ",
            egui::FontId::monospace(11.0),
            Color32::from_gray(100),
        );
    }

    // ---- dialogs ----------------------------------------------------------------------------

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.show_effects {
            let mut open = true;
            let mut fx = self.pending_fx.unwrap_or(self.fx);
            egui::Window::new("이미지 효과").open(&mut open).resizable(false).show(ctx, |ui| {
                ui.add(egui::Slider::new(&mut fx.brightness, -100..=100).text("밝기 조절"));
                ui.add(egui::Slider::new(&mut fx.contrast, -100..=100).text("대비 조절"));
                ui.add(egui::Slider::new(&mut fx.saturation, -100..=100).text("채도 조절"));
                if ui.button("초기화").clicked() {
                    fx.brightness = 0;
                    fx.contrast = 0;
                    fx.saturation = 0;
                }
            });
            self.show_effects = open;
            self.pending_fx = Some(fx);
        }
        // apply slider changes when the mouse is released, not on every step
        if let Some(fx) = self.pending_fx {
            if !ctx.input(|i| i.pointer.any_down()) {
                self.pending_fx = None;
                self.set_fx(fx);
            }
        }
        if self.show_webtoon_settings {
            let mut open = true;
            egui::Window::new("웹툰 보기 모드 설정").open(&mut open).resizable(false).show(ctx, |ui| {
                ui.checkbox(&mut self.webtoon_apply_width, "웹툰 폭 지정 (끄면 창 폭에 맞춤)");
                ui.add_enabled_ui(self.webtoon_apply_width, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("웹툰 폭");
                        ui.add(egui::DragValue::new(&mut self.webtoon_width).range(200..=4000).suffix(" px"));
                    });
                    ui.horizontal(|ui| {
                        for w in WEBTOON_PRESETS {
                            if ui.selectable_label(self.webtoon_width == w, format!("{w}")).clicked() {
                                self.webtoon_width = w;
                            }
                        }
                    });
                });
                ui.checkbox(&mut self.webtoon_spacing, "웹툰 보기에서 이미지 사이 간격 넣기");
            });
            self.show_webtoon_settings = open;
        }
        if self.show_help {
            let mut open = true;
            egui::Window::new("단축키 도움말").open(&mut open).show(ctx, |ui| {
                egui::Grid::new("keys").striped(true).show(ui, |ui| {
                    for (k, d) in [
                        ("← → / 휠", "이전 / 다음 이미지"),
                        ("Home / End", "처음 / 마지막 이미지"),
                        ("Ctrl+PgUp / PgDn", "10장 이동"),
                        ("[ / ]", "이전 / 다음 폴더 · 압축 파일"),
                        ("0 9 8 7 6 5", "원본 · 꽉차게 · 폭맞춤 · 두장(→) · 두장(←) · 웹툰"),
                        ("Space / PgDn", "웹툰: 길게 아래로"),
                        ("+ / − / Ctrl+휠", "확대 / 축소 (Shift: 1%)"),
                        ("`  /  /", "100% / 확대 끄기"),
                        ("Alt+화살표", "돌려보기"),
                        ("Ctrl+I G F M Y", "반전 · 감마 · 상하 · 좌우 · 흑백"),
                        ("Ctrl+1…9 / Ctrl+0", "슬라이드 쇼 N초 / 끄기"),
                        ("Tab", "파일 정보"),
                        ("Ctrl+T", "이미지 일괄 변환"),
                        ("F11 / 더블클릭", "전체 화면"),
                        ("X", "종료"),
                    ] {
                        ui.monospace(k);
                        ui.label(d);
                        ui.end_row();
                    }
                });
            });
            self.show_help = open;
        }
        self.convert_dialog(ctx);
    }

    fn convert_dialog(&mut self, ctx: &egui::Context) {
        let Some(cv) = &mut self.convert else { return };
        let mut open = true;
        let mut pick_files = false;
        egui::Window::new("이미지 변환").open(&mut open).resizable(false).default_width(420.0).show(ctx, |ui| {
            let running = cv.progress.as_ref().map(|p| !p.finished.load(Ordering::Relaxed)).unwrap_or(false);
            ui.add_enabled_ui(!running, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("변환할 파일: {}개", cv.items.len()));
                    if ui.button("파일 고르기...").clicked() {
                        pick_files = true;
                    }
                });
                ui.separator();
                ui.strong("포맷 설정");
                let s = &mut cv.settings;
                ui.horizontal(|ui| {
                    ui.label("포맷:");
                    egui::ComboBox::from_id_salt("fmt").selected_text(s.format.label()).show_ui(ui, |ui| {
                        for f in convert::OutFormat::ALL {
                            ui.selectable_value(&mut s.format, f, f.label());
                        }
                    });
                });
                ui.add_enabled(s.format == convert::OutFormat::Jpeg, egui::Slider::new(&mut s.quality, 1..=100).text("품질"));
                ui.separator();
                ui.strong("크기 변경");
                egui::ComboBox::from_id_salt("rs").selected_text(s.resize.label()).width(260.0).show_ui(ui, |ui| {
                    for r in convert::Resize::ALL {
                        ui.selectable_value(&mut s.resize, r, r.label());
                    }
                });
                if s.resize != convert::Resize::None {
                    ui.horizontal(|ui| {
                        ui.add_enabled(s.resize != convert::Resize::FitHeight, egui::DragValue::new(&mut s.width).range(1..=30000).prefix("가로 "));
                        ui.add_enabled(s.resize != convert::Resize::FitWidth, egui::DragValue::new(&mut s.height).range(1..=30000).prefix("세로 "));
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("저장 폴더:");
                    ui.label(egui::RichText::new(s.out_dir.display().to_string()).small());
                    if ui.button("바꾸기...").clicked() {
                        if let Some(d) = rfd::FileDialog::new().pick_folder() {
                            s.out_dir = d;
                        }
                    }
                });
                ui.label(
                    egui::RichText::new(format!("같은 이름이 있으면 '이름 (2)'로 저장 · CPU {}개 코어로 동시 변환", rayon::current_num_threads()))
                        .small()
                        .weak(),
                );
            });
            ui.separator();
            if let Some(p) = &cv.progress {
                let (t, d, f) = (p.total.load(Ordering::Relaxed), p.done.load(Ordering::Relaxed), p.failed.load(Ordering::Relaxed));
                ui.add(egui::ProgressBar::new((d + f) as f32 / t.max(1) as f32).text(format!("{}/{t}", d + f)));
                if p.finished.load(Ordering::Relaxed) {
                    let secs = *p.seconds.lock().unwrap();
                    ui.label(format!("처리가 완료되었습니다. 성공 {d} · 실패 {f} · {secs:.1}초 (초당 {:.1}장)", d as f32 / secs.max(0.001)));
                    for e in p.errors.lock().unwrap().iter().take(5) {
                        ui.label(egui::RichText::new(e).small().color(Color32::from_rgb(230, 120, 120)));
                    }
                } else {
                    ctx.request_repaint_after(Duration::from_millis(100));
                    if ui.button("취소").clicked() {
                        p.cancel.store(true, Ordering::Relaxed);
                    }
                }
            }
            if !running && ui.add_enabled(!cv.items.is_empty(), egui::Button::new("변환 시작")).clicked() {
                cv.progress = Some(convert::start(cv.items.clone(), cv.settings.clone()));
            }
        });
        if pick_files {
            if let Some(files) = rfd::FileDialog::new().pick_files() {
                cv.items = files.into_iter().filter(|p| decode::is_image_name(&p.to_string_lossy())).map(Item::File).collect();
                if let Some(Item::File(p)) = cv.items.first() {
                    if let Some(par) = p.parent() {
                        cv.settings.out_dir = par.join("converted");
                    }
                }
            }
        }
        if !open {
            if let Some(p) = &cv.progress {
                p.cancel.store(true, Ordering::Relaxed);
            }
            self.convert = None;
        }
    }

    fn update_title(&mut self, ctx: &egui::Context, scale: f32, ppp: f32) {
        let t = match self.items.get(self.cur) {
            Some(it) => {
                let dims = match self.cache.get(&self.cur) {
                    Some(Slot::Ready(p)) => format!(" - {}×{}", p.orig[0], p.orig[1]),
                    _ => String::new(),
                };
                // "${folder} ${sep} ${file} [${page}/${total}]" (FUN_14007a090)
                let folder = self.container.as_ref().and_then(|c| c.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                format!("{folder} - {} [{}/{}]{dims} - {:.0}% - TailView", it.name(), self.cur + 1, self.items.len(), scale * ppp * 100.0)
            }
            None => "TailView".into(),
        };
        if t != self.title {
            self.title = t.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(t));
        }
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        self.frame += 1;
        ctx.request_repaint();
        if self.frame == 120 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None })
        });
        if let Some(img) = shot {
            let [w, h] = img.size;
            let raw: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Some(buf) = RgbaImage::from_raw(w as u32, h as u32, raw) {
                if let Err(e) = buf.save(&path) {
                    eprintln!("screenshot: {e}");
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.frame > 600 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn make_pic(ctx: &egui::Context, img: &RgbaImage, orig: [u32; 2], format: String, ultra_hdr: bool, opts: egui::TextureOptions) -> Pic {
    let (w, h) = img.dimensions();
    let mut tiles = vec![];
    for y in (0..h).step_by(TILE as usize) {
        let th = TILE.min(h - y);
        for x in (0..w).step_by(TILE as usize) {
            let tw = TILE.min(w - x);
            let sub = image::imageops::crop_imm(img, x, y, tw, th).to_image();
            let ci = egui::ColorImage::from_rgba_unmultiplied([tw as usize, th as usize], sub.as_raw());
            let tex = ctx.load_texture(format!("p{x}_{y}"), ci, opts);
            let r = Rect::from_min_max(
                Pos2::new(x as f32 / w as f32, y as f32 / h as f32),
                Pos2::new((x + tw) as f32 / w as f32, (y + th) as f32 / h as f32),
            );
            tiles.push((tex, r));
        }
    }
    Pic { size: [w, h], orig, tiles, format, ultra_hdr }
}

fn paint_pic(painter: &egui::Painter, p: &Pic, r: Rect) {
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    for (tex, f) in &p.tiles {
        let tr = Rect::from_min_max(r.min + f.min.to_vec2() * r.size(), r.min + f.max.to_vec2() * r.size());
        if painter.clip_rect().intersects(tr) {
            painter.image(tex.id(), tr, uv, Color32::WHITE);
        }
    }
}

fn human_size(n: u64) -> String {
    let n = n as f64;
    if n >= 1048576.0 {
        format!("{:.1} MB", n / 1048576.0)
    } else if n >= 1024.0 {
        format!("{:.1} KB", n / 1024.0)
    } else {
        format!("{n} B")
    }
}

fn fmt_time(t: std::time::SystemTime) -> String {
    // Civil date from days since 1970 (UTC), no extra crate needed.
    let secs = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y}-{m:02}-{d:02} {:02}:{:02} (UTC)", rem / 3600, rem % 3600 / 60)
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        self.receive(&ctx);
        self.tick_slideshow(&ctx);

        let dropped: Option<PathBuf> = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()));
        if let Some(p) = dropped {
            self.open_path(&p);
        }

        let scale_prev = self.last_scale;
        if !self.fullscreen {
            egui::Panel::top("menu")
                .frame(egui::Frame::new().fill(BAR).inner_margin(egui::Margin::symmetric(8, 4)))
                .show(ui, |ui| self.menu_bar(ui));
            egui::Panel::bottom("bar")
                .exact_size(38.0)
                .frame(egui::Frame::new().fill(BAR).inner_margin(egui::Margin::symmetric(8, 4)))
                .show(ui, |ui| self.bottom_bar(ui, scale_prev, ppp));
        }
        let scale = egui::CentralPanel::no_frame().show(ui, |ui| self.central(ui)).inner;
        self.last_scale = scale;
        let view = ctx.content_rect().size();
        self.handle_keys(&ctx, view, ppp, scale);
        self.dialogs(&ctx);
        self.update_title(&ctx, scale, ppp);

        if let Some((msg, t)) = &self.toast {
            let age = t.elapsed().as_secs_f32();
            if age > 1.6 {
                self.toast = None;
            } else {
                let alpha = ((1.6 - age) / 0.3).min(1.0);
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("toast")));
                let pos = ctx.content_rect().center_top() + Vec2::new(0.0, 70.0);
                let g = painter.layout_no_wrap(msg.clone(), egui::FontId::proportional(16.0), Color32::WHITE);
                let r = Rect::from_center_size(pos, g.size() + Vec2::new(28.0, 14.0));
                painter.rect_filled(r, 8.0, Color32::from_black_alpha((200.0 * alpha) as u8));
                painter.galley(r.min + Vec2::new(14.0, 7.0), g, Color32::from_white_alpha((255.0 * alpha) as u8));
                ctx.request_repaint();
            }
        }
        self.screenshot_step(&ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zoom_steps_match_original() {
        assert_eq!(zoom_step(10000, ZOOM_STEP), 11000);
        assert_eq!(zoom_step(29000, ZOOM_STEP), 30000);
        assert_eq!(zoom_step(30000, ZOOM_STEP), 32000);
        assert_eq!(zoom_step(50000, ZOOM_STEP), 55000);
        assert_eq!(zoom_step(55000, -ZOOM_STEP), 50000);
        assert_eq!(zoom_step(32000, -ZOOM_STEP), 30000);
        assert_eq!(zoom_step(12345, ZOOM_STEP), 13000);
        assert_eq!(zoom_step(1000, -ZOOM_STEP), ZOOM_MIN);
        assert_eq!(zoom_step(300, ZOOM_STEP), 1000);
        assert_eq!(zoom_step(10000, ZOOM_STEP_FINE), 10100);
        assert_eq!(zoom_step(1_998_000, ZOOM_STEP), ZOOM_MAX);
    }
}
