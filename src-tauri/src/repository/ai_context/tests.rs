use super::{
    patch::{CPUSH_MAX_TOTAL_PATCH_BYTES, REVIEW_MAX_PATCH_BYTES, REVIEW_MIN_PATCH_BYTES},
    *,
};
use crate::{
    i18n::LanguageKind,
    repository::{
        RepositoryStore,
        smoke::Fixture,
        types::{CommitSelection, ReadTarget},
    },
};
use std::fs;

#[test]
fn head_tail_truncation_preserves_both_ends_and_utf8_boundaries() {
    let mut raw = String::from("--- a/src/views/sofDialog.vue\n+++ b/src/views/sofDialog.vue\n");
    for idx in 0..300 {
        raw.push_str(&format!(
            "+  <div>field {idx}: \u{4e2d}\u{6587}-utf8-boundary-validation</div>\n"
        ));
    }
    raw.push_str("+export function submitSofDialog() { return true; }\n");

    let slimmed = truncate_patch_head_tail(&raw, 1024, LanguageKind::En);
    assert!(slimmed.len() <= 1024);
    assert!(slimmed.starts_with("--- a/src/views/sofDialog.vue"));
    assert!(slimmed.contains("middle changed lines omitted"));
    assert!(slimmed.contains("+export function submitSofDialog()"));

    let slimmed_zh = truncate_patch_head_tail(&raw, 1024, LanguageKind::Zh);
    assert!(slimmed_zh.len() <= 1024);
    assert!(slimmed_zh.contains("\u{7701}\u{7565}\u{4e2d}\u{95f4}"));
}

#[tokio::test]
async fn cpush_scope_distributes_patch_budget_across_many_large_files() {
    let fixture = Fixture::new().unwrap();
    for idx in 0..7 {
        let mut content = format!("<template><section id=\"view-{idx}\">\n");
        for line in 0..320 {
            content.push_str(&format!(
                "  <p>domain-field {idx}-{line}: port operation node record</p>\n"
            ));
        }
        content.push_str(&format!(
            "</section></template>\n<script setup>\nconst pageId = {idx};\n</script>\n"
        ));
        fs::write(fixture.root.join(format!("page-{idx}.vue")), content).unwrap();
    }

    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    let tree = &snapshot.worktrees[0];
    let selections: Vec<CommitSelection> = tree
        .files
        .iter()
        .filter(|f| f.path.starts_with("page-"))
        .map(|f| CommitSelection {
            file_id: f.id.clone(),
            commit_token: f.commit_token.clone(),
        })
        .collect();
    assert_eq!(selections.len(), 7);

    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "sess-budget".into(),
        scope_version: tree.status_version.clone(),
    };
    let extracted = store
        .extract_cpush_scope(&target, &selections, LanguageKind::En)
        .await
        .unwrap();
    assert_eq!(extracted.files.len(), 7);

    let total_bytes: usize = extracted
        .files
        .iter()
        .map(|f| f.patch.as_ref().map_or(0, String::len))
        .sum();
    assert!(total_bytes <= CPUSH_MAX_TOTAL_PATCH_BYTES);

    for file in &extracted.files {
        let patch = file
            .patch
            .as_deref()
            .expect("every file gets fair patch share");
        assert!(patch.contains("<template><section"));
        assert!(patch.contains("<script setup>"));
        assert_eq!(
            file.skip_reason.as_deref(),
            Some("Large diff, patch truncated")
        );
    }
}

#[tokio::test]
async fn cpush_scope_preserves_patch_context_across_ninety_five_files_within_budget() {
    let fixture = Fixture::new().unwrap();
    for idx in 0..95 {
        let content = format!(
            "export const moduleIndex = {idx};\nexport function runFeature{idx}() {{\n  return 'value-{idx}';\n}}\n"
        );
        fs::write(fixture.root.join(format!("mod-{idx:02}.ts")), content).unwrap();
    }

    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    let tree = &snapshot.worktrees[0];
    let selections: Vec<CommitSelection> = tree
        .files
        .iter()
        .filter(|f| f.path.starts_with("mod-"))
        .map(|f| CommitSelection {
            file_id: f.id.clone(),
            commit_token: f.commit_token.clone(),
        })
        .collect();
    assert_eq!(selections.len(), 95);

    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "sess-95".into(),
        scope_version: tree.status_version.clone(),
    };
    let extracted = store
        .extract_cpush_scope(&target, &selections, LanguageKind::En)
        .await
        .unwrap();
    assert_eq!(extracted.files.len(), 95);
    let total_bytes: usize = extracted
        .files
        .iter()
        .map(|f| f.patch.as_ref().map_or(0, String::len))
        .sum();
    assert!(total_bytes <= CPUSH_MAX_TOTAL_PATCH_BYTES);
    assert!(
        extracted.files.iter().all(|f| f.patch.is_some()),
        "Every file in a 95-file scope should retain patch context without tail starvation"
    );
}

