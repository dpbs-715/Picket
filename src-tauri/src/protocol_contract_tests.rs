//! Cross-language protocol contract tests verifying that Rust structs serialize to and
//! deserialize from the exact golden JSON fixtures validated by Vitest in
//! `shared/protocol-contracts.test.ts`.

use crate::{
    actions::{
        message_user::MessageBlock,
        session_store::ActionSessionSnapshot,
        types::{
            CPushLandedCommit, CPushMessageFileChip, CPushSessionSnapshot, CPushSessionStatus,
            CPushTimelineItem, CPushUiMessage, CustomActionLink, CustomActionMessageCommitSummary,
            CustomActionUiMessage, MessageQuestionItem, MessageQuestionOption, MessageTone,
            MessageUserCard, MessageUserRequest,
        },
    },
    i18n::LanguageKind,
    repository::types::{
        Branch, Change, EditorKind, Range, ReadReply, ReadTarget, Remote, RemoteBranch,
        RepositoryInfo, RepositorySnapshot, ThemeKind, WorkbenchSaveRequest, WorktreeSnapshot,
    },
    tray::{TraySessionStatus, TrayStateSnapshot},
};
use serde_json::Value;

const CPUSH_SESSION_FIXTURE: &str = include_str!("../../shared/fixtures/cpush-session.json");
const ACTION_SESSION_FIXTURE: &str = include_str!("../../shared/fixtures/action-session.json");
const REPO_SNAPSHOT_FIXTURE: &str = include_str!("../../shared/fixtures/repository-snapshot.json");
const TRAY_STATE_FIXTURE: &str = include_str!("../../shared/fixtures/tray-state.json");
const WORKBENCH_SAVE_FIXTURE: &str = include_str!("../../shared/fixtures/workbench-save.json");

#[test]
fn cpush_session_matches_shared_golden_contract() {
    let expected: Value = serde_json::from_str(CPUSH_SESSION_FIXTURE).unwrap();
    let sample = CPushSessionSnapshot {
        run_id: "run-contract-cpush-1".into(),
        repository_id: "repo-orders".into(),
        worktree_id: "wt-orders-main".into(),
        session_id: "sess-orders-1".into(),
        frozen_scope_version: "sv-101".into(),
        model_id: "model-claude-sonnet".into(),
        frozen_file_ids: vec!["file-order".into(), "file-notes".into()],
        committed_file_ids: vec!["file-order".into()],
        skipped_file_ids: vec![],
        session_status: CPushSessionStatus::NeedsDecision,
        pending_call_id: Some("tc-1".into()),
        message: Some(CPushUiMessage {
            id: "q-run-contract-cpush-1-1".into(),
            kind: "question".into(),
            title: "Include release notes in this commit?".into(),
            description: "Confirm how to group RELEASE_NOTES.md".into(),
            scope: Some("Affects commit 2/2 (docs)".into()),
            files: Some(vec![CPushMessageFileChip {
                id: "file-notes".into(),
                label: "RELEASE_NOTES.md".into(),
                badge: Some("Confirm".into()),
            }]),
            commits: Some(vec![CustomActionMessageCommitSummary {
                hash: "a1b2c3d".into(),
                title: "feat(order): validate retry window".into(),
            }]),
            options: None,
            links: Some(vec![CustomActionLink {
                label: "PR #101".into(),
                url: "https://github.com/acme/orders/pull/101".into(),
            }]),
            card: Some(MessageUserCard {
                title: "Include release notes in this commit?".into(),
                subtitle: Some("Affects commit 2/2 (docs)".into()),
                tone: Some(MessageTone::Info),
                blocks: Some(vec![MessageBlock::Markdown {
                    content: "Code and release notes are both selected.".into(),
                }]),
                links: None,
            }),
            questionnaire: Some(MessageUserRequest {
                title: Some("Include release notes in this commit?".into()),
                subtitle: Some("Affects commit 2/2 (docs)".into()),
                tone: Some(MessageTone::Info),
                blocks: None,
                links: None,
                questions: Some(vec![MessageQuestionItem {
                    id: "doc-grouping".into(),
                    header: "Release notes".into(),
                    question: "How should RELEASE_NOTES.md be handled?".into(),
                    multi_select: None,
                    options: vec![
                        MessageQuestionOption {
                            id: "split".into(),
                            label: "Commit separately and push together".into(),
                            description: "Create two commits and push together".into(),
                            recommended: Some(true),
                            preview: None,
                        },
                        MessageQuestionOption {
                            id: "skip".into(),
                            label: "Skip for now".into(),
                            description: "Keep RELEASE_NOTES.md in working tree".into(),
                            recommended: None,
                            preview: None,
                        },
                    ],
                }]),
                context: None,
            }),
            progress: None,
        }),
        timeline: vec![
            CPushTimelineItem {
                state: "done".into(),
                label: "Frozen scope of 2 selected files".into(),
            },
            CPushTimelineItem {
                state: "active".into(),
                label: "Waiting for decision: Include release notes in this commit?".into(),
            },
        ],
        qa_log: vec![],
        commits: vec![CPushLandedCommit {
            oid: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".into(),
            short_oid: "a1b2c3d".into(),
            title: "feat(order): validate retry window".into(),
            branch: "feat/pay".into(),
            file_ids: vec!["file-order".into()],
        }],
        can_retry_remaining: false,
        auto_collapse: None,
    };

    assert_eq!(serde_json::to_value(&sample).unwrap(), expected);
}

