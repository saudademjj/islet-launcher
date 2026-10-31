//! The proxy every HTTP client of the launcher goes through: the system's
//! (macOS and Windows proxy settings, or the usual environment variables),
//! none, or one the user typed in.
use std::sync::RwLock;

use crate::core::error::{AppError, AppResult};
use crate::core::settings::{ProxyMode, Settings};

#[derive(Clone)]
enum Route {
    System,
    Direct,
    Via(reqwest::Proxy),
}

static ROUTE: RwLock<Route> = RwLock::new(Route::System);

fn route(settings: &Settings) -> AppResult<Route> {
    Ok(match settings.proxy_mode {
        ProxyMode::System => Route::System,
        ProxyMode::None => Route::Direct,
        ProxyMode::Custom => Route::Via(parse(&settings.proxy_url)?),
    })
}

/// Checks the settings' proxy without applying it.
pub fn check(settings: &Settings) -> AppResult<()> {
    route(settings).map(|_| ())
}

/// Applies the settings' proxy to clients built from now on.
pub fn configure(settings: &Settings) -> AppResult<()> {
    let route = route(settings)?;
    *ROUTE.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = route;
    Ok(())
}

/// A client builder with the launcher's user agent and proxy.
pub fn builder() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder().user_agent(concat!("Islet/", env!("CARGO_PKG_VERSION")));
    match ROUTE.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone() {
        Route::System => builder,
        Route::Direct => builder.no_proxy(),
        Route::Via(proxy) => builder.proxy(proxy),
    }
}

/// `http://`, `https://`, `socks5://` or `socks5h://`, with a host and port.
fn parse(text: &str) -> AppResult<reqwest::Proxy> {
    let invalid = || {
        AppError::new(
            "proxy",
            crate::core::i18n::pick(
                "代理地址无效，应类似 http://127.0.0.1:7890 或 socks5://127.0.0.1:7891。",
                "Invalid proxy address; it should look like http://127.0.0.1:7890 or socks5://127.0.0.1:7891.",
            ),
        )
    };
    let url = reqwest::Url::parse(text.trim()).map_err(|_| invalid())?;
    let known = ["http", "https", "socks5", "socks5h"].contains(&url.scheme());
    if !known || url.host_str().is_none_or(str::is_empty) || url.port_or_known_default().is_none() {
        return Err(invalid());
    }
    reqwest::Proxy::all(url.as_str()).map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_addresses_need_a_known_scheme_and_a_port() {
        assert!(parse("http://127.0.0.1:7890").is_ok());
        assert!(parse(" socks5://127.0.0.1:7891 ").is_ok());
        assert!(parse("http://proxy.example.com").is_ok());
        assert!(parse("socks5://127.0.0.1").is_err());
        assert!(parse("ftp://127.0.0.1:21").is_err());
        assert!(parse("127.0.0.1:7890").is_err());
        assert!(parse("").is_err());
    }
}
