//! LLM providers and AI options, stored as JSON under the library setting `ai`. API keys and
//! header values are in the keychain ([`crate::secrets`]); this holds only header names.

use std::collections::HashSet;

use echonote_core::store::Store;
use echonote_llm::summary::Budget;
use echonote_llm::{Api, Client, Endpoint, Policy, prompts};
use serde::{Deserialize, Serialize};

use crate::error::{CommandError, CommandResult};
use crate::secrets::Secret;

pub const KEY: &str = "ai";
pub const MAX_PROVIDERS: usize = 20;
const MAX_NAME: usize = 60;
const MAX_PRESET: usize = 40;
const MAX_URL: usize = 500;
const MAX_MODEL: usize = 200;
const MAX_KEY: usize = 4_096;
const MAX_HEADERS: usize = 10;
const MAX_HEADER_NAME: usize = 64;
const MAX_HEADER_VALUE: usize = 4_096;
pub const CONTEXT_TOKENS: std::ops::RangeInclusive<usize> = 2_048..=2_000_000;
pub const OUTPUT_TOKENS: std::ops::RangeInclusive<usize> = 256..=128_000;
pub const DEFAULT_TEMPLATE: &str = "general";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    /// The settings preset it started from (`openai`, `ollama`, `custom`…), for display.
    #[serde(default)]
    pub preset: String,
    pub api: Api,
    pub base_url: String,
    pub model: String,
    pub context_tokens: usize,
    pub max_output_tokens: usize,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub allow_http: bool,
    /// Names of the extra headers; their values are in the keychain.
    #[serde(default)]
    pub header_names: Vec<String>,
    /// An API key is saved in the keychain.
    #[serde(default)]
    pub has_key: bool,
    /// The server the user agreed to send transcripts to; a different one asks again.
    #[serde(default)]
    pub consented_origin: Option<String>,
}

impl Provider {
    pub fn endpoint(&self, secret: Secret) -> Endpoint {
        Endpoint {
            api: self.api,
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            api_key: secret.api_key,
            headers: secret.headers,
            proxy: self.proxy.clone(),
            allow_http: self.allow_http,
        }
    }

    pub fn client(&self, secret: Secret, policy: Policy) -> CommandResult<Client> {
        Ok(Client::new(self.endpoint(secret), policy)?)
    }

    pub fn budget(&self) -> Budget {
        Budget {
            context_tokens: self.context_tokens,
            max_output_tokens: self.max_output_tokens,
        }
    }

    /// The server runs on this computer, so nothing leaves it.
    pub fn is_local(&self) -> bool {
        self.endpoint(Secret::default()).is_local()
    }

    pub fn origin(&self) -> Option<String> {
        self.endpoint(Secret::default()).origin().ok()
    }

    /// Sending a transcript here needs the user's go-ahead first.
    pub fn needs_consent(&self) -> bool {
        !self.is_local()
            && (self.consented_origin.is_none() || self.consented_origin != self.origin())
    }

    /// Who wrote a summary or answer: `name / model`.
    pub fn label(&self) -> String {
        format!("{} / {}", self.name, self.model)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    pub providers: Vec<Provider>,
    /// The provider AI features use.
    pub active: Option<String>,
    /// Template for new notes: a built-in id or `custom:<id>`.
    pub template: String,
    /// Update highlights while recording. Off by default: it sends the transcript as it grows.
    pub live_highlights: bool,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            active: None,
            template: DEFAULT_TEMPLATE.into(),
            live_highlights: false,
        }
    }
}

