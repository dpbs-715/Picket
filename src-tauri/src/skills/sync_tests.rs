use super::{
    RuntimeSkillSnapshot,
    sync::{
        DEFAULT_GIT_SKILLS, DEFAULT_SKILLS_SOURCE_REPO, ResetSkillsRequest, SkillManager,
        SyncSkillsRequest, UpdateEnabledSkillsRequest, sanitize_display_source_repo,
    },
};
use crate::i18n::LanguageKind;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;

fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("/usr/bin/git")
        .current_dir(cwd)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Skill Tester")
        .env("GIT_AUTHOR_EMAIL", "tester@picket.local")
        .env("GIT_COMMITTER_NAME", "Skill Tester")
        .env("GIT_COMMITTER_EMAIL", "tester@picket.local")
        .output()
        .expect("git command should run");
    assert!(
        out.status.success(),
        "git {:?} failed:\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn create_sample_skills_repo() -> (TempDir, PathBuf, String) {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("upstream-skills");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.name", "Skill Tester"]);
    git(&repo, &["config", "user.email", "tester@picket.local"]);

    for skill in DEFAULT_GIT_SKILLS {
        let dir = repo.join("skills").join(skill);
        fs::create_dir_all(&dir).unwrap();
        let body = if *skill == "dcr" {
            fs::create_dir_all(dir.join("references")).unwrap();
            fs::write(
                dir.join("references/checklist.md"),
                "# Hot-Synced DCR Checklist v1\n",
            )
            .unwrap();
            // Also include an evals/ folder that MUST be filtered out during sync:
            fs::create_dir_all(dir.join("evals")).unwrap();
            fs::write(dir.join("evals/cases.json"), r#"{"cases":[]}"#).unwrap();
            "---\nname: dcr\ndescription: \"Hot-synced diff code review v1\"\n---\n\nSee [checklist](references/checklist.md).\n"
        } else {
            &format!(
                "---\nname: {skill}\ndescription: \"Synced {skill} skill\"\n---\n\n# {skill}\n"
            )
        };
        fs::write(dir.join("SKILL.md"), body).unwrap();
    }

    // Add a non-Git skill (`apifox`) and a new custom skill (`release-tag`):
    for (extra, desc) in [
        ("apifox", "Apifox API sync (non-Git)"),
        ("release-tag", "Bump semver tag and push release"),
    ] {
        let dir = repo.join("skills").join(extra);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {extra}\ndescription: \"{desc}\"\n---\n\n# {extra}\n"),
        )
        .unwrap();
    }

    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "feat: initial skills"]);
    let sha = git(&repo, &["rev-parse", "HEAD"]);
    (tmp, repo, sha)
}

#[test]
fn sanitizes_embedded_url_credentials_before_persisting() {
    assert_eq!(
        sanitize_display_source_repo("https://user:ghp_secret123@github.com/acme/skills.git/"),
        "https://github.com/acme/skills"
    );
    assert_eq!(
        sanitize_display_source_repo("https://token@gitlab.example.com/team/skills.git"),
        "https://gitlab.example.com/team/skills"
    );
    assert_eq!(
        sanitize_display_source_repo("https://github.com/dpbs-715/skills"),
        DEFAULT_SKILLS_SOURCE_REPO
    );
}

