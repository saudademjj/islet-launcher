//! Screenshots the game saved for an instance. The gallery shows small cached
//! thumbnails, so the page never decodes a wall of full-size images.

use std::fs;
use std::io::BufReader;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use sha1::{Digest, Sha1};

use crate::core::error::{AppError, AppResult};
use crate::core::paths::AppPaths;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Screenshot {
    pub name: String,
    pub bytes: u64,
    pub modified_ms: i64,
}

const LIMIT: usize = 500;
/// Twice the widest tile, for sharp thumbnails on Retina screens.
const THUMB_WIDTH: usize = 480;
/// What a thumbnail will decode. The PNG decoder's own limit covers its
/// internal buffers, not the frame buffer we allocate from the header, so the
/// header is checked first: a crafted 65535×65535 file would otherwise ask
/// for 4 GiB+. 16384 a side and 64 megapixels cover any real screenshot
/// (8K is 33 megapixels); larger files are served as they are.
const MAX_SIDE: u32 = 16_384;
const MAX_PIXELS: u64 = 64 * 1024 * 1024;
const MAX_FRAME_BYTES: usize = 256 * 1024 * 1024;

fn is_image(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg")
}

fn modified_ms(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

pub fn folder(paths: &AppPaths, id: &str) -> AppResult<PathBuf> {
    AppPaths::validate_instance_id(id)?;
    Ok(paths.instance_game_dir(id).join("screenshots"))
}

/// Newest first.
pub fn list(paths: &AppPaths, id: &str) -> AppResult<Vec<Screenshot>> {
    let Ok(entries) = fs::read_dir(folder(paths, id)?) else {
        return Ok(Vec::new());
    };
    let mut shots: Vec<Screenshot> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if !is_image(&name) || !entry.file_type().ok()?.is_file() {
                return None;
            }
            let meta = entry.metadata().ok()?;
            Some(Screenshot { name, bytes: meta.len(), modified_ms: modified_ms(&meta) })
        })
        .collect();
    shots.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms).then_with(|| b.name.cmp(&a.name)));
    shots.truncate(LIMIT);
    Ok(shots)
}

/// One screenshot, checked to be an image directly inside the folder.
pub fn file(paths: &AppPaths, id: &str, name: &str) -> AppResult<PathBuf> {
    let mut parts = Path::new(name).components();
    let single = matches!((parts.next(), parts.next()), (Some(Component::Normal(_)), None));
    let path = folder(paths, id)?.join(name);
    if !single || !is_image(name) || !path.is_file() {
        return Err(AppError::new("screenshot_missing", crate::core::i18n::pick("找不到这张截图。", "Screenshot not found.")));
    }
    Ok(path)
}

pub fn content_type(name: &str) -> &'static str {
    if name.to_ascii_lowercase().ends_with(".png") { "image/png" } else { "image/jpeg" }
}

fn thumbnail_dir(paths: &AppPaths, id: &str) -> PathBuf {
    paths.cache.join("thumbnails").join(id)
}

fn name_key(name: &str) -> String {
    hex::encode(&Sha1::digest(name.as_bytes())[..8])
}

/// A small PNG of the screenshot, cached under its size and time so a
/// replaced file gets a fresh one. JPEG files are served as they are.
pub fn thumbnail(paths: &AppPaths, id: &str, name: &str) -> AppResult<(Vec<u8>, &'static str)> {
    let source = file(paths, id, name)?;
    if content_type(name) != "image/png" {
        return Ok((fs::read(&source)?, "image/jpeg"));
    }
    let meta = fs::metadata(&source)?;
    let cache = thumbnail_dir(paths, id).join(format!(
        "{}-{}-{}.png",
        name_key(name),
        meta.len(),
        modified_ms(&meta)
    ));
    if let Ok(bytes) = fs::read(&cache) {
        return Ok((bytes, "image/png"));
    }
    match render_thumbnail(&source) {
        Ok(bytes) => {
            // A cache write that fails only means the next view renders again.
            if fs::create_dir_all(cache.parent().unwrap_or(Path::new("."))).is_ok() {
                let _ = fs::write(&cache, &bytes);
            }
            Ok((bytes, "image/png"))
        }
        // An image this decoder cannot read may still show in the page.
        Err(error) => {
            tracing::debug!(error = %error, name, "thumbnail failed; serving the original");
            Ok((fs::read(&source)?, "image/png"))
        }
    }
}

