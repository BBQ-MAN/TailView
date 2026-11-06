//! Finding images in folders and archives, and decoding every supported format.

use image::RgbaImage;
use std::cmp::Ordering;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Formats the `image` crate reads directly.
const NATIVE_EXT: &[&str] = &[
    "jpg", "jpeg", "jfif", "png", "apng", "gif", "webp", "bmp", "tif", "tiff", "tga", "ico", "hdr",
    "exr", "pnm", "pbm", "pgm", "ppm", "qoi", "dds",
];
/// Formats we decode ourselves.
const OWN_EXT: &[&str] = &["psd", "clip"];
/// Formats handed to the operating system's codecs (Windows WIC / macOS ImageIO).
const OS_EXT: &[&str] = &[
    "heic", "heif", "hif", "avif", "jxl", "dng", "cr2", "cr3", "nef", "nrw", "arw", "orf", "rw2",
    "raf", "psb",
];
pub const ARCHIVE_EXT: &[&str] = &["zip", "cbz"];

pub fn ext_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

pub fn is_image_name(name: &str) -> bool {
    let e = ext_of(name);
    NATIVE_EXT.contains(&e.as_str()) || OWN_EXT.contains(&e.as_str()) || OS_EXT.contains(&e.as_str())
}

pub fn is_archive(p: &Path) -> bool {
    ARCHIVE_EXT.contains(&ext_of(&p.to_string_lossy()).as_str())
}

/// One page: a file on disk or an entry inside a zip/cbz.
#[derive(Clone, Debug)]
pub enum Item {
    File(PathBuf),
    Zip(PathBuf, String),
}

