use super::{
    popover::{
        TRAY_ICON_DOT, TRAY_ICON_HEIGHT, TRAY_ICON_NORMAL, TRAY_ICON_WIDTH, TRAY_POPOVER_WIDTH,
        compute_popover_position, resolve_tray_anchor_logical,
    },
    state::{
        TrayRepositorySummary, TraySessionStatus, TrayStateSnapshot, TrayWorktreeSessionInfo,
        build_tray_state_snapshot, build_tray_tooltip, tray_needs_attention,
        validate_tray_snapshot,
    },
};
use crate::{
    i18n::LanguageKind,
    repository::types::{
        Change, Range, Remote, RepositoryInfo, RepositorySnapshot, ThemeKind, WorktreeSnapshot,
    },
    window_motion::MonitorWorkArea,
};
use std::collections::HashMap;
use tauri::{PhysicalPosition, PhysicalSize, Position, Rect, Size};

fn sample_repo(name: &str, changes: u32, status: TraySessionStatus) -> TrayRepositorySummary {
    TrayRepositorySummary {
        repository_id: format!("repo:{name}"),
        worktree_id: format!("wt:{name}"),
        name: name.to_string(),
        branch: "main".to_string(),
        detached: false,
        ahead: 0,
        behind: 0,
        has_upstream: true,
        change_count: changes,
        local_keep_count: 0,
        worktree_count: 1,
        active: false,
        read_error: None,
        session_status: status,
        status_detail: None,
    }
}

#[test]
fn tray_template_assets_have_exact_48x36_rgba_dimensions() {
    let expected = (TRAY_ICON_WIDTH * TRAY_ICON_HEIGHT * 4) as usize;
    assert_eq!(TRAY_ICON_NORMAL.len(), expected);
    assert_eq!(TRAY_ICON_DOT.len(), expected);
    let normal_alpha: u64 = TRAY_ICON_NORMAL
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| u64::from(px[3]))
        .sum();
    let dot_alpha: u64 = TRAY_ICON_DOT
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| u64::from(px[3]))
        .sum();
    assert!(normal_alpha > 0);
    assert!(dot_alpha > normal_alpha);
}

#[test]
fn popover_positioning_centers_under_icon_clamps_right_edge_and_supports_secondary_monitors() {
    let primary = MonitorWorkArea {
        x: 0.0,
        y: 25.0,
        width: 1512.0,
        height: 945.0,
    };
    let secondary = MonitorWorkArea {
        x: 1512.0,
        y: 0.0,
        width: 2560.0,
        height: 1415.0,
    };

    let rect = Rect {
        position: Position::Physical(PhysicalPosition::new(2936, 0)),
        size: Size::Physical(PhysicalSize::new(48, 50)),
    };
    let (cx, bottom) = resolve_tray_anchor_logical(
        rect,
        Some(1480.0),
        &[(primary, 2.0), (secondary, 1.0)],
        Some((primary, 2.0)),
    );
    assert_eq!((cx, bottom), (1480.0, 25.0));

    let (px, py) = compute_popover_position(
        cx,
        bottom,
        TRAY_POPOVER_WIDTH,
        240.0,
        &[primary, secondary],
        Some(primary),
    );
    assert_eq!(px, 1184.0);
    assert_eq!(py, 31.0);

    let sec_rect = Rect {
        position: Position::Physical(PhysicalPosition::new(2188, 0)),
        size: Size::Physical(PhysicalSize::new(24, 24)),
    };
    let (scx, sbottom) = resolve_tray_anchor_logical(
        sec_rect,
        Some(2200.0),
        &[(primary, 2.0), (secondary, 1.0)],
        Some((primary, 2.0)),
    );
    let (spx, spy) = compute_popover_position(
        scx,
        sbottom,
        TRAY_POPOVER_WIDTH,
        240.0,
        &[primary, secondary],
        Some(primary),
    );
    assert_eq!(spx, 2200.0 - 160.0);
    assert_eq!(spy, 30.0);
}