#[test]
fn action_session_matches_shared_golden_contract() {
    let expected: Value = serde_json::from_str(ACTION_SESSION_FIXTURE).unwrap();
    let sample = ActionSessionSnapshot {
        run_id: "run-contract-action-1".into(),
        repository_id: "repo-orders".into(),
        worktree_id: "wt-orders-main".into(),
        action_id: "act-submit".into(),
        action_label: "Submit -> main".into(),
        status: "done".into(),
        summary: "Created Issue #42 and opened PR #101".into(),
        message: Some(CustomActionUiMessage {
            id: "done-action-run-contract-action-1".into(),
            kind: "done".into(),
            title: "Submit -> main".into(),
            description: "Created Issue #42 and opened PR #101".into(),
            scope: None,
            files: None,
            commits: Some(vec![CustomActionMessageCommitSummary {
                hash: "a1b2c3d".into(),
                title: "feat(order): validate retry window".into(),
            }]),
            links: Some(vec![
                CustomActionLink {
                    label: "Issue #42".into(),
                    url: "https://github.com/acme/orders/issues/42".into(),
                },
                CustomActionLink {
                    label: "PR #101".into(),
                    url: "https://github.com/acme/orders/pull/101".into(),
                },
            ]),
            card: None,
            questionnaire: None,
            progress: None,
        }),
        timeline: vec![
            CPushTimelineItem {
                state: "done".into(),
                label: "Checked GitHub CLI login".into(),
            },
            CPushTimelineItem {
                state: "done".into(),
                label: "Created PR #101".into(),
            },
        ],
        commits: vec![CPushLandedCommit {
            oid: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".into(),
            short_oid: "a1b2c3d".into(),
            title: "feat(order): validate retry window".into(),
            branch: "fix/issue-42".into(),
            file_ids: vec!["file-order".into()],
        }],
        links: vec![
            CustomActionLink {
                label: "Issue #42".into(),
                url: "https://github.com/acme/orders/issues/42".into(),
            },
            CustomActionLink {
                label: "PR #101".into(),
                url: "https://github.com/acme/orders/pull/101".into(),
            },
        ],
    };

    assert_eq!(serde_json::to_value(&sample).unwrap(), expected);
}

#[test]
fn repository_read_reply_matches_shared_golden_contract() {
    let expected: Value = serde_json::from_str(REPO_SNAPSHOT_FIXTURE).unwrap();
    let sample = ReadReply {
        protocol_version: 1,
        request_id: "req-contract-read-1".into(),
        target: Some(ReadTarget {
            repository_id: "repo-orders".into(),
            worktree_id: "wt-orders-main".into(),
            session_id: "sess-orders-1".into(),
            scope_version: "sv-101".into(),
        }),
        snapshot: Some(RepositorySnapshot {
            repository: RepositoryInfo {
                id: "repo-orders".into(),
                name: "orders".into(),
                branches: vec![
                    Branch {
                        name: "feat/pay".into(),
                        worktree_id: Some("wt-orders-main".into()),
                    },
                    Branch {
                        name: "main".into(),
                        worktree_id: None,
                    },
                ],
                remote_branches: vec![RemoteBranch {
                    name: "origin/main".into(),
                    remote: "origin".into(),
                    local_name: "main".into(),
                }],
                local_keep_paths: vec!["config/local.dev.yaml".into()],
            },
            worktrees: vec![WorktreeSnapshot {
                repository_id: "repo-orders".into(),
                worktree_id: "wt-orders-main".into(),
                label: "orders".into(),
                path: "/tmp/orders".into(),
                branch: "feat/pay".into(),
                detached: false,
                head: Some("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".into()),
                status_version: "sv-101".into(),
                branch_token: Some("bt-contract-1".into()),
                read_error: None,
                remote: Remote::Tracked {
                    branch: "origin/feat/pay".into(),
                    ahead: 1,
                    behind: 0,
                    local_only: false,
                },
                files: vec![Change {
                    id: "file-order".into(),
                    path: "src/order.ts".into(),
                    previous_path: None,
                    status: "M".into(),
                    git_status: "MM".into(),
                    range: Range {
                        kind: "partial".into(),
                        added: 12,
                        removed: 3,
                    },
                    ignored_for_review: false,
                    local_keep: false,
                    binary: false,
                    content_version: "cv-1".into(),
                    commit_token: "ct-contract-1".into(),
                }],
            }],
            active_worktree_id: "wt-orders-main".into(),
            read_sequence: 4,
        }),
    };

    assert_eq!(serde_json::to_value(&sample).unwrap(), expected);
}

#[test]
fn tray_state_and_workbench_save_roundtrip_shared_golden_contracts() {
    let tray: TrayStateSnapshot = serde_json::from_str(TRAY_STATE_FIXTURE).unwrap();
    assert_eq!(tray.protocol_version, 1);
    assert_eq!(tray.locale, LanguageKind::En);
    assert_eq!(tray.theme, ThemeKind::Dark);
    assert!(tray.has_update);
    assert_eq!(tray.repositories.len(), 1);
    assert_eq!(
        tray.repositories[0].session_status,
        TraySessionStatus::NeedsDecision
    );
    assert_eq!(
        serde_json::to_value(&tray).unwrap(),
        serde_json::from_str::<Value>(TRAY_STATE_FIXTURE).unwrap()
    );

    let save_req: WorkbenchSaveRequest = serde_json::from_str(WORKBENCH_SAVE_FIXTURE).unwrap();
    assert_eq!(save_req.revision, 7);
    assert_eq!(save_req.locale, LanguageKind::Zh);
    assert_eq!(save_req.theme, ThemeKind::System);
    assert_eq!(save_req.editor, EditorKind::Rustrover);
    assert_eq!(save_req.default_commit_language, LanguageKind::En);
    assert_eq!(save_req.repository_settings.len(), 1);
    assert_eq!(save_req.repository_settings[0].custom_actions.len(), 1);
    assert_eq!(save_req.worktrees.len(), 1);
}