impl AiSettings {
    /// Settings from the library, defaults for anything missing or unreadable.
    pub fn load(store: &Store) -> Self {
        store
            .setting(KEY)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<Self>(&json).ok())
            .map(Self::sanitized)
            .unwrap_or_default()
    }

    fn sanitized(mut self) -> Self {
        let mut ids = HashSet::new();
        self.providers
            .retain(|provider| !provider.id.is_empty() && ids.insert(provider.id.clone()));
        self.providers.truncate(MAX_PROVIDERS);
        for provider in &mut self.providers {
            provider.context_tokens = provider
                .context_tokens
                .clamp(*CONTEXT_TOKENS.start(), *CONTEXT_TOKENS.end());
            provider.max_output_tokens = provider
                .max_output_tokens
                .clamp(*OUTPUT_TOKENS.start(), *OUTPUT_TOKENS.end());
        }
        if self.active.as_ref().is_some_and(|id| !ids.contains(id)) {
            self.active = None;
        }
        if !valid_template(&self.template) {
            self.template = DEFAULT_TEMPLATE.into();
        }
        self
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("AI settings serialize")
    }

    pub fn provider(&self, id: &str) -> CommandResult<&Provider> {
        self.providers
            .iter()
            .find(|provider| provider.id == id)
            .ok_or_else(|| CommandError::new("not-found", "no such provider"))
    }

    pub fn active(&self) -> CommandResult<&Provider> {
        match &self.active {
            Some(id) => self.provider(id),
            None => Err(CommandError::new(
                "no-provider",
                "no LLM provider is set up",
            )),
        }
    }

    /// An id no provider has.
    pub fn new_id(&self, now_ms: i64) -> String {
        let mut n = now_ms.max(0);
        loop {
            let id = format!("p{n:x}");
            if self.provider(&id).is_err() {
                return id;
            }
            n += 1;
        }
    }
}

/// A built-in template id or `custom:<id>`.
pub fn valid_template(template: &str) -> bool {
    prompts::BUILTIN_TEMPLATES.contains(&template)
        || template
            .strip_prefix("custom:")
            .is_some_and(|id| id.parse::<i64>().is_ok_and(|id| id > 0))
}

/// A provider as the settings form sends it. Not `Debug`: it can hold a key.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub preset: String,
    pub api: Api,
    pub base_url: String,
    pub model: String,
    pub context_tokens: usize,
    pub max_output_tokens: usize,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub allow_http: bool,
    /// `None` keeps the saved key, empty removes it.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Extra headers; an empty value keeps that header's saved value.
    #[serde(default)]
    pub headers: Vec<(String, String)>,
}

fn visible(text: &str) -> bool {
    text.bytes().all(|byte| (0x20..0x7f).contains(&byte))
}

fn is_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn too_long(text: &str, max: usize) -> bool {
    text.chars().count() > max
}

fn one_line(text: &str, max: usize) -> bool {
    !text.is_empty() && !too_long(text, max) && !text.chars().any(char::is_control)
}