#[test]
fn tray_attention_and_tooltip_prioritize_urgent_sessions_changes_and_updates() {
    let clean = TrayStateSnapshot {
        protocol_version: 1,
        locale: LanguageKind::Zh,
        theme: ThemeKind::System,
        has_update: false,
        refreshing: false,
        repositories: vec![sample_repo("alpha", 0, TraySessionStatus::Idle)],
    };
    assert!(!tray_needs_attention(&clean));
    assert_eq!(build_tray_tooltip(&clean), "Picket");

    let with_changes = TrayStateSnapshot {
        protocol_version: 1,
        locale: LanguageKind::En,
        theme: ThemeKind::Dark,
        has_update: false,
        refreshing: false,
        repositories: vec![
            sample_repo("alpha", 3, TraySessionStatus::Idle),
            sample_repo("beta", 1, TraySessionStatus::Idle),
        ],
    };
    assert!(!tray_needs_attention(&with_changes));
    assert_eq!(
        build_tray_tooltip(&with_changes),
        "Picket · 2 repositories with changes"
    );

    let with_decision = TrayStateSnapshot {
        protocol_version: 1,
        locale: LanguageKind::Zh,
        theme: ThemeKind::Light,
        has_update: false,
        refreshing: false,
        repositories: vec![sample_repo("alpha", 2, TraySessionStatus::NeedsDecision)],
    };
    assert!(tray_needs_attention(&with_decision));
    assert_eq!(build_tray_tooltip(&with_decision), "Picket · 待你决定");

    let with_update = TrayStateSnapshot {
        protocol_version: 1,
        locale: LanguageKind::En,
        theme: ThemeKind::System,
        has_update: true,
        refreshing: false,
        repositories: vec![],
    };
    assert!(tray_needs_attention(&with_update));
    assert_eq!(
        build_tray_tooltip(&with_update),
        "Picket · Update available"
    );
}

#[test]
fn tray_snapshot_validation_rejects_unknown_fields_and_invalid_version() {
    let bad = serde_json::from_str::<TrayStateSnapshot>(
        r#"{"protocolVersion":1,"locale":"en","hasUpdate":false,"refreshing":false,"repositories":[],"extra":1}"#,
    );
    assert!(bad.is_err());

    let wrong_version = TrayStateSnapshot {
        protocol_version: 2,
        ..TrayStateSnapshot::default()
    };
    assert!(validate_tray_snapshot(&wrong_version).is_err());
}

#[test]
fn rust_authoritative_tray_snapshot_picks_most_urgent_worktree_and_counts_local_keep() {
    let make_file = |id: &str, local_keep: bool| Change {
        id: id.into(),
        path: format!("src/{id}.ts"),
        previous_path: None,
        status: "M".into(),
        git_status: " M".into(),
        range: Range {
            kind: "unstaged".into(),
            added: 2,
            removed: 1,
        },
        ignored_for_review: false,
        local_keep,
        binary: false,
        content_version: "v1".into(),
        commit_token: "tok".into(),
    };

    let snap = RepositorySnapshot {
        repository: RepositoryInfo {
            id: "repo-1".into(),
            name: "picket".into(),
            branches: vec![],
            remote_branches: vec![],
            local_keep_paths: vec!["src/keep.ts".into()],
        },
        worktrees: vec![
            WorktreeSnapshot {
                repository_id: "repo-1".into(),
                worktree_id: "wt-main".into(),
                label: "picket".into(),
                path: "/tmp/picket".into(),
                branch: "main".into(),
                detached: false,
                head: Some("abc".into()),
                status_version: "s1".into(),
                branch_token: Some("bt1".into()),
                read_error: None,
                remote: Remote::Tracked {
                    branch: "origin/main".into(),
                    ahead: 0,
                    behind: 0,
                    local_only: false,
                },
                files: vec![make_file("keep", true)],
            },
            WorktreeSnapshot {
                repository_id: "repo-1".into(),
                worktree_id: "wt-feat".into(),
                label: "picket-feat".into(),
                path: "/tmp/picket-feat".into(),
                branch: "feat/tray".into(),
                detached: false,
                head: Some("def".into()),
                status_version: "s2".into(),
                branch_token: Some("bt2".into()),
                read_error: None,
                remote: Remote::Tracked {
                    branch: "origin/feat/tray".into(),
                    ahead: 2,
                    behind: 1,
                    local_only: false,
                },
                files: vec![make_file("a", false), make_file("keep", true)],
            },
        ],
        active_worktree_id: "wt-main".into(),
        read_sequence: 1,
    };

    let mut sessions = HashMap::new();
    sessions.insert(
        "wt-feat".into(),
        TrayWorktreeSessionInfo {
            status: TraySessionStatus::NeedsDecision,
            detail: Some("Confirm split".into()),
        },
    );

    let built = build_tray_state_snapshot(
        &[snap],
        &sessions,
        Some("wt-main"),
        LanguageKind::En,
        ThemeKind::Dark,
        true,
        false,
    );
    assert_eq!(built.repositories.len(), 1);
    let item = &built.repositories[0];
    assert_eq!(item.worktree_id, "wt-feat");
    assert_eq!(item.branch, "feat/tray");
    assert_eq!(item.change_count, 1);
    assert_eq!(item.local_keep_count, 1);
    assert_eq!(item.ahead, 2);
    assert_eq!(item.behind, 1);
    assert_eq!(item.session_status, TraySessionStatus::NeedsDecision);
    assert_eq!(item.status_detail.as_deref(), Some("Confirm split"));
    assert!(built.has_update);
}
