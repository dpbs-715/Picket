use crate::i18n::{LanguageKind, deserialize_language_or_default};
use serde::{Deserialize, Serialize};

pub const UPDATE_PROTOCOL_VERSION: u8 = 1;
pub const RELEASE_PAGE_URL: &str = "https://github.com/dpbs-715/Picket/releases";
pub const UPDATE_STATE_EVENT: &str = "picket:update-state";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum UpdatePhase {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Installing,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum UpdateInstallBlocker {
    None,
    DevBuild,
    ReadOnlyVolume,
    AppTranslocation,
    SigningKeyUnconfigured,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct UpdateStateSnapshot {
    #[cfg_attr(test, ts(type = "1"))]
    pub protocol_version: u8,
    pub current_version: String,
    pub phase: UpdatePhase,
    pub release_page_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub downloaded_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub total_bytes: Option<u64>,
    pub install_blocker: UpdateInstallBlocker,
    pub can_install: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<String>,
}

impl UpdateStateSnapshot {
    pub fn initial(current_version: String, install_blocker: UpdateInstallBlocker) -> Self {
        Self {
            protocol_version: UPDATE_PROTOCOL_VERSION,
            current_version,
            phase: UpdatePhase::Idle,
            release_page_url: RELEASE_PAGE_URL.to_owned(),
            available_version: None,
            release_notes: None,
            published_at: None,
            downloaded_bytes: None,
            total_bytes: None,
            install_blocker,
            can_install: false,
            error: None,
            checked_at: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckUpdateRequest {
    #[serde(default)]
    pub manual: bool,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallUpdateRequest {
    pub version: String,
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelUpdateRequest {
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenReleasePageRequest {
    #[serde(default, deserialize_with = "deserialize_language_or_default")]
    #[cfg_attr(test, ts(as = "Option<LanguageKind>", optional))]
    pub locale: LanguageKind,
}
