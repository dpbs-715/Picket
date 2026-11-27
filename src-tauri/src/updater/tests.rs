use super::{
    app_bundle_root_from_exe, blocker_message, classify_updater_error, detect_install_blocker,
    sanitize_release_notes, types::*, unix_secs_to_iso8601, validate_install_preconditions,
};
use crate::{actions::CustomActionCoordinator, i18n::LanguageKind, repository::RepositoryStore};
use std::path::Path;
use tauri_plugin_updater::Error as UpdaterError;

#[test]
fn detects_install_blockers_accurately_for_macos_bundles_and_dev_builds() {
    let apps_exe = Path::new("/Applications/Picket.app/Contents/MacOS/Picket");
    assert_eq!(
        app_bundle_root_from_exe(apps_exe).as_deref(),
        Some(Path::new("/Applications/Picket.app"))
    );
    assert_eq!(
        app_bundle_root_from_exe(Path::new("/Users/dev/Picket/target/debug/picket")),
        None
    );

    // Debug build always blocks self-replacement:
    assert_eq!(
        detect_install_blocker(apps_exe, true, "dW50cnVzdGVk"),
        UpdateInstallBlocker::DevBuild
    );

    // Non-bundle binary blocks even in release mode:
    assert_eq!(
        detect_install_blocker(Path::new("/usr/local/bin/picket"), false, "dW50cnVzdGVk"),
        UpdateInstallBlocker::DevBuild
    );

    // Gatekeeper AppTranslocation blocks in-app updates:
    assert_eq!(
        detect_install_blocker(
            Path::new(
                "/private/var/folders/xx/AppTranslocation/ABC/d/Picket.app/Contents/MacOS/Picket"
            ),
            false,
            "dW50cnVzdGVk"
        ),
        UpdateInstallBlocker::AppTranslocation
    );

    // Unconfigured signing key blocks in-app install even inside /Applications:
    assert_eq!(
        detect_install_blocker(apps_exe, false, "   "),
        UpdateInstallBlocker::SigningKeyUnconfigured
    );

    // Configured signing key inside /Applications allows in-app install:
    assert_eq!(
        detect_install_blocker(apps_exe, false, "dW50cnVzdGVkIGNvbW1lbnQ="),
        UpdateInstallBlocker::None
    );

    // Non-existent /Volumes/... mount is detected as read-only:
    assert_eq!(
        detect_install_blocker(
            Path::new("/Volumes/Picket v0.1.0/Picket.app/Contents/MacOS/Picket"),
            false,
            "dW50cnVzdGVk"
        ),
        UpdateInstallBlocker::ReadOnlyVolume
    );
}

#[test]
fn enforces_git_busy_guard_blockers_and_version_matching_before_install() {
    let repo_store = RepositoryStore::default();
    let actions = CustomActionCoordinator::default();
    assert!(!repo_store.has_active_write());
    assert!(!actions.has_active_operation());

    {
        let _write_guard = repo_store.track_write();
        assert!(repo_store.has_active_write());
        let err_en = validate_install_preconditions(
            UpdateInstallBlocker::None,
            repo_store.has_active_write(),
            "0.2.0",
            Some("0.2.0"),
            LanguageKind::En,
        )
        .unwrap_err();
        assert!(err_en.contains("Git or C-Push task is currently running"));
    }
    assert!(!repo_store.has_active_write());

    {
        let _op_guard = actions.track_operation();
        assert!(actions.has_active_operation());
        let err_zh = validate_install_preconditions(
            UpdateInstallBlocker::None,
            actions.has_active_operation(),
            "0.2.0",
            Some("0.2.0"),
            LanguageKind::Zh,
        )
        .unwrap_err();
        assert_eq!(
            err_zh,
            crate::i18n::updater::updater_git_busy_error(LanguageKind::Zh)
        );
    }
    assert!(!actions.has_active_operation());

    // Blocker prevents install with localized explanation:
    for blocker in [
        UpdateInstallBlocker::DevBuild,
        UpdateInstallBlocker::ReadOnlyVolume,
        UpdateInstallBlocker::AppTranslocation,
        UpdateInstallBlocker::SigningKeyUnconfigured,
    ] {
        assert!(blocker_message(blocker, LanguageKind::En).is_some());
        assert!(blocker_message(blocker, LanguageKind::Zh).is_some());
        assert!(
            validate_install_preconditions(
                blocker,
                false,
                "0.2.0",
                Some("0.2.0"),
                LanguageKind::En
            )
            .is_err()
        );
    }

    // Version mismatch or missing pending update is rejected:
    assert!(
        validate_install_preconditions(
            UpdateInstallBlocker::None,
            false,
            "0.2.0",
            Some("0.1.9"),
            LanguageKind::En,
        )
        .is_err()
    );
    assert!(
        validate_install_preconditions(
            UpdateInstallBlocker::None,
            false,
            "0.2.0",
            None,
            LanguageKind::En,
        )
        .is_err()
    );

    // Matching pending version with no blockers and idle Git succeeds:
    assert!(
        validate_install_preconditions(
            UpdateInstallBlocker::None,
            false,
            "0.2.0",
            Some("0.2.0"),
            LanguageKind::En,
        )
        .is_ok()
    );
}

#[test]
fn formats_timestamps_sanitizes_notes_classifies_signature_errors_and_rejects_unknown_wire_fields()
{
    assert_eq!(unix_secs_to_iso8601(0), "1970-01-01T00:00:00Z");
    assert_eq!(unix_secs_to_iso8601(1_764_036_000), "2025-11-25T02:00:00Z");

    assert_eq!(sanitize_release_notes(None), None);
    assert_eq!(sanitize_release_notes(Some("   \n\t  ".into())), None);
    assert_eq!(
        sanitize_release_notes(Some("  Fix crash  ".into())).as_deref(),
        Some("Fix crash")
    );
    let long_zh = "\u{66f4}\u{65b0}".repeat(2_500);
    let truncated = sanitize_release_notes(Some(long_zh)).unwrap();
    assert!(truncated.ends_with('…'));
    assert_eq!(truncated.chars().count(), 4_001);

    let sig_err_en =
        classify_updater_error(&UpdaterError::MissingSignedVersion, LanguageKind::En, true);
    assert!(sig_err_en.contains("signature verification failed"));
    let sig_err_zh =
        classify_updater_error(&UpdaterError::MissingSignedVersion, LanguageKind::Zh, true);
    assert_eq!(
        sig_err_zh,
        crate::i18n::updater::updater_signature_error(LanguageKind::Zh)
    );

    // Wire contracts reject unknown fields (including arbitrary URL injection on open_release_page):
    assert!(
        serde_json::from_value::<OpenReleasePageRequest>(serde_json::json!({
            "locale": "en",
            "url": "https://evil.example.com"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<InstallUpdateRequest>(serde_json::json!({
            "version": "0.2.0",
            "downloadUrl": "https://evil.example.com/app.tar.gz"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<CheckUpdateRequest>(serde_json::json!({
            "manual": true,
            "endpoint": "https://evil.example.com/latest.json"
        }))
        .is_err()
    );
}