fn render_thumbnail(source: &Path) -> AppResult<Vec<u8>> {
    let fail = |detail: String| AppError::detail("thumbnail", crate::core::i18n::pick("无法读取截图。", "Couldn't read the screenshot."), detail);
    let mut decoder = png::Decoder::new_with_limits(
        BufReader::new(fs::File::open(source)?),
        png::Limits { bytes: MAX_FRAME_BYTES },
    );
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|error| fail(error.to_string()))?;
    let (width, height) = (reader.info().width, reader.info().height);
    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(fail(format!("image too large: {width}×{height}")));
    }
    let size = reader
        .output_buffer_size()
        .filter(|size| *size <= MAX_FRAME_BYTES)
        .ok_or_else(|| fail("image too large".into()))?;
    // Ask for the memory rather than abort the process when it is not there.
    let mut buffer = Vec::new();
    buffer.try_reserve_exact(size).map_err(|error| fail(error.to_string()))?;
    buffer.resize(size, 0);
    let frame = reader.next_frame(&mut buffer).map_err(|error| fail(error.to_string()))?;
    let channels = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(fail("indexed output".into())),
    };
    let (out_w, out_h, pixels) = downscale(
        &buffer,
        frame.width as usize,
        frame.height as usize,
        frame.line_size,
        channels,
        THUMB_WIDTH,
    );
    let mut bytes = Vec::with_capacity(pixels.len() / 3);
    {
        let mut encoder = png::Encoder::new(&mut bytes, out_w as u32, out_h as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().map_err(|error| fail(error.to_string()))?;
        writer.write_image_data(&pixels).map_err(|error| fail(error.to_string()))?;
    }
    Ok(bytes)
}

/// Box filter to at most `max_width`: each output pixel averages the source
/// pixels it covers. Returns RGB rows; alpha is dropped (screenshots are
/// opaque).
fn downscale(
    data: &[u8],
    width: usize,
    height: usize,
    line: usize,
    channels: usize,
    max_width: usize,
) -> (usize, usize, Vec<u8>) {
    let out_w = max_width.min(width).max(1);
    let out_h = ((height * out_w + width / 2) / width.max(1)).max(1);
    let mut out = vec![0u8; out_w * out_h * 3];
    for oy in 0..out_h {
        let y0 = oy * height / out_h;
        let y1 = ((oy + 1) * height / out_h).max(y0 + 1).min(height);
        for ox in 0..out_w {
            let x0 = ox * width / out_w;
            let x1 = ((ox + 1) * width / out_w).max(x0 + 1).min(width);
            let mut sum = [0u32; 3];
            let mut count = 0u32;
            for y in y0..y1 {
                let row = &data[y * line..];
                for x in x0..x1 {
                    let pixel = &row[x * channels..];
                    let (r, g, b) = if channels >= 3 {
                        (pixel[0], pixel[1], pixel[2])
                    } else {
                        (pixel[0], pixel[0], pixel[0])
                    };
                    sum[0] += r as u32;
                    sum[1] += g as u32;
                    sum[2] += b as u32;
                    count += 1;
                }
            }
            let at = (oy * out_w + ox) * 3;
            let count = count.max(1);
            out[at] = (sum[0] / count) as u8;
            out[at + 1] = (sum[1] / count) as u8;
            out[at + 2] = (sum[2] / count) as u8;
        }
    }
    (out_w, out_h, out)
}