impl Item {
    pub fn name(&self) -> String {
        match self {
            Item::File(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            Item::Zip(_, e) => e.rsplit('/').next().unwrap_or(e).to_string(),
        }
    }
    pub fn bytes(&self) -> Result<Vec<u8>, String> {
        match self {
            Item::File(p) => std::fs::read(p).map_err(|e| e.to_string()),
            Item::Zip(z, entry) => {
                let f = std::fs::File::open(z).map_err(|e| e.to_string())?;
                let mut a = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
                let mut e = a.by_name(entry).map_err(|e| e.to_string())?;
                let mut v = Vec::with_capacity(e.size() as usize);
                e.read_to_end(&mut v).map_err(|e| e.to_string())?;
                Ok(v)
            }
        }
    }
    pub fn file_meta(&self) -> Option<std::fs::Metadata> {
        match self {
            Item::File(p) => std::fs::metadata(p).ok(),
            Item::Zip(..) => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortBy {
    Name,
    Modified,
    Size,
    Ext,
    Created,
}

/// Explorer-like natural order: "2.jpg" before "10.jpg", case-insensitive.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().copied().filter(|c| c.is_ascii_digit()) {
                    na.push(c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().copied().filter(|c| c.is_ascii_digit()) {
                    nb.push(c);
                    bi.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let o = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

pub fn sort_items(items: &mut [Item], by: SortBy, descending: bool) {
    let key_time = |i: &Item, created: bool| {
        i.file_meta()
            .and_then(|m| if created { m.created().ok() } else { m.modified().ok() })
            .unwrap_or(std::time::UNIX_EPOCH)
    };
    match by {
        SortBy::Name => items.sort_by(|a, b| natural_cmp(&a.name(), &b.name())),
        SortBy::Ext => items.sort_by(|a, b| {
            ext_of(&a.name()).cmp(&ext_of(&b.name())).then_with(|| natural_cmp(&a.name(), &b.name()))
        }),
        SortBy::Size => items.sort_by_key(|i| i.file_meta().map(|m| m.len()).unwrap_or(0)),
        SortBy::Modified => items.sort_by_key(|i| key_time(i, false)),
        SortBy::Created => items.sort_by_key(|i| key_time(i, true)),
    }
    if descending {
        items.reverse();
    }
}

/// All images in a folder, or in a zip/cbz.
pub fn list_container(p: &Path) -> Vec<Item> {
    if is_archive(p) {
        let Ok(f) = std::fs::File::open(p) else { return vec![] };
        let Ok(a) = zip::ZipArchive::new(f) else { return vec![] };
        a.file_names()
            .filter(|n| !n.ends_with('/') && is_image_name(n))
            .map(|n| Item::Zip(p.to_path_buf(), n.to_string()))
            .collect()
    } else {
        std::fs::read_dir(p)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_file() && is_image_name(&p.to_string_lossy()))
                    .map(Item::File)
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Sibling folders and archives of `p`, in natural order (for the [ and ] keys).
pub fn sibling_containers(p: &Path) -> Vec<PathBuf> {
    let Some(parent) = p.parent() else { return vec![] };
    let mut v: Vec<PathBuf> = std::fs::read_dir(parent)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|q| q.is_dir() || is_archive(q)).collect())
        .unwrap_or_default();
    v.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
    v
}

pub struct Decoded {
    pub img: RgbaImage,
    pub format: String,
    pub ultra_hdr: bool,
}

/// Ultra HDR JPEG = JPEG with an embedded gain map (XMP hdrgm namespace + MPF second image).
pub fn is_ultra_hdr(bytes: &[u8]) -> bool {
    fn has(h: &[u8], n: &[u8]) -> bool {
        h.windows(n.len()).any(|w| w == n)
    }
    bytes.starts_with(&[0xFF, 0xD8]) && has(&bytes[..bytes.len().min(256 * 1024)], b"hdrgm:")
}

pub fn decode(item: &Item) -> Result<Decoded, String> {
    let bytes = item.bytes()?;
    decode_bytes(&item.name(), &bytes)
}

pub fn decode_bytes(name: &str, bytes: &[u8]) -> Result<Decoded, String> {
    let ext = ext_of(name);
    let format = ext.to_uppercase();
    let img = match ext.as_str() {
        "psd" => {
            let p = psd::Psd::from_bytes(bytes).map_err(|e| e.to_string())?;
            RgbaImage::from_raw(p.width(), p.height(), p.rgba()).ok_or("PSD 크기 오류")?
        }
        "clip" => decode_clip(bytes)?,
        e if OS_EXT.contains(&e) => decode_with_os(name, bytes)?,
        _ => match image::load_from_memory(bytes) {
            Ok(i) => i.to_rgba8(),
            // Some files carry the wrong extension: let the OS try.
            Err(e) => decode_with_os(name, bytes).map_err(|_| e.to_string())?,
        },
    };
    Ok(Decoded { img, format, ultra_hdr: is_ultra_hdr(bytes) })
}

/// Fast size read for webtoon layout; falls back to a full decode.
pub fn dimensions(item: &Item) -> Option<[u32; 2]> {
    if let Item::File(p) = item {
        if NATIVE_EXT.contains(&ext_of(&item.name()).as_str()) {
            if let Ok((w, h)) = image::image_dimensions(p) {
                return Some([w, h]);
            }
        }
    }
    let bytes = item.bytes().ok()?;
    if let Ok(r) = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format() {
        if let Ok((w, h)) = r.into_dimensions() {
            return Some([w, h]);
        }
    }
    decode_bytes(&item.name(), &bytes).ok().map(|d| [d.img.width(), d.img.height()])
}

/// Clip Studio .clip: a chunk file holding an SQLite database; its CanvasPreview table has a PNG.
fn decode_clip(bytes: &[u8]) -> Result<RgbaImage, String> {
    let start = find(bytes, b"SQLite format 3\0").ok_or("CLIP: 데이터베이스 없음")?;
    let end = find(&bytes[start..], b"CHNKFoot").map(|e| start + e).unwrap_or(bytes.len());
    let tmp = temp_path("clip", "sqlite");
    std::fs::write(&tmp, &bytes[start..end]).map_err(|e| e.to_string())?;
    let png: Result<Vec<u8>, String> = (|| {
        let db = rusqlite::Connection::open(&tmp).map_err(|e| e.to_string())?;
        db.query_row("SELECT ImageData FROM CanvasPreview ORDER BY rowid DESC LIMIT 1", [], |r| r.get(0))
            .map_err(|e| e.to_string())
    })();
    let _ = std::fs::remove_file(&tmp);
    Ok(image::load_from_memory(&png?).map_err(|e| e.to_string())?.to_rgba8())
}

/// Unique temp file name, safe when several worker threads decode at once.
fn temp_path(kind: &str, ext: &str) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("bv_{kind}_{}_{n}.{ext}", std::process::id()))
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// HEIC/AVIF/RAW and friends: convert to PNG with the system's own codecs.
fn decode_with_os(name: &str, bytes: &[u8]) -> Result<RgbaImage, String> {
    let src = temp_path("in", &ext_of(name));
    let dst = temp_path("out", "png");
    std::fs::write(&src, bytes).map_err(|e| e.to_string())?;
    let ok = os_convert(&src, &dst);
    let _ = std::fs::remove_file(&src);
    let out = std::fs::read(&dst);
    let _ = std::fs::remove_file(&dst);
    if !ok {
        return Err(format!("{} 해독기가 이 컴퓨터에 없습니다", ext_of(name).to_uppercase()));
    }
    Ok(image::load_from_memory(&out.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?.to_rgba8())
}

#[cfg(target_os = "windows")]
fn os_convert(src: &Path, dst: &Path) -> bool {
    use std::os::windows::process::CommandExt;
    // WPF's BitmapDecoder uses every installed WIC codec (HEIF/AV1/RAW extensions).
    let script = format!(
        "Add-Type -AssemblyName PresentationCore; \
         $s=[IO.File]::OpenRead('{}'); \
         $d=[Windows.Media.Imaging.BitmapDecoder]::Create($s,'None','OnLoad'); \
         $e=New-Object Windows.Media.Imaging.PngBitmapEncoder; \
         $e.Frames.Add([Windows.Media.Imaging.BitmapFrame]::Create($d.Frames[0])); \
         $o=[IO.File]::Create('{}'); $e.Save($o); $o.Close(); $s.Close()",
        src.display().to_string().replace('\'', "''"),
        dst.display().to_string().replace('\'', "''")
    );
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
        && dst.exists()
}

#[cfg(target_os = "macos")]
fn os_convert(src: &Path, dst: &Path) -> bool {
    std::process::Command::new("sips")
        .args(["-s", "format", "png"])
        .arg(src)
        .arg("--out")
        .arg(dst)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
        && dst.exists()
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn os_convert(_src: &Path, _dst: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn natural_order() {
        assert_eq!(natural_cmp("2.jpg", "10.jpg"), Ordering::Less);
        assert_eq!(natural_cmp("a10", "A9"), Ordering::Greater);
        assert_eq!(natural_cmp("img001", "img1"), Ordering::Equal);
    }
}
