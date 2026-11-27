use super::{patch::CPUSH_MAX_TOTAL_PATCH_BYTES, *};
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
async fn cpush_scope_retains_patch_context_across_large_refactor_file_set() {
    let fixture = Fixture::new().unwrap();
    for idx in 0..150 {
        let content = format!(
            "export const moduleIndex = {idx};\nexport function refactorPart{idx}() {{\n  return 'module-{idx}';\n}}\n"
        );
        fs::write(fixture.root.join(format!("refactor-{idx:03}.ts")), content).unwrap();
    }

    let store = RepositoryStore::default();
    let snapshot = store.open(&fixture.root).await.unwrap();
    let tree = &snapshot.worktrees[0];
    let selections: Vec<CommitSelection> = tree
        .files
        .iter()
        .filter(|f| f.path.starts_with("refactor-"))
        .map(|f| CommitSelection {
            file_id: f.id.clone(),
            commit_token: f.commit_token.clone(),
        })
        .collect();
    assert_eq!(selections.len(), 150);

    let target = ReadTarget {
        repository_id: tree.repository_id.clone(),
        worktree_id: tree.worktree_id.clone(),
        session_id: "sess-150".into(),
        scope_version: tree.status_version.clone(),
    };
    let extracted = store
        .extract_cpush_scope(&target, &selections, LanguageKind::En)
        .await
        .unwrap();
    assert_eq!(extracted.files.len(), 150);
    let total_bytes: usize = extracted
        .files
        .iter()
        .map(|f| f.patch.as_ref().map_or(0, String::len))
        .sum();
    assert!(total_bytes <= CPUSH_MAX_TOTAL_PATCH_BYTES);
    assert!(
        extracted.files.iter().all(|f| f.patch.is_some()),
        "Every file in a 150-file refactor scope should retain patch context within the raised budget"
    );
}