/// Moves a screenshot to the launcher's recycle folder, and drops its
/// cached thumbnails.
pub fn recycle(paths: &AppPaths, id: &str, name: &str) -> AppResult<()> {
    let source = file(paths, id, name)?;
    let target_dir = paths.root.join("recycle").join("screenshots").join(id);
    fs::create_dir_all(&target_dir)?;
    let mut target = target_dir.join(name);
    if target.exists() {
        target = target_dir.join(format!("{}-{name}", crate::core::util::now_millis()));
    }
    if fs::rename(&source, &target).is_err() {
        // Across volumes (a moved data folder), copy then remove.
        fs::copy(&source, &target)?;
        fs::remove_file(&source)?;
    }
    let key = name_key(name);
    if let Ok(entries) = fs::read_dir(thumbnail_dir(paths, id)) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(&key) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_png(path: &Path, width: u32, height: u32, rgba: [u8; 4]) {
        let file = fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let data: Vec<u8> = (0..width * height).flat_map(|_| rgba).collect();
        writer.write_image_data(&data).unwrap();
    }

    #[test]
    fn lists_thumbnails_and_recycles_screenshots() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let shots = folder(&paths, &id).unwrap();
        fs::create_dir_all(&shots).unwrap();
        write_png(&shots.join("2026-10-28_20.15.03.png"), 1280, 720, [200, 100, 50, 255]);
        fs::write(shots.join("notes.txt"), "x").unwrap();
        fs::create_dir_all(shots.join("folder.png")).unwrap();

        let list = list(&paths, &id).unwrap();
        assert_eq!(list.len(), 1);
        let name = &list[0].name;

        let (bytes, kind) = thumbnail(&paths, &id, name).unwrap();
        assert_eq!(kind, "image/png");
        let decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((info.width, info.height), (480, 270));
        assert_eq!(&pixels[..3], &[200, 100, 50]);
        // The second view comes from the cache.
        assert_eq!(fs::read_dir(thumbnail_dir(&paths, &id)).unwrap().count(), 1);
        assert_eq!(thumbnail(&paths, &id, name).unwrap().0, bytes);

        assert!(file(&paths, &id, "../instance.json").is_err());
        assert!(file(&paths, &id, "notes.txt").is_err());

        recycle(&paths, &id, name).unwrap();
        assert!(super::list(&paths, &id).unwrap().is_empty());
        assert!(paths.root.join("recycle/screenshots").join(&id).join(name).is_file());
        assert_eq!(fs::read_dir(thumbnail_dir(&paths, &id)).unwrap().count(), 0);
    }

    /// A PNG chunk: length, type, data and CRC-32.
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut crc = !0u32;
        for byte in kind.iter().chain(data) {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&(!crc).to_be_bytes());
        out
    }

    #[test]
    fn oversized_headers_are_refused_before_allocating() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.png");
        // 65535×65535 RGBA: a 16 GiB frame from a file of a few bytes.
        let mut header = 65_535u32.to_be_bytes().to_vec();
        header.extend_from_slice(&65_535u32.to_be_bytes());
        header.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend(chunk(b"IHDR", &header));
        bytes.extend(chunk(b"IDAT", &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]));
        bytes.extend(chunk(b"IEND", &[]));
        fs::write(&path, bytes).unwrap();
        let error = format!("{:?}", render_thumbnail(&path).unwrap_err());
        assert!(error.contains("too large"), "{error}");

        // Within one limit but past the pixel budget: 16384×16384.
        let mut header = 16_384u32.to_be_bytes().to_vec();
        header.extend_from_slice(&16_384u32.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend(chunk(b"IHDR", &header));
        bytes.extend(chunk(b"IDAT", &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]));
        bytes.extend(chunk(b"IEND", &[]));
        fs::write(&path, bytes).unwrap();
        let error = format!("{:?}", render_thumbnail(&path).unwrap_err());
        assert!(error.contains("too large"), "{error}");
    }

    #[test]
    fn downscale_averages_and_keeps_the_aspect() {
        // 4×2 gray image: left half black, right half white.
        let data = [0, 0, 255, 255, 0, 0, 255, 255];
        let (w, h, out) = downscale(&data, 4, 2, 4, 1, 2);
        assert_eq!((w, h), (2, 1));
        assert_eq!(out, vec![0, 0, 0, 255, 255, 255]);
        // Never upscales.
        assert_eq!(downscale(&data, 4, 2, 4, 1, 100).0, 4);
    }
}
