//! AI features: LLM providers, notes, Q&A, correction and live highlights. Requests go out
//! from Rust only; the WebView never sees a key. Logs hold job kinds and error codes, never
//! addresses, prompts or answers.

pub mod config;
pub mod jobs;
pub mod live;

use std::time::Duration;

use echonote_llm::{Cancel, Policy, prompts};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::commands::blocking;
use crate::error::CommandResult;
use crate::log::log;
use crate::secrets::Secret;
use crate::state::AppState;
use config::{AiSettings, MAX_PROVIDERS, Provider, ProviderInput, valid_template};

/// A connection test or model list: one quick attempt.
fn probe_policy() -> Policy {
    Policy {
        attempts: 1,
        idle_timeout: Duration::from_secs(30),
        ..Policy::default()
    }
}

const MAX_MODELS: usize = 500;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    #[serde(flatten)]
    provider: Provider,
    local: bool,
    origin: Option<String>,
    needs_consent: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateView {
    id: i64,
    name: String,
    instructions: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiView {
    providers: Vec<ProviderView>,
    active: Option<String>,
    template: String,
    live_highlights: bool,
    builtin_templates: &'static [&'static str],
    templates: Vec<TemplateView>,
}

fn view(state: &AppState) -> CommandResult<AiView> {
    let settings = state.ai();
    let templates = state.read(|store| store.templates())?;
    Ok(AiView {
        providers: settings
            .providers
            .into_iter()
            .map(|provider| ProviderView {
                local: provider.is_local(),
                origin: provider.origin(),
                needs_consent: provider.needs_consent(),
                provider,
            })
            .collect(),
        active: settings.active,
        template: settings.template,
        live_highlights: settings.live_highlights,
        builtin_templates: &prompts::BUILTIN_TEMPLATES,
        templates: templates
            .into_iter()
            .map(|template| TemplateView {
                id: template.id,
                name: template.name,
                instructions: template.instructions,
            })
            .collect(),
    })
}

#[tauri::command]
pub async fn get_ai(state: State<'_, AppState>) -> CommandResult<AiView> {
    view(&state)
}

/// The provider an input edits and its saved secret (`None` when not read). The keychain is
/// read only when the form left a secret blank, so saving a new key never waits for an
/// access prompt.
fn saved(
    state: &AppState,
    input: &ProviderInput,
) -> CommandResult<Option<(Provider, Option<Secret>)>> {
    let Some(id) = &input.id else {
        return Ok(None);
    };
    let provider = state.ai().provider(id)?.clone();
    let blank = input.api_key.is_none()
        || input
            .headers
            .iter()
            .any(|(_, value)| value.trim().is_empty());
    let secret = if !provider.has_key && provider.header_names.is_empty() {
        Some(Secret::default())
    } else if blank {
        Some(state.secrets.get(&provider.id)?)
    } else {
        None
    };
    Ok(Some((provider, secret)))
}

/// The checked provider, its secret, and whether the keychain needs writing.
fn resolve(state: &AppState, input: ProviderInput) -> CommandResult<(Provider, Secret, bool)> {
    let saved = saved(state, &input)?;
    let none = Secret::default();
    let (provider, secret) = input.resolve(
        saved
            .as_ref()
            .map(|(provider, secret)| (provider, secret.as_ref().unwrap_or(&none))),
    )?;
    let changed = match &saved {
        None => !secret.is_empty(),
        Some((_, Some(old))) => *old != secret,
        Some((_, None)) => true,
    };
    Ok((provider, secret, changed))
}

/// Add or update a provider; the first one becomes active.
#[tauri::command]
pub async fn save_provider(app: AppHandle, provider: ProviderInput) -> CommandResult<AiView> {
    blocking(app, move |state| {
        let (mut provider, secret, secret_changed) = resolve(state, provider)?;
        let settings = state.ai();
        if provider.id.is_empty() {
            if settings.providers.len() >= MAX_PROVIDERS {
                return Err(crate::error::CommandError::new(
                    "too-many-providers",
                    "there are too many providers",
                ));
            }
            provider.id = settings.new_id(now_ms());
        }
        // The keychain first: a failure there leaves the settings as they were.
        if secret_changed {
            state.secrets.set(&provider.id, &secret)?;
        }
        state.update_ai(move |settings| {
            match settings.providers.iter_mut().find(|p| p.id == provider.id) {
                Some(existing) => *existing = provider,
                None => {
                    if settings.active.is_none() {
                        settings.active = Some(provider.id.clone());
                    }
                    settings.providers.push(provider);
                }
            }
            Ok(())
        })?;
        log!("saved an LLM provider");
        view(state)
    })
    .await
}