#[tokio::test]
async fn review_scope_fair_budget_prevents_alphabetical_starvation_and_tracks_diff_tool_coverage() {
    let fixture = Fixture::new().unwrap();
    let mut giant = String::from("export const giantStart = true;\n");
    for idx in 0..900 {
        giant.push_str(&format!(
            "export const item_{idx:04} = 'payload-line-with-realistic-content-{idx:04}';\n"
        ));
    }
    giant.push_str("export const giantEnd = true;\n");
    fs::write(fixture.root.join("a-giant.ts"), giant).unwrap();
    for idx in 0..5 {
        fs::write(
            fixture.root.join(format!("z-small-{idx}.ts")),
            format!("export const small{idx} = {idx};\n"),
        )
        .unwrap();
    }

    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    let tree = &snapshot.worktrees[0];
    let selections: Vec<CommitSelection> = tree
        .files
        .iter()
        .filter(|f| f.path == "a-giant.ts" || f.path.starts_with("z-small-"))
        .map(|f| CommitSelection {
            file_id: f.id.clone(),
            commit_token: f.commit_token.clone(),
        })
        .collect();
    assert_eq!(selections.len(), 6);

    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "sess-review-fair".into(),
        scope_version: tree.status_version.clone(),
    };
    let scope = store
        .extract_ai_scope_with_budget(&target, &selections, LanguageKind::En, 24 * 1024)
        .await
        .unwrap();

    assert_eq!(scope.files.len(), 6);
    assert!(scope.skipped_paths.is_empty());
    // Every smaller `z-small-*` file got its 100% full patch without starvation:
    for idx in 0..5 {
        let path = format!("z-small-{idx}.ts");
        let entry = scope.files.iter().find(|f| f.path == path).unwrap();
        assert!(
            entry
                .patch
                .contains(&format!("export const small{idx} = {idx};"))
        );
        assert!(
            !scope.deferred_notices.iter().any(|n| n.path == path),
            "Small file {path} should not be truncated or starved by earlier large file"
        );
    }

    // Only `a-giant.ts` was truncated inline and carries a `read_scope_diff` hint + deferred notice:
    assert_eq!(scope.deferred_notices.len(), 1);
    let notice = &scope.deferred_notices[0];
    assert_eq!(notice.path, "a-giant.ts");
    assert!(!notice.deferred_only);
    assert!(!ranges_cover_all_lines(
        notice.total_lines,
        &notice.initial_ranges
    ));
    let giant_entry = scope.files.iter().find(|f| f.path == "a-giant.ts").unwrap();
    assert!(giant_entry.patch.contains("read_scope_diff"));

    // Reading an unauthorized file outside the scope is strictly refused:
    assert!(
        read_authorized_scope_diff(&scope.authorized_patches, "outside.ts", Some(1), Some(20))
            .is_err()
    );

    // Reading remaining diff chunks via `read_authorized_scope_diff` completes 100% line coverage:
    let mut combined = notice.initial_ranges.clone();
    let mut cursor = 1usize;
    while cursor <= notice.total_lines {
        let (chunk, start, end, total) = read_authorized_scope_diff(
            &scope.authorized_patches,
            "a-giant.ts",
            Some(cursor),
            Some(cursor + 249),
        )
        .unwrap();
        assert_eq!(total, notice.total_lines);
        assert!(chunk.contains("Authorized diff: a-giant.ts"));
        combined.push((start, end));
        if end >= total {
            break;
        }
        cursor = end + 1;
    }
    assert!(ranges_cover_all_lines(notice.total_lines, &combined));

    // Dynamic model-aware budget helper derives inline diff bytes directly from
    // (context_window - max_output_tokens - 8k reserve) * 55% * 3 bytes/token,
    // clamped to [32 KB, 1.5 MB]:
    assert_eq!(review_patch_budget_for_model(32_000, 4_096), 32_841);
    assert_eq!(review_patch_budget_for_model(128_000, 8_192), 184_482);
    assert_eq!(review_patch_budget_for_model(256_000, 16_384), 382_164);
    assert_eq!(
        review_patch_budget_for_model(1_000_000, 32_768),
        REVIEW_MAX_PATCH_BYTES
    );
    // Tiny/misconfigured windows clamp safely to the 32 KB floor without underflow:
    assert_eq!(
        review_patch_budget_for_model(8_000, 8_000),
        REVIEW_MIN_PATCH_BYTES
    );
}