impl ProviderInput {
    /// The provider and its secret, checked. Empty secret fields keep what `saved` has.
    pub fn resolve(self, saved: Option<(&Provider, &Secret)>) -> CommandResult<(Provider, Secret)> {
        let name = self.name.trim().to_owned();
        let base_url = self.base_url.trim().to_owned();
        let model = self.model.trim().to_owned();
        let preset = self.preset.trim().to_owned();
        let proxy = self
            .proxy
            .map(|proxy| proxy.trim().to_owned())
            .filter(|proxy| !proxy.is_empty());
        if !one_line(&name, MAX_NAME) {
            return Err(CommandError::invalid(
                "the provider name is empty or too long",
            ));
        }
        if !one_line(&base_url, MAX_URL) || proxy.as_ref().is_some_and(|p| !one_line(p, MAX_URL)) {
            return Err(CommandError::invalid("an address is empty or too long"));
        }
        if !one_line(&model, MAX_MODEL) {
            return Err(CommandError::invalid("the model is empty or too long"));
        }
        if too_long(&preset, MAX_PRESET) || preset.chars().any(char::is_control) {
            return Err(CommandError::invalid("bad preset"));
        }
        if !CONTEXT_TOKENS.contains(&self.context_tokens)
            || !OUTPUT_TOKENS.contains(&self.max_output_tokens)
        {
            return Err(CommandError::invalid("token limits are out of range"));
        }
        if self.headers.len() > MAX_HEADERS {
            return Err(CommandError::invalid("too many headers"));
        }
        let saved_secret = saved.map(|(_, secret)| secret.clone()).unwrap_or_default();
        let api_key = match self.api_key.map(|key| key.trim().to_owned()) {
            None => saved_secret.api_key.clone(),
            Some(key) if key.is_empty() => None,
            Some(key) if key.len() > MAX_KEY || !visible(&key) => {
                return Err(CommandError::invalid(
                    "the API key has characters keys never have",
                ));
            }
            Some(key) => Some(key),
        };
        let mut headers: Vec<(String, String)> = Vec::new();
        for (name, value) in self.headers {
            let name = name.trim().to_owned();
            if name.is_empty() || name.len() > MAX_HEADER_NAME || !name.bytes().all(is_token) {
                return Err(CommandError::invalid("a header name is not valid"));
            }
            if headers
                .iter()
                .any(|(other, _)| other.eq_ignore_ascii_case(&name))
            {
                return Err(CommandError::invalid("a header is listed twice"));
            }
            let value = match value.trim() {
                "" => saved_secret
                    .headers
                    .iter()
                    .find(|(saved, _)| saved.eq_ignore_ascii_case(&name))
                    .map(|(_, value)| value.clone())
                    .ok_or_else(|| CommandError::invalid("a header has no value"))?,
                value => value.to_owned(),
            };
            if value.len() > MAX_HEADER_VALUE || !visible(&value) {
                return Err(CommandError::invalid("a header value is not valid"));
            }
            headers.push((name, value));
        }
        let provider = Provider {
            id: self.id.unwrap_or_default(),
            name,
            preset,
            api: self.api,
            base_url,
            model,
            context_tokens: self.context_tokens,
            max_output_tokens: self.max_output_tokens,
            proxy,
            allow_http: self.allow_http,
            header_names: headers.iter().map(|(name, _)| name.clone()).collect(),
            has_key: api_key.is_some(),
            consented_origin: saved.and_then(|(provider, _)| provider.consented_origin.clone()),
        };
        let secret = Secret { api_key, headers };
        // The client checks the address and proxy the way requests will use them.
        provider.client(secret.clone(), Policy::default())?;
        Ok((provider, secret))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(base_url: &str) -> ProviderInput {
        ProviderInput {
            id: Some("p1".into()),
            name: " DeepSeek ".into(),
            preset: "deepseek".into(),
            api: Api::OpenAi,
            base_url: base_url.into(),
            model: "deepseek-chat".into(),
            context_tokens: 64_000,
            max_output_tokens: 4_096,
            proxy: Some("  ".into()),
            allow_http: false,
            api_key: Some("sk-new".into()),
            headers: vec![("X-Org".into(), "acme".into())],
        }
    }

    #[test]
    fn secrets_are_kept_replaced_or_removed() {
        let (provider, secret) = input("https://api.deepseek.com").resolve(None).unwrap();
        assert_eq!(provider.name, "DeepSeek");
        assert_eq!(provider.proxy, None);
        assert!(provider.has_key);
        assert_eq!(provider.header_names, ["X-Org"]);
        assert_eq!(secret.api_key.as_deref(), Some("sk-new"));

        // Blank fields in the form keep what is saved.
        let keep = ProviderInput {
            api_key: None,
            headers: vec![("x-org".into(), String::new())],
            ..input("https://api.deepseek.com")
        };
        let (_, kept) = keep.resolve(Some((&provider, &secret))).unwrap();
        assert!(
            kept == Secret {
                api_key: Some("sk-new".into()),
                headers: vec![("x-org".into(), "acme".into())]
            }
        );

        let remove = ProviderInput {
            api_key: Some(String::new()),
            headers: Vec::new(),
            ..input("https://api.deepseek.com")
        };
        let (removed, secret) = remove.resolve(Some((&provider, &secret))).unwrap();
        assert!(!removed.has_key && secret.is_empty());
    }

    #[test]
    fn bad_input_is_rejected() {
        let code = |input: ProviderInput| input.resolve(None).err().map(|error| error.code);
        assert_eq!(
            code(input("http://example.com/v1")),
            Some("llm-insecure-url")
        );
        assert_eq!(code(input("ftp://example.com")), Some("llm-invalid-url"));
        assert_eq!(
            code(input("https://user:pw@example.com")),
            Some("llm-invalid-url")
        );
        let lan = ProviderInput {
            allow_http: true,
            ..input("http://192.168.1.5:8000/v1")
        };
        assert!(lan.resolve(None).is_ok());
        let bad_header = ProviderInput {
            headers: vec![("X Org".into(), "a".into())],
            ..input("https://api.deepseek.com")
        };
        assert_eq!(code(bad_header), Some("invalid"));
        let twice = ProviderInput {
            headers: vec![("A".into(), "1".into()), ("a".into(), "2".into())],
            ..input("https://api.deepseek.com")
        };
        assert_eq!(code(twice), Some("invalid"));
        let missing = ProviderInput {
            headers: vec![("A".into(), String::new())],
            ..input("https://api.deepseek.com")
        };
        assert_eq!(code(missing), Some("invalid"));
        let newline = ProviderInput {
            api_key: Some("sk-1\nX-Evil: 1".into()),
            ..input("https://api.deepseek.com")
        };
        assert_eq!(code(newline), Some("invalid"));
        let tiny = ProviderInput {
            context_tokens: 100,
            ..input("https://api.deepseek.com")
        };
        assert_eq!(code(tiny), Some("invalid"));
        let proxy = ProviderInput {
            proxy: Some("socks5://127.0.0.1:1080".into()),
            ..input("https://api.deepseek.com")
        };
        assert_eq!(code(proxy), Some("llm-invalid-proxy"));
    }

    #[test]
    fn consent_follows_the_server() {
        let (mut provider, _) = input("https://api.deepseek.com/v1").resolve(None).unwrap();
        assert!(provider.needs_consent());
        provider.consented_origin = provider.origin();
        assert!(!provider.needs_consent());
        provider.base_url = "https://api.deepseek.com/beta".into();
        assert!(!provider.needs_consent(), "same server, other path");
        provider.base_url = "https://evil.example.com/v1".into();
        assert!(provider.needs_consent(), "another server asks again");
        provider.base_url = "http://localhost:11434/v1".into();
        provider.consented_origin = None;
        assert!(!provider.needs_consent(), "nothing leaves this computer");
    }

    #[test]
    fn loading_repairs_what_it_can() {
        let (provider, _) = input("https://api.deepseek.com").resolve(None).unwrap();
        let settings = AiSettings {
            providers: vec![
                Provider {
                    context_tokens: 1,
                    ..provider.clone()
                },
                provider.clone(),
                Provider {
                    id: String::new(),
                    ..provider.clone()
                },
            ],
            active: Some("gone".into()),
            template: "custom:x".into(),
            live_highlights: true,
        };
        let clean = settings.sanitized();
        assert_eq!(clean.providers.len(), 1);
        assert_eq!(clean.providers[0].context_tokens, *CONTEXT_TOKENS.start());
        assert_eq!(clean.active, None);
        assert_eq!(clean.template, DEFAULT_TEMPLATE);
        assert!(
            valid_template("custom:3") && valid_template("weekly") && !valid_template("custom:0")
        );
        let partial: AiSettings = serde_json::from_str(r#"{"liveHighlights":true}"#).unwrap();
        assert!(partial.live_highlights && partial.providers.is_empty());
        assert_eq!(clean.new_id(255), "pff");
    }
}