#[tauri::command]
pub async fn delete_provider(app: AppHandle, id: String) -> CommandResult<AiView> {
    blocking(app, move |state| {
        state.ai().provider(&id)?;
        state.secrets.delete(&id)?;
        state.update_ai(|settings| {
            settings.providers.retain(|provider| provider.id != id);
            if settings.active.as_deref() == Some(id.as_str()) {
                settings.active = settings.providers.first().map(|p| p.id.clone());
                settings.live_highlights = false;
            }
            Ok(())
        })?;
        log!("deleted an LLM provider");
        view(state)
    })
    .await
}

/// The active provider, the default template and live highlights.
#[tauri::command]
pub async fn set_ai_options(
    state: State<'_, AppState>,
    active: Option<String>,
    template: String,
    live_highlights: bool,
) -> CommandResult<AiView> {
    if !valid_template(&template) {
        return Err(crate::error::CommandError::invalid("unknown template"));
    }
    state.update_ai(|settings: &mut AiSettings| {
        if let Some(id) = &active {
            settings.provider(id)?;
        }
        settings.active = active;
        settings.template = template;
        settings.live_highlights = live_highlights;
        Ok(())
    })?;
    view(&state)
}

/// The user agreed to send transcripts to this provider's server.
#[tauri::command]
pub async fn consent_provider(state: State<'_, AppState>, id: String) -> CommandResult<AiView> {
    state.update_ai(|settings| {
        let provider = settings
            .providers
            .iter_mut()
            .find(|provider| provider.id == id)
            .ok_or_else(|| crate::error::CommandError::new("not-found", "no such provider"))?;
        provider.consented_origin = provider.origin();
        Ok(())
    })?;
    view(&state)
}

/// Send a tiny request with the form as it is (saved or not). No transcript is sent.
#[tauri::command]
pub async fn test_provider(app: AppHandle, provider: ProviderInput) -> CommandResult<()> {
    let client = blocking(app, move |state| {
        let (provider, secret, _) = resolve(state, provider)?;
        provider.client(secret, probe_policy())
    })
    .await?;
    let result = client.test(&Cancel::new()).await;
    match &result {
        Ok(()) => log!("provider test passed"),
        Err(error) => log!("provider test failed: {}", error.code()),
    }
    Ok(result?)
}

/// Model ids the server offers, for the form's suggestions.
#[tauri::command]
pub async fn list_models(app: AppHandle, provider: ProviderInput) -> CommandResult<Vec<String>> {
    let client = blocking(app, move |state| {
        let (provider, secret, _) = resolve(state, provider)?;
        provider.client(secret, probe_policy())
    })
    .await?;
    let mut models = client.models(&Cancel::new()).await.inspect_err(|error| {
        log!("model list failed: {}", error.code());
    })?;
    models.truncate(MAX_MODELS);
    Ok(models)
}

/// Add (`id` None) or replace a custom template.
#[tauri::command]
pub async fn save_template(
    state: State<'_, AppState>,
    id: Option<i64>,
    name: String,
    instructions: String,
) -> CommandResult<AiView> {
    state
        .engine()?
        .store()
        .call(move |store| store.save_template(id, &name, &instructions))?;
    view(&state)
}

#[tauri::command]
pub async fn delete_template(state: State<'_, AppState>, id: i64) -> CommandResult<AiView> {
    state
        .engine()?
        .store()
        .call(move |store| store.delete_template(id))?;
    let custom = format!("custom:{id}");
    state.update_ai(|settings| {
        if settings.template == custom {
            settings.template = config::DEFAULT_TEMPLATE.into();
        }
        Ok(())
    })?;
    view(&state)
}
