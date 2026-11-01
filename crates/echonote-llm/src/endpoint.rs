//! Where requests go. The base URL includes the version path (`https://api.openai.com/v1`,
//! `https://api.anthropic.com/v1`, `http://localhost:11434/v1`). Plain http is allowed for this
//! computer, or for another host only when the user said so; credentials in the URL are
//! refused, so a URL can be shown or logged without leaking anything.

use std::fmt;

use url::{Host, Url};

use crate::LlmError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Api {
    /// `POST {base}/chat/completions` with a bearer key.
    OpenAi,
    /// `POST {base}/messages` with `x-api-key`.
    Anthropic,
}

#[derive(Clone)]
pub struct Endpoint {
    pub api: Api,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    /// Sent with every request after the auth headers, so they can replace them.
    pub headers: Vec<(String, String)>,
    /// `http(s)://host:port`; `None` uses the system proxy. Never used for this computer.
    pub proxy: Option<String>,
    /// Allow plain http to a host that is not this computer (a server on the LAN).
    pub allow_http: bool,
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: Vec<&str> = self.headers.iter().map(|(name, _)| name.as_str()).collect();
        f.debug_struct("Endpoint")
            .field("api", &self.api)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("headers", &headers)
            .finish_non_exhaustive()
    }
}

impl Endpoint {
    /// `path` under the base URL, after checking the base URL.
    pub fn url(&self, path: &str) -> Result<Url, LlmError> {
        let mut url = Url::parse(self.base_url.trim()).map_err(|_| LlmError::InvalidUrl)?;
        if url.host().is_none() || !url.username().is_empty() || url.password().is_some() {
            return Err(LlmError::InvalidUrl);
        }
        match url.scheme() {
            "https" => {}
            "http" if is_loopback(&url) || self.allow_http => {}
            "http" => return Err(LlmError::InsecureUrl),
            _ => return Err(LlmError::InvalidUrl),
        }
        let base = url.path().trim_end_matches('/').to_owned();
        url.set_path(&format!("{base}/{}", path.trim_start_matches('/')));
        url.set_fragment(None);
        Ok(url)
    }

    /// `scheme://host[:port]`, naming who receives the transcript (the consent prompt).
    pub fn origin(&self) -> Result<String, LlmError> {
        Ok(self.url("")?.origin().ascii_serialization())
    }

    /// The server runs on this computer (Ollama, LM Studio): no proxy, nothing leaves it.
    pub fn is_local(&self) -> bool {
        Url::parse(self.base_url.trim()).is_ok_and(|url| is_loopback(&url))
    }
}

/// Only `localhost` itself and loopback IPs: a `*.localhost` name is resolved like any other
/// (a resolver or proxy may send it off this computer).
fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(base_url: &str) -> Endpoint {
        Endpoint {
            api: Api::OpenAi,
            base_url: base_url.into(),
            model: "m".into(),
            api_key: Some("sk-secret".into()),
            headers: vec![("X-Org".into(), "org-secret".into())],
            proxy: None,
            allow_http: false,
        }
    }

    #[test]
    fn urls_are_checked_and_joined() {
        let url = endpoint("https://api.openai.com/v1/").url("chat/completions");
        assert_eq!(
            url.unwrap().as_str(),
            "https://api.openai.com/v1/chat/completions"
        );
        let azure = endpoint("https://x.openai.azure.com/openai/v1?api-version=preview");
        assert_eq!(
            azure.url("chat/completions").unwrap().as_str(),
            "https://x.openai.azure.com/openai/v1/chat/completions?api-version=preview"
        );
        assert!(endpoint("http://localhost:11434/v1").url("models").is_ok());
        assert!(endpoint("http://127.0.0.1:1234/v1").is_local());
        assert!(endpoint("http://[::1]:1234/v1").is_local());
        assert!(endpoint("http://LocalHost:11434/v1").is_local());
        assert!(endpoint("http://127.8.0.1:1234/v1").is_local());
        for remote in [
            "http://api.localhost:11434/v1",
            "http://localhost.example.com/v1",
            "http://[::2]:1234/v1",
        ] {
            let remote = endpoint(remote);
            assert!(!remote.is_local(), "{}", remote.base_url);
            assert!(matches!(remote.url("models"), Err(LlmError::InsecureUrl)));
        }
        assert_eq!(
            endpoint("https://API.example.com:8443/v1?x=1")
                .origin()
                .unwrap(),
            "https://api.example.com:8443"
        );
        assert_eq!(
            endpoint("https://api.deepseek.com").origin().unwrap(),
            "https://api.deepseek.com"
        );
        assert!(matches!(
            endpoint("http://192.168.1.5:11434/v1").url("models"),
            Err(LlmError::InsecureUrl)
        ));
        let lan = Endpoint {
            allow_http: true,
            ..endpoint("http://192.168.1.5:11434/v1")
        };
        assert!(lan.url("models").is_ok() && !lan.is_local());
        for bad in [
            "https://user:pw@api.x.com/v1",
            "ftp://x.com",
            "api.openai.com/v1",
            "",
        ] {
            assert!(
                matches!(endpoint(bad).url("models"), Err(LlmError::InvalidUrl)),
                "{bad}"
            );
        }
    }

    #[test]
    fn debug_output_hides_secrets() {
        let text = format!("{:?}", endpoint("https://api.openai.com/v1"));
        assert!(
            !text.contains("sk-secret") && !text.contains("org-secret"),
            "{text}"
        );
        assert!(text.contains("X-Org"));
    }
}
