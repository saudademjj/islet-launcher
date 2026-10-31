//! A per-game loopback-only skin API. No credentials or external listeners.
use crate::core::{
    accounts::Account,
    error::{AppError, AppResult},
    paths::AppPaths,
};
use base64::Engine;
use rsa::pkcs8::EncodePublicKey;
use sha1_compat::Digest;
use std::io::Read;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub fn save(
    paths: &AppPaths,
    account: &mut Account,
    png: Option<Vec<u8>>,
    slim: bool,
) -> AppResult<()> {
    let folder = paths.root.join("offline-skins");
    let file = folder.join(format!("{}.png", account.uuid));
    if let Some(png) = png {
        super::check_skin(&png)?;
        crate::core::util::atomic_write(&file, &png)?;
        account.skin_url = Some(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png)
        ));
    } else {
        if file.is_file() {
            std::fs::remove_file(file)?;
        }
        account.skin_url = None;
    }
    account.skin_slim = slim;
    Ok(())
}

pub struct SkinServer {
    pub url: String,
    stop: Arc<AtomicBool>,
}
impl Drop for SkinServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
impl SkinServer {
    pub fn start(paths: &AppPaths, account: &Account) -> AppResult<Self> {
        AppPaths::validate_instance_id(&account.uuid)?;
        let png = std::fs::read(
            paths
                .root
                .join("offline-skins")
                .join(format!("{}.png", account.uuid)),
        )?;
        let signing_key = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048)
            .map_err(|e| AppError::new("skin", e.to_string()))?;
        let public_key = rsa::RsaPublicKey::from(&signing_key)
            .to_public_key_pem(rsa::pkcs8::LineEnding::LF)
            .map_err(|e| AppError::new("skin", e.to_string()))?;
        let server = tiny_http::Server::http("127.0.0.1:0").map_err(|e| {
            AppError::detail(
                "skin",
                "Couldn't start the local skin server.",
                e.to_string(),
            )
        })?;
        let url = format!("http://{}", server.server_addr());
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let api = url.clone();
        let account = account.clone();
        std::thread::Builder::new().name("offline-skin".into()).spawn(move || {
            while !done.load(Ordering::Relaxed) {
                let Ok(Some(mut req)) = server.recv_timeout(Duration::from_millis(200)) else { continue };
                let path = req.url().split('?').next().unwrap_or("").to_string();
                if req.body_length().unwrap_or(0) > 8192 { let _ = req.respond(tiny_http::Response::empty(413)); continue; }
                let (body, kind, status) = if path == "/skin.png" {
                    (png.clone(), "image/png", 200)
                } else if path == "/" {
                    (serde_json::to_vec(&serde_json::json!({ "meta": { "serverName": "Local offline skin", "feature.no_mojang_namespace": true }, "skinDomains": ["127.0.0.1"], "signaturePublickey": public_key })).unwrap(), "application/json", 200)
                } else if path == format!("/sessionserver/session/minecraft/profile/{}", account.uuid.replace('-', "")) {
                    let texture = serde_json::json!({ "timestamp": crate::core::util::now_millis(), "profileId": account.uuid.replace('-', ""), "profileName": account.username, "signatureRequired": true, "textures": { "SKIN": { "url": format!("{api}/skin.png"), "metadata": { "model": if account.skin_slim { "slim" } else { "default" } } } } });
                    let value = base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&texture).unwrap());
                    let signature = signing_key.sign(rsa::Pkcs1v15Sign::new::<sha1_compat::Sha1>(), &sha1_compat::Sha1::digest(value.as_bytes())).expect("SHA-1 digest length is fixed");
                    let signature = base64::engine::general_purpose::STANDARD.encode(signature);
                    (serde_json::to_vec(&serde_json::json!({ "id": account.uuid.replace('-', ""), "name": account.username, "properties": [{ "name": "textures", "value": value, "signature": signature }] })).unwrap(), "application/json", 200)
                } else if path == "/api/profiles/minecraft" && req.method() == &tiny_http::Method::Post {
                    let mut body = String::new();
                    let _ = std::io::Read::read_to_string(&mut req.as_reader().take(8192), &mut body);
                    let names: Vec<String> = serde_json::from_str(&body).unwrap_or_default();
                    let found = if names.iter().any(|n| n.eq_ignore_ascii_case(&account.username)) { vec![serde_json::json!({ "id": account.uuid.replace('-', ""), "name": account.username })] } else { vec![] };
                    (serde_json::to_vec(&found).unwrap(), "application/json", 200)
                } else { (b"{}".to_vec(), "application/json", 404) };
                let response = tiny_http::Response::from_data(body).with_status_code(status).with_header(tiny_http::Header::from_bytes("Content-Type", kind).unwrap());
                let _ = req.respond(response);
            }
        })?;
        Ok(Self { url, stop })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loopback_serves_only_selected_profile_and_texture() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf()).unwrap();
        let account = super::super::offline::create_account("LocalSkin").unwrap();
        let file = paths
            .root
            .join("offline-skins")
            .join(format!("{}.png", account.uuid));
        crate::core::util::atomic_write(&file, b"png-fixture").unwrap();
        let server = SkinServer::start(&paths, &account).unwrap();
        let mut stream =
            std::net::TcpStream::connect(server.url.trim_start_matches("http://")).unwrap();
        use std::io::{Read, Write};
        write!(
            stream,
            "GET /sessionserver/session/minecraft/profile/{} HTTP/1.0\r\nHost: localhost\r\n\r\n",
            account.uuid.replace('-', "")
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.contains("textures"));
        assert!(response.contains("LocalSkin"));
    }
}
