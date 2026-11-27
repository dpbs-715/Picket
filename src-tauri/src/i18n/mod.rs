//! Centralized user-facing localization for the Rust desktop layer.
//! Files inside `src-tauri/src/i18n/` are the ONLY Rust source files permitted to contain
//! Chinese characters (`[\p{Han}]`).

pub mod actions;
pub mod cpush;
pub mod desktop;
pub mod models;
pub mod pi;
pub mod repository;
pub mod skills;
pub mod updater;

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum LanguageKind {
    #[default]
    En,
    Zh,
}

pub fn deserialize_language_or_default<'de, D>(deserializer: D) -> Result<LanguageKind, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<LanguageKind>::deserialize(deserializer)?.unwrap_or_default())
}

pub const fn tr(locale: LanguageKind, en: &'static str, zh: &'static str) -> &'static str {
    match locale {
        LanguageKind::En => en,
        LanguageKind::Zh => zh,
    }
}

pub fn join_list<S: AsRef<str>>(locale: LanguageKind, items: &[S]) -> String {
    let sep = tr(locale, ", ", "、");
    items
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join(sep)
}
