//! Rust -> TypeScript protocol codegen and drift verification via `ts-rs`.
//!
//! - Running with `PICKET_TS_EXPORT_DIR=<dir>` writes all generated bindings and an `index.ts`
//!   barrel into `<dir>` (used by `pnpm protocol:sync` and `pnpm protocol:check`).
//! - Running plain `cargo test` exports into an isolated temporary directory and verifies
//!   byte-for-byte parity against `../shared/generated/` without mutating the working tree.

use crate::{
    actions::{
        message_user::{
            MessageBlock, MessageCalloutTone, MessageChecklistItem, MessageChecklistStatus,
            MessageFactItem, MessageFactTone, MessageFileChipItem, MessageFindingItem,
            MessageFindingSeverity, MessageQuestionContext, MessageQuestionItem,
            MessageQuestionOption, MessageSkippedFileItem, MessageTone, MessageUserAnswer,
            MessageUserCard, MessageUserRequest, MessageUserResponse,
        },
        session_store::ActionSessionSnapshot,
        types::{
            CPushCommandReply, CPushLandedCommit, CPushProgressEvent, CPushQaItem,
            CPushSessionSnapshot, CPushSessionStatus, CustomActionCommandReply, CustomActionDraft,
            CustomActionProgressEvent, DismissSessionDoneRequest,
            GenerateCustomActionProgressEvent, GenerateCustomActionReply,
            GenerateCustomActionRequest, PullRebaseRequest, ReconcileCPushRequest,
            RespondCPushRequest, RespondCustomActionRequest, StartCPushRequest,
            StartCustomActionRequest, TrustedActionUpdate, WorkflowLink,
            WorkflowMessageCommitSummary, WorkflowMessageFileChip, WorkflowMessageOption,
            WorkflowMessageProgress, WorkflowTimelineItem, WorkflowUiMessage,
        },
    },
    error::{AppError, AppErrorCode, ErrorCategory},
    i18n::LanguageKind,
    models::types::{
        ApiFormatKind, CreateProviderConnectionInput, ListModelsInput, ModelListReply,
        ModelMutateReply, ModelSaveReply, ModelTestReply, ProviderKind, ProviderRevealKeyOutcome,
        ProviderRevealKeyReply, ProviderSaveReply, ProviderUpdateReply, RemoteModelOption,
        SaveModelInput, SavedModelConfiguration, SavedProviderConnection, TestConnectionInput,
        UpdateProviderConnectionInput,
    },
    repository::types::{
        Branch, BranchAction, BranchReply, BranchRequest, Change, CommitSelection, EditorKind,
        LocalKeepReply, LocalKeepRequest, OpenFileAction, OpenFileReply, OpenFileRequest,
        OpenRequest, PersistedCustomAction, PersistedRepositorySettings,
        PersistedWindowPreferences, PersistedWorktreeState, Range, ReadReply, ReadTarget,
        RefreshRequest, Remote, RemoteBranch, RepositoryInfo, RepositorySnapshot, ThemeKind,
        WindowPosition, WindowSize, WorkbenchRestoreReply, WorkbenchSaveRequest, WorkbenchTab,
        WorktreeSnapshot,
    },
    skills::sync::{
        GetSkillsStateRequest, ResetSkillsRequest, SkillCatalogItem, SkillsStateSnapshot,
        SyncSkillsRequest, UpdateEnabledSkillsRequest,
    },
    tray::state::{
        TrayMainWindowAction, TrayRepositorySummary, TraySessionStatus, TrayStateSnapshot,
        TrayUiContextUpdate,
    },
    updater::types::{
        CancelUpdateRequest, CheckUpdateRequest, InstallUpdateRequest, OpenReleasePageRequest,
        UpdateInstallBlocker, UpdatePhase, UpdateStateSnapshot,
    },
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use ts_rs::{Config, TS};

fn export_all_bindings(dir: &Path) {
    fs::create_dir_all(dir).expect("Failed to create ts-rs export directory");
    let cfg = Config::from_env().with_out_dir(dir);

    AppError::export_all(&cfg).unwrap();
    AppErrorCode::export_all(&cfg).unwrap();
    ErrorCategory::export_all(&cfg).unwrap();
    LanguageKind::export_all(&cfg).unwrap();
    ThemeKind::export_all(&cfg).unwrap();
    EditorKind::export_all(&cfg).unwrap();
    WindowSize::export_all(&cfg).unwrap();
    WindowPosition::export_all(&cfg).unwrap();
    PersistedWindowPreferences::export_all(&cfg).unwrap();
    ReadTarget::export_all(&cfg).unwrap();
    OpenRequest::export_all(&cfg).unwrap();
    RefreshRequest::export_all(&cfg).unwrap();
    LocalKeepRequest::export_all(&cfg).unwrap();
    LocalKeepReply::export_all(&cfg).unwrap();
    BranchAction::export_all(&cfg).unwrap();
    BranchRequest::export_all(&cfg).unwrap();
    BranchReply::export_all(&cfg).unwrap();
    CommitSelection::export_all(&cfg).unwrap();
    OpenFileRequest::export_all(&cfg).unwrap();
    OpenFileAction::export_all(&cfg).unwrap();
    OpenFileReply::export_all(&cfg).unwrap();
    WorkbenchTab::export_all(&cfg).unwrap();
    PersistedCustomAction::export_all(&cfg).unwrap();
    PersistedRepositorySettings::export_all(&cfg).unwrap();
    PersistedWorktreeState::export_all(&cfg).unwrap();
    WorkbenchSaveRequest::export_all(&cfg).unwrap();
    WorkbenchRestoreReply::export_all(&cfg).unwrap();
    ReadReply::export_all(&cfg).unwrap();
    RepositorySnapshot::export_all(&cfg).unwrap();
    RepositoryInfo::export_all(&cfg).unwrap();
    Branch::export_all(&cfg).unwrap();
    RemoteBranch::export_all(&cfg).unwrap();
    WorktreeSnapshot::export_all(&cfg).unwrap();
    Remote::export_all(&cfg).unwrap();
    Change::export_all(&cfg).unwrap();
    Range::export_all(&cfg).unwrap();
    ApiFormatKind::export_all(&cfg).unwrap();
    ProviderKind::export_all(&cfg).unwrap();
    SavedProviderConnection::export_all(&cfg).unwrap();
    SavedModelConfiguration::export_all(&cfg).unwrap();
    CreateProviderConnectionInput::export_all(&cfg).unwrap();
    UpdateProviderConnectionInput::export_all(&cfg).unwrap();
    SaveModelInput::export_all(&cfg).unwrap();
    TestConnectionInput::export_all(&cfg).unwrap();
    ModelTestReply::export_all(&cfg).unwrap();
    RemoteModelOption::export_all(&cfg).unwrap();
    ListModelsInput::export_all(&cfg).unwrap();
    ModelListReply::export_all(&cfg).unwrap();
    ProviderSaveReply::export_all(&cfg).unwrap();
    ProviderUpdateReply::export_all(&cfg).unwrap();
    ModelSaveReply::export_all(&cfg).unwrap();
    ProviderRevealKeyOutcome::export_all(&cfg).unwrap();
    ProviderRevealKeyReply::export_all(&cfg).unwrap();
    ModelMutateReply::export_all(&cfg).unwrap();
    SkillCatalogItem::export_all(&cfg).unwrap();
    SkillsStateSnapshot::export_all(&cfg).unwrap();
    GetSkillsStateRequest::export_all(&cfg).unwrap();
    SyncSkillsRequest::export_all(&cfg).unwrap();
    UpdateEnabledSkillsRequest::export_all(&cfg).unwrap();
    ResetSkillsRequest::export_all(&cfg).unwrap();
    UpdatePhase::export_all(&cfg).unwrap();
    UpdateInstallBlocker::export_all(&cfg).unwrap();
    UpdateStateSnapshot::export_all(&cfg).unwrap();
    CheckUpdateRequest::export_all(&cfg).unwrap();
    InstallUpdateRequest::export_all(&cfg).unwrap();
    CancelUpdateRequest::export_all(&cfg).unwrap();
    OpenReleasePageRequest::export_all(&cfg).unwrap();
    TraySessionStatus::export_all(&cfg).unwrap();
    TrayRepositorySummary::export_all(&cfg).unwrap();
    TrayStateSnapshot::export_all(&cfg).unwrap();
    TrayUiContextUpdate::export_all(&cfg).unwrap();
    TrayMainWindowAction::export_all(&cfg).unwrap();
    MessageTone::export_all(&cfg).unwrap();
    MessageCalloutTone::export_all(&cfg).unwrap();
    MessageFactTone::export_all(&cfg).unwrap();
    MessageFindingSeverity::export_all(&cfg).unwrap();
    MessageChecklistStatus::export_all(&cfg).unwrap();
    MessageFactItem::export_all(&cfg).unwrap();
    MessageFindingItem::export_all(&cfg).unwrap();
    MessageSkippedFileItem::export_all(&cfg).unwrap();
    MessageChecklistItem::export_all(&cfg).unwrap();
    MessageFileChipItem::export_all(&cfg).unwrap();
    MessageBlock::export_all(&cfg).unwrap();
    MessageUserCard::export_all(&cfg).unwrap();
    MessageQuestionOption::export_all(&cfg).unwrap();
    MessageQuestionItem::export_all(&cfg).unwrap();
    MessageQuestionContext::export_all(&cfg).unwrap();
    MessageUserRequest::export_all(&cfg).unwrap();
    MessageUserAnswer::export_all(&cfg).unwrap();
    MessageUserResponse::export_all(&cfg).unwrap();
    CPushSessionStatus::export_all(&cfg).unwrap();
    CPushQaItem::export_all(&cfg).unwrap();
    CPushLandedCommit::export_all(&cfg).unwrap();
    WorkflowMessageOption::export_all(&cfg).unwrap();
    WorkflowLink::export_all(&cfg).unwrap();
    WorkflowTimelineItem::export_all(&cfg).unwrap();
    WorkflowMessageFileChip::export_all(&cfg).unwrap();
    WorkflowMessageCommitSummary::export_all(&cfg).unwrap();
    WorkflowMessageProgress::export_all(&cfg).unwrap();
    WorkflowUiMessage::export_all(&cfg).unwrap();
    TrustedActionUpdate::export_all(&cfg).unwrap();
    CustomActionDraft::export_all(&cfg).unwrap();
    StartCustomActionRequest::export_all(&cfg).unwrap();
    RespondCustomActionRequest::export_all(&cfg).unwrap();
    CustomActionProgressEvent::export_all(&cfg).unwrap();
    CustomActionCommandReply::export_all(&cfg).unwrap();
    GenerateCustomActionRequest::export_all(&cfg).unwrap();
    GenerateCustomActionProgressEvent::export_all(&cfg).unwrap();
    GenerateCustomActionReply::export_all(&cfg).unwrap();
    StartCPushRequest::export_all(&cfg).unwrap();
    PullRebaseRequest::export_all(&cfg).unwrap();
    ReconcileCPushRequest::export_all(&cfg).unwrap();
    RespondCPushRequest::export_all(&cfg).unwrap();
    CPushProgressEvent::export_all(&cfg).unwrap();
    CPushCommandReply::export_all(&cfg).unwrap();
    DismissSessionDoneRequest::export_all(&cfg).unwrap();
    CPushSessionSnapshot::export_all(&cfg).unwrap();
    ActionSessionSnapshot::export_all(&cfg).unwrap();

    // Generate deterministic barrel file for convenient imports:
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "index.ts" || !name.ends_with(".ts") {
                None
            } else {
                Some(name.trim_end_matches(".ts").to_owned())
            }
        })
        .collect();
    names.sort();
    let mut index_lines = vec![
        "// This file was generated by Rust `ts-rs` via `pnpm protocol:sync`. Do not edit manually."
            .to_owned(),
    ];
    for name in names {
        index_lines.push(format!("export type {{ {name} }} from './{name}';"));
    }
    index_lines.push(String::new());
    fs::write(dir.join("index.ts"), index_lines.join("\n")).unwrap();
}

