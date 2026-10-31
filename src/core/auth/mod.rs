pub mod local_skin;
pub mod microsoft;
pub mod offline;
pub mod yggdrasil;

use serde::{Deserialize, Serialize};

/// The values needed to build Minecraft launch arguments. Access tokens never
/// leave the Rust side.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftSession {
    pub username: String,
    /// Undashed UUID, as expected by the game arguments.
    pub uuid: String,
    pub access_token: String,
    pub xuid: Option<String>,
    /// "msa" for Microsoft accounts, "legacy" for offline accounts.
    pub user_type: String,
}

/// A skin PNG checked to be one the game accepts: 64×64, or the old 64×32.
pub fn check_skin(png: &[u8]) -> crate::core::error::AppResult<()> {
    let bad = || crate::core::error::AppError::new("skin_file", crate::core::i18n::pick("皮肤需要是 64×64（或旧版 64×32）的 PNG 图片。", "The skin must be a 64×64 (or old 64×32) PNG image."));
    if png.len() > 1024 * 1024 {
        return Err(bad());
    }
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let reader = decoder.read_info().map_err(|_| bad())?;
    let info = reader.info();
    if info.width == 64 && (info.height == 64 || info.height == 32) {
        Ok(())
    } else {
        Err(bad())
    }
}

/// A multipart form with text fields and the skin as `file`, built by hand
/// so no extra HTTP feature is needed. Returns (content type, body).
pub fn skin_form(fields: &[(&str, &str)], png: Vec<u8>) -> (String, Vec<u8>) {
    let boundary = format!("----islet{}", uuid::Uuid::new_v4().simple());
    let mut body = Vec::with_capacity(png.len() + 512);
    for (name, value) in fields {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"skin.png\"\r\nContent-Type: image/png\r\n\r\n").as_bytes());
    body.extend_from_slice(&png);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&vec![0u8; (width * height * 4) as usize]).unwrap();
        drop(writer);
        out
    }

    #[test]
    fn skins_are_checked_and_sent_as_a_form() {
        assert!(check_skin(&png(64, 64)).is_ok());
        assert!(check_skin(&png(64, 32)).is_ok());
        assert!(check_skin(&png(128, 128)).is_err());
        assert!(check_skin(b"not a png").is_err());
        let (kind, body) = skin_form(&[("variant", "slim")], vec![1, 2, 3]);
        let text = String::from_utf8_lossy(&body);
        assert!(kind.starts_with("multipart/form-data; boundary="));
        assert!(text.contains("name=\"variant\"\r\n\r\nslim\r\n"));
        assert!(text.contains("filename=\"skin.png\""));
    }
}