#[tokio::test]
async fn phase4_syncs_shallow_validates_preserves_enabled_and_restores_bundled() {
    let (_upstream_tmp, upstream_repo, sha_v1) = create_sample_skills_repo();
    let data_tmp = TempDir::new().unwrap();
    let data_dir = data_tmp.path().join(".picket");
    let manager = SkillManager::default();

    // 1. Initial state without `~/.picket/skills/` is bundled with all 8 default Git skills:
    let initial = manager.get_state(&data_dir).await;
    assert_eq!(initial.mode, "bundled");
    assert_eq!(initial.skills.len(), 8);
    assert!(
        initial
            .skills
            .iter()
            .all(|s| s.built_in && s.enabled && s.source == "bundled")
    );

    // 2. First hot-sync from local upstream repo:
    let synced = manager
        .sync_skills(
            &data_dir,
            &SyncSkillsRequest {
                source_repo: Some(upstream_repo.to_string_lossy().into_owned()),
                locale: LanguageKind::En,
            },
        )
        .await
        .expect("initial skill sync should succeed");
    assert_eq!(synced.mode, "synced");
    assert_eq!(synced.source_commit.as_deref(), Some(sha_v1.as_str()));
    assert!(synced.synced_at.is_some());
    assert_eq!(synced.skills.len(), 10);

    // Default 8 Git skills enabled; non-Git `apifox` & extra `release-tag` discovered but unchecked by default:
    let enabled_names: Vec<&str> = synced
        .skills
        .iter()
        .filter(|s| s.enabled)
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(enabled_names.len(), 8);
    assert!(!enabled_names.contains(&"apifox"));
    assert!(!enabled_names.contains(&"release-tag"));

    // Permissions 0700 dir / 0600 manifest, and `evals/` excluded:
    let skills_dir = data_dir.join("skills");
    let manifest_path = skills_dir.join("skills-manifest.json");
    assert_eq!(
        fs::metadata(&skills_dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&manifest_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!skills_dir.join("dcr/evals").exists());

    // Capture startup RuntimeSkillSnapshot (simulating an in-flight task):
    let in_flight_snapshot = RuntimeSkillSnapshot::load_from_data_dir(&data_dir);
    assert_eq!(in_flight_snapshot.catalog.skills.len(), 8);
    assert!(
        in_flight_snapshot
            .read_doc("dcr", "references/checklist.md")
            .unwrap()
            .unwrap()
            .contains("Checklist v1")
    );
    assert!(
        in_flight_snapshot
            .read_doc("release-tag", "SKILL.md")
            .unwrap()
            .is_none()
    );

    // 3. Enable newly discovered `release-tag` via `set_enabled_skills`:
    let mut next_enabled: Vec<String> =
        DEFAULT_GIT_SKILLS.iter().map(|s| (*s).to_owned()).collect();
    next_enabled.push("release-tag".into());
    let updated = manager
        .set_enabled_skills(
            &data_dir,
            &UpdateEnabledSkillsRequest {
                enabled_skills: next_enabled,
                locale: LanguageKind::En,
            },
        )
        .await
        .expect("enabling release-tag should succeed");
    assert!(
        updated
            .skills
            .iter()
            .any(|s| s.name == "release-tag" && s.enabled)
    );

    // In-flight task stays locked to its startup snapshot; NEW task sees `release-tag`!
    assert!(
        in_flight_snapshot
            .read_doc("release-tag", "SKILL.md")
            .unwrap()
            .is_none()
    );
    let next_task_snapshot = RuntimeSkillSnapshot::load_from_data_dir(&data_dir);
    assert_eq!(next_task_snapshot.catalog.skills.len(), 9);
    assert!(
        next_task_snapshot
            .read_doc("release-tag", "SKILL.md")
            .unwrap()
            .unwrap()
            .contains("# release-tag")
    );

    // 4. Push commit v2 upstream and re-sync: content updates AND user's `release-tag` choice is preserved!
    fs::write(
        upstream_repo.join("skills/dcr/references/checklist.md"),
        "# Hot-Synced DCR Checklist v2\n",
    )
    .unwrap();
    git(&upstream_repo, &["add", "."]);
    git(
        &upstream_repo,
        &["commit", "-m", "docs: update dcr checklist to v2"],
    );
    let sha_v2 = git(&upstream_repo, &["rev-parse", "HEAD"]);

    let resynced = manager
        .sync_skills(
            &data_dir,
            &SyncSkillsRequest {
                source_repo: Some(upstream_repo.to_string_lossy().into_owned()),
                locale: LanguageKind::En,
            },
        )
        .await
        .expect("re-sync should succeed");
    assert_eq!(resynced.source_commit.as_deref(), Some(sha_v2.as_str()));
    assert!(
        resynced
            .skills
            .iter()
            .any(|s| s.name == "release-tag" && s.enabled),
        "re-syncing same repo must preserve previously enabled custom skills"
    );
    let v2_snapshot = RuntimeSkillSnapshot::load_from_data_dir(&data_dir);
    assert!(
        v2_snapshot
            .read_doc("dcr", "references/checklist.md")
            .unwrap()
            .unwrap()
            .contains("Checklist v2")
    );

    // 5. Broken upstream repo (broken relative Markdown link) MUST fail cleanly without disturbing active v2 skills!
    let bad_tmp = TempDir::new().unwrap();
    let bad_repo = bad_tmp.path().join("bad-skills");
    fs::create_dir_all(bad_repo.join("skills/broken")).unwrap();
    git(&bad_repo, &["init", "-b", "main"]);
    git(&bad_repo, &["config", "user.name", "Skill Tester"]);
    git(&bad_repo, &["config", "user.email", "tester@picket.local"]);
    fs::write(
        bad_repo.join("skills/broken/SKILL.md"),
        "---\nname: broken\ndescription: \"Has broken link\"\n---\n\nSee [missing](references/does-not-exist.md).\n",
    )
    .unwrap();
    git(&bad_repo, &["add", "."]);
    git(&bad_repo, &["commit", "-m", "chore: broken link"]);

    let err = manager
        .sync_skills(
            &data_dir,
            &SyncSkillsRequest {
                source_repo: Some(bad_repo.to_string_lossy().into_owned()),
                locale: LanguageKind::En,
            },
        )
        .await
        .expect_err("syncing a repo with broken relative links must fail");
    assert!(err.contains("Broken relative link"), "{err}");

    // Existing v2 skills remain 100% intact after failed sync!
    let after_fail = manager.get_state(&data_dir).await;
    assert_eq!(after_fail.source_commit.as_deref(), Some(sha_v2.as_str()));
    assert!(
        RuntimeSkillSnapshot::load_from_data_dir(&data_dir)
            .read_doc("dcr", "references/checklist.md")
            .unwrap()
            .unwrap()
            .contains("Checklist v2")
    );

    // 6. Restore built-in bundled skills removes `~/.picket/skills/` cleanly:
    let reset = manager
        .reset_to_bundled(
            &data_dir,
            &ResetSkillsRequest {
                locale: LanguageKind::En,
            },
        )
        .await
        .expect("reset to bundled should succeed");
    assert_eq!(reset.mode, "bundled");
    assert!(!data_dir.join("skills").exists());
    assert!(RuntimeSkillSnapshot::load_from_data_dir(&data_dir).is_empty());
}

#[tokio::test]
async fn phase4_rejects_unsafe_source_urls_and_enables_all_skills_for_custom_non_git_repo() {
    let data_tmp = TempDir::new().unwrap();
    let data_dir = data_tmp.path().join(".picket");
    let manager = SkillManager::default();

    for bad_url in [
        "--upload-pack=sh",
        "ext::sh -c id",
        "javascript:alert(1)",
        "https://example.com/repo with spaces",
    ] {
        let err = manager
            .sync_skills(
                &data_dir,
                &SyncSkillsRequest {
                    source_repo: Some(bad_url.into()),
                    locale: LanguageKind::En,
                },
            )
            .await
            .unwrap_err();
        assert!(err.contains("valid Git repository URL"), "{bad_url}: {err}");
    }

    // Third-party repo with custom root-level skills (no `skills/` subdir, none of the 8 default names):
    let custom_tmp = TempDir::new().unwrap();
    let custom_repo = custom_tmp.path().join("custom-team-skills");
    fs::create_dir_all(custom_repo.join("changelog-gen")).unwrap();
    git(&custom_repo, &["init", "-b", "main"]);
    git(&custom_repo, &["config", "user.name", "Skill Tester"]);
    git(
        &custom_repo,
        &["config", "user.email", "tester@picket.local"],
    );
    fs::write(
        custom_repo.join("changelog-gen/SKILL.md"),
        "---\nname: changelog-gen\ndescription: \"Generate release changelog\"\n---\n\n# Changelog\n",
    )
    .unwrap();
    git(&custom_repo, &["add", "."]);
    git(&custom_repo, &["commit", "-m", "feat: custom skill"]);

    let synced = manager
        .sync_skills(
            &data_dir,
            &SyncSkillsRequest {
                source_repo: Some(custom_repo.to_string_lossy().into_owned()),
                locale: LanguageKind::En,
            },
        )
        .await
        .expect("custom root-level skills repo should sync cleanly");
    assert_eq!(synced.skills.len(), 1);
    assert_eq!(synced.skills[0].name, "changelog-gen");
    assert!(synced.skills[0].enabled);
    assert!(!synced.skills[0].built_in);
}