fn read_ts_tree(dir: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if !dir.exists() {
        return out;
    }
    for entry in fs::read_dir(dir).unwrap().filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".ts") {
            let content = fs::read_to_string(entry.path()).unwrap();
            out.insert(name, content);
        }
    }
    out
}

#[test]
fn export_ts_protocol_bindings() {
    if let Ok(target) = std::env::var("PICKET_TS_EXPORT_DIR")
        && !target.trim().is_empty()
    {
        let out_dir = PathBuf::from(target);
        if out_dir.exists() {
            for entry in fs::read_dir(&out_dir).unwrap().filter_map(Result::ok) {
                if entry.file_name().to_string_lossy().ends_with(".ts") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        export_all_bindings(&out_dir);
        return;
    }

    // Default test mode: export into an isolated tempdir and verify parity against `shared/generated`.
    let temp = tempfile::tempdir().unwrap();
    export_all_bindings(temp.path());
    let expected = read_ts_tree(temp.path());
    let committed_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../shared/generated");
    let actual = read_ts_tree(&committed_dir);

    let expected_keys: Vec<&String> = expected.keys().collect();
    let actual_keys: Vec<&String> = actual.keys().collect();
    assert_eq!(
        actual_keys, expected_keys,
        "shared/generated file list drifted from Rust ts-rs types; run `pnpm protocol:sync`"
    );
    for (file, expected_content) in &expected {
        let actual_content = actual.get(file).map(String::as_str).unwrap_or("");
        assert_eq!(
            actual_content, expected_content,
            "shared/generated/{file} drifted from Rust ts-rs types; run `pnpm protocol:sync`"
        );
    }
}
