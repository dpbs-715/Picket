//! Fast, bounded, read-only recursive workspace module scanner for multi-module repositories
//! (Maven / Spring Cloud, Gradle, pnpm/npm/yarn workspaces, Cargo workspaces, and Go workspaces).
//! Never reads `.env` files, secrets, or symlinks escaping the worktree.

use super::process_io::trim_incomplete_trailing_utf8;
use std::{
    collections::{BTreeSet, HashSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_POM_BYTES: u64 = 256 * 1024;
const MAX_SOURCE_BYTES: u64 = 64 * 1024;
const MAX_CONFIG_BYTES: u64 = 32 * 1024;
const MAX_MANIFEST_BYTES: u64 = 128 * 1024;
const MAX_MAVEN_DEPTH: usize = 5;
const MAX_MAVEN_MODULES: usize = 120;
const MAX_JAVA_FILES_PER_MODULE: usize = 120;
const MAX_JAVA_SCAN_DEPTH: usize = 12;
const MAX_REPORTED_SERVICES: usize = 48;
const MAX_WORKSPACE_DESCRIPTORS: usize = 64;

const IGNORED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    ".idea",
    ".vscode",
    "coverage",
    ".gradle",
    "vendor",
];

fn is_regular_file_inside(canon_root: &Path, path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file())
        && fs::canonicalize(path).is_ok_and(|canon| canon.starts_with(canon_root))
}

fn read_bounded_utf8(canon_root: &Path, path: &Path, max_bytes: u64) -> Option<String> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() {
        return None;
    }
    let canon = fs::canonicalize(path).ok()?;
    if !canon.starts_with(canon_root) {
        return None;
    }
    let file = fs::File::open(&canon).ok()?;
    let mut buf = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut buf)
        .ok()?;
    if buf.contains(&0) {
        return None;
    }
    let max_len = max_bytes as usize;
    if buf.len() > max_len {
        buf.truncate(max_len);
        let safe = trim_incomplete_trailing_utf8(&buf);
        Some(String::from_utf8_lossy(safe).into_owned())
    } else {
        Some(String::from_utf8_lossy(&buf).into_owned())
    }
}

fn canonical_subdir_inside(canon_root: &Path, candidate: &Path) -> Option<(PathBuf, String)> {
    let meta = fs::symlink_metadata(candidate).ok()?;
    if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
        return None;
    }
    let canon = fs::canonicalize(candidate).ok()?;
    if !canon.starts_with(canon_root) {
        return None;
    }
    let rel = canon.strip_prefix(canon_root).ok()?;
    let rel_str = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    Some((canon, rel_str))
}

/// Scans a worktree read-only for multi-module workspace structure and returns compact,
/// non-secret structured descriptors suitable for Pi Core `discoverRunCommands`.
pub fn scan_workspace_modules(worktree_root: &Path) -> Vec<String> {
    let Ok(canon_root) = fs::canonicalize(worktree_root) else {
        return Vec::new();
    };
    let mut out = Vec::new();

    scan_maven_workspace(&canon_root, &mut out);
    scan_gradle_workspace(&canon_root, &mut out);
    scan_node_workspace(&canon_root, &mut out);
    scan_cargo_workspace(&canon_root, &mut out);
    scan_go_workspace(&canon_root, &mut out);

    out.truncate(MAX_WORKSPACE_DESCRIPTORS);
    out
}

// ---------------------------------------------------------------------------
// Maven / Spring Cloud recursive scanner
// ---------------------------------------------------------------------------

fn strip_xml_comments(xml: &str) -> String {
    let mut result = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest.find("<!--") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 4..];
        if let Some(end) = after.find("-->") {
            rest = &after[end + 3..];
        } else {
            return result;
        }
    }
    result.push_str(rest);
    result
}

fn strip_xml_block(xml: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut result = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        let after_open = &rest[start + open.len()..];
        let is_tag_boundary = after_open
            .as_bytes()
            .first()
            .is_some_and(|b| matches!(b, b'>' | b' ' | b'\t' | b'\r' | b'\n' | b'/'));
        if !is_tag_boundary {
            result.push_str(&rest[..start + open.len()]);
            rest = after_open;
            continue;
        }
        result.push_str(&rest[..start]);
        let Some(gt_pos) = after_open.find('>') else {
            return result;
        };
        if after_open[..gt_pos].trim_end().ends_with('/') {
            rest = &after_open[gt_pos + 1..];
            continue;
        }
        let after_tag = &after_open[gt_pos + 1..];
        if let Some(end) = after_tag.find(&close) {
            rest = &after_tag[end + close.len()..];
        } else {
            return result;
        }
    }
    result.push_str(rest);
    result
}

fn extract_xml_tag_value(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)?;
    let value = xml[start..start + end].trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn extract_maven_top_level_field(cleaned_pom: &str, tag: &str) -> Option<String> {
    let mut stripped = strip_xml_block(cleaned_pom, "parent");
    for block in [
        "dependencies",
        "dependencyManagement",
        "build",
        "profiles",
        "pluginManagement",
        "reporting",
        "modules",
    ] {
        stripped = strip_xml_block(&stripped, block);
    }
    extract_xml_tag_value(&stripped, tag)
}

fn extract_maven_profile_ids(cleaned_pom: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = cleaned_pom;
    while let Some(p_start) = rest.find("<profiles>") {
        let after = &rest[p_start + "<profiles>".len()..];
        let Some(p_end) = after.find("</profiles>") else {
            break;
        };
        let mut block = &after[..p_end];
        while let Some(start) = block.find("<profile>") {
            let p_after = &block[start + "<profile>".len()..];
            let Some(end) = p_after.find("</profile>") else {
                break;
            };
            let profile_body = strip_xml_block(&p_after[..end], "activation");
            let profile_body = strip_xml_block(&profile_body, "dependencies");
            let profile_body = strip_xml_block(&profile_body, "build");
            if let Some(id) = extract_xml_tag_value(&profile_body, "id")
                && is_safe_identifier(&id)
                && !ids.contains(&id)
            {
                ids.push(id);
                if ids.len() >= 8 {
                    return ids;
                }
            }
            block = &p_after[end + "</profile>".len()..];
        }
        rest = &after[p_end + "</profiles>".len()..];
    }
    ids
}

fn extract_maven_modules(cleaned_pom: &str) -> Vec<String> {
    let mut modules = Vec::new();
    let mut rest = cleaned_pom;
    while let Some(start) = rest.find("<modules>") {
        let after = &rest[start + "<modules>".len()..];
        let Some(end) = after.find("</modules>") else {
            break;
        };
        let mut block = &after[..end];
        while let Some(m_start) = block.find("<module>") {
            let m_after = &block[m_start + "<module>".len()..];
            let Some(m_end) = m_after.find("</module>") else {
                break;
            };
            let raw = m_after[..m_end].trim();
            if !raw.is_empty() && !raw.contains("${") && !raw.contains('\0') {
                modules.push(raw.to_owned());
            }
            block = &m_after[m_end + "</module>".len()..];
        }
        rest = &after[end + "</modules>".len()..];
    }
    modules
}

struct SpringBootModuleInfo {
    module_path: String,
    artifact_id: String,
    main_class: Option<String>,
    app_name: Option<String>,
    port: Option<u16>,
    has_boot_plugin: bool,
}

fn scan_maven_workspace(canon_root: &Path, out: &mut Vec<String>) {
    let root_pom_path = canon_root.join("pom.xml");
    let Some(root_pom_raw) = read_bounded_utf8(canon_root, &root_pom_path, MAX_POM_BYTES) else {
        return;
    };
    let root_pom = strip_xml_comments(&root_pom_raw);
    let root_modules = extract_maven_modules(&root_pom);
    if root_modules.is_empty() {
        return;
    }

    let root_artifact =
        extract_maven_top_level_field(&root_pom, "artifactId").unwrap_or_else(|| "root".to_owned());
    let root_profiles = extract_maven_profile_ids(&root_pom);
    let wrapper = if is_regular_file_inside(canon_root, &canon_root.join("mvnw")) {
        "./mvnw"
    } else {
        "mvn"
    };

    let mut visited = HashSet::new();
    visited.insert(canon_root.to_path_buf());

    let mut total_modules = 0usize;
    let mut library_modules = 0usize;
    let mut runnable = Vec::<SpringBootModuleInfo>::new();

    let mut queue: Vec<(PathBuf, String, usize)> = Vec::new();
    for rel in root_modules {
        queue.push((canon_root.join(&rel), rel, 1));
    }

    let mut idx = 0;
    while idx < queue.len() && total_modules < MAX_MAVEN_MODULES {
        let (dir_candidate, _raw_rel, depth) = queue[idx].clone();
        idx += 1;
        if depth > MAX_MAVEN_DEPTH {
            continue;
        }
        let Some((canon_dir, rel_path)) = canonical_subdir_inside(canon_root, &dir_candidate)
        else {
            continue;
        };
        if rel_path.is_empty() || !visited.insert(canon_dir.clone()) {
            continue;
        }
        let Some(pom_raw) =
            read_bounded_utf8(canon_root, &canon_dir.join("pom.xml"), MAX_POM_BYTES)
        else {
            continue;
        };
        total_modules += 1;
        let pom = strip_xml_comments(&pom_raw);
        let child_modules = extract_maven_modules(&pom);
        if depth < MAX_MAVEN_DEPTH {
            for child in &child_modules {
                queue.push((canon_dir.join(child), child.clone(), depth + 1));
            }
        }

        let packaging = extract_maven_top_level_field(&pom, "packaging")
            .unwrap_or_else(|| "jar".to_owned())
            .to_ascii_lowercase();
        if packaging == "pom" || !child_modules.is_empty() {
            continue;
        }

        let artifact_id = extract_maven_top_level_field(&pom, "artifactId").unwrap_or_else(|| {
            canon_dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(&rel_path)
                .to_owned()
        });
        let has_boot_plugin = pom.contains("spring-boot-maven-plugin");
        let main_class = find_spring_boot_main_class(canon_root, &canon_dir);

        if main_class.is_some() || has_boot_plugin {
            let (app_name, port) = extract_spring_config_metadata(canon_root, &canon_dir);
            runnable.push(SpringBootModuleInfo {
                module_path: rel_path,
                artifact_id,
                main_class,
                app_name,
                port,
                has_boot_plugin,
            });
        } else {
            library_modules += 1;
        }
    }

    runnable.sort_by(|a, b| a.module_path.cmp(&b.module_path));

    let mut root_parts = vec![
        "path=.".to_owned(),
        format!("artifact={root_artifact}"),
        format!("wrapper={wrapper}"),
        format!("totalModules={total_modules}"),
        format!("runnableServices={}", runnable.len()),
        format!("libraryModules={library_modules}"),
    ];
    if !root_profiles.is_empty() {
        root_parts.push(format!("profiles={}", root_profiles.join(",")));
    }
    out.push(format!("maven-root: {}", root_parts.join(" ")));

    for svc in runnable.into_iter().take(MAX_REPORTED_SERVICES) {
        let mut parts = vec![
            format!("module={}", svc.module_path),
            format!("artifact={}", svc.artifact_id),
        ];
        if let Some(main_class) = svc.main_class {
            parts.push(format!("mainClass={main_class}"));
        }
        if let Some(app_name) = svc.app_name {
            parts.push(format!("appName={app_name}"));
        }
        if let Some(port) = svc.port {
            parts.push(format!("port={port}"));
        }
        if svc.has_boot_plugin {
            parts.push("bootPlugin=true".into());
        }
        out.push(format!("spring-boot-service: {}", parts.join(" ")));
    }
}

fn is_likely_boot_entry_filename(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with("application.java")
        || lower.ends_with("application.kt")
        || lower.ends_with("app.java")
        || lower.ends_with("app.kt")
        || lower.ends_with("server.java")
        || lower.ends_with("starter.java")
        || lower.ends_with("bootstrap.java")
        || lower.ends_with("main.java")
}

fn find_spring_boot_main_class(canon_root: &Path, module_dir: &Path) -> Option<String> {
    let mut priority_files = Vec::new();
    let mut other_files = Vec::new();

    for source_root in ["src/main/java", "src/main/kotlin"] {
        let Some((root, _)) = canonical_subdir_inside(canon_root, &module_dir.join(source_root))
        else {
            continue;
        };
        collect_jvm_source_files(
            &root,
            0,
            &mut priority_files,
            &mut other_files,
            MAX_JAVA_FILES_PER_MODULE,
        );
    }

    priority_files.sort();
    other_files.sort();

    for file_path in priority_files
        .into_iter()
        .chain(other_files)
        .take(MAX_JAVA_FILES_PER_MODULE)
    {
        let Some(content) = read_bounded_utf8(canon_root, &file_path, MAX_SOURCE_BYTES) else {
            continue;
        };
        let is_boot_entry = content.contains("@SpringBootApplication")
            || (content.contains("SpringApplication.run")
                && (content.contains("@EnableAutoConfiguration")
                    || content.contains("public static void main")
                    || content.contains("fun main")));
        if is_boot_entry
            && let Some(stem) = file_path.file_stem().and_then(|s| s.to_str())
            && is_safe_identifier(stem)
        {
            return Some(stem.to_owned());
        }
    }
    None
}

fn collect_jvm_source_files(
    dir: &Path,
    depth: usize,
    priority: &mut Vec<PathBuf>,
    other: &mut Vec<PathBuf>,
    limit: usize,
) {
    if depth > MAX_JAVA_SCAN_DEPTH || priority.len() + other.len() >= limit {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut dirs = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        if priority.len() + other.len() >= limit {
            break;
        }
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if ft.is_dir() {
            if !name.starts_with('.') && !IGNORED_DIRS.contains(&name.as_str()) {
                dirs.push(entry.path());
            }
        } else if ft.is_file() && (name.ends_with(".java") || name.ends_with(".kt")) {
            if is_likely_boot_entry_filename(&name) {
                priority.push(entry.path());
            } else {
                other.push(entry.path());
            }
        }
    }
    dirs.sort();
    for sub in dirs {
        collect_jvm_source_files(&sub, depth + 1, priority, other, limit);
        if priority.len() + other.len() >= limit {
            break;
        }
    }
}

fn is_safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

fn resolve_placeholder_default(raw: &str) -> &str {
    let trimmed = raw.trim().trim_matches(|c| c == '\'' || c == '"');
    if let Some(inner) = trimmed.strip_prefix("${").and_then(|s| s.strip_suffix('}'))
        && let Some((_var, default_val)) = inner.split_once(':')
    {
        return default_val.trim();
    }
    trimmed
}

fn parse_safe_port(raw: &str) -> Option<u16> {
    let resolved = resolve_placeholder_default(raw);
    let port: u16 = resolved.parse().ok()?;
    (port > 0).then_some(port)
}

fn parse_safe_app_name(raw: &str) -> Option<String> {
    let resolved = resolve_placeholder_default(raw);
    if resolved.is_empty() || resolved.contains("${") || !is_safe_identifier(resolved) {
        return None;
    }
    Some(resolved.to_owned())
}

/// Reads ONLY allowlisted Spring resource files for non-secret `spring.application.name` and
/// `server.port` metadata. Never reads `.env` or secret values.
fn extract_spring_config_metadata(
    canon_root: &Path,
    module_dir: &Path,
) -> (Option<String>, Option<u16>) {
    let Some((resources, _)) =
        canonical_subdir_inside(canon_root, &module_dir.join("src/main/resources"))
    else {
        return (None, None);
    };
    let mut app_name: Option<String> = None;
    let mut port: Option<u16> = None;

    for filename in [
        "bootstrap.yml",
        "bootstrap.yaml",
        "bootstrap.properties",
        "application.yml",
        "application.yaml",
        "application.properties",
    ] {
        if app_name.is_some() && port.is_some() {
            break;
        }
        let Some(text) = read_bounded_utf8(canon_root, &resources.join(filename), MAX_CONFIG_BYTES)
        else {
            continue;
        };
        if filename.ends_with(".properties") {
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
                    continue;
                }
                if let Some((k, v)) = trimmed.split_once('=').or_else(|| trimmed.split_once(':')) {
                    match k.trim() {
                        "spring.application.name" if app_name.is_none() => {
                            app_name = parse_safe_app_name(v);
                        }
                        "server.port" if port.is_none() => {
                            port = parse_safe_port(v);
                        }
                        _ => {}
                    }
                }
            }
        } else {
            let mut section_stack: Vec<(usize, String)> = Vec::new();
            for raw_line in text.lines() {
                let no_comment = raw_line.split('#').next().unwrap_or("");
                let trimmed = no_comment.trim();
                if trimmed.is_empty() || trimmed == "---" {
                    if trimmed == "---" {
                        section_stack.clear();
                    }
                    continue;
                }
                let indent = no_comment.len() - no_comment.trim_start().len();
                while section_stack
                    .last()
                    .is_some_and(|(prev_indent, _)| *prev_indent >= indent)
                {
                    section_stack.pop();
                }
                let Some((key_raw, val_raw)) = trimmed.split_once(':') else {
                    continue;
                };
                let key = key_raw.trim();
                let val = val_raw.trim();
                if val.is_empty() {
                    section_stack.push((indent, key.to_owned()));
                    continue;
                }
                let mut full_key = section_stack
                    .iter()
                    .map(|(_, k)| k.as_str())
                    .collect::<Vec<_>>();
                full_key.push(key);
                let joined = full_key.join(".");
                if joined == "spring.application.name" && app_name.is_none() {
                    app_name = parse_safe_app_name(val);
                } else if joined == "server.port" && port.is_none() {
                    port = parse_safe_port(val);
                }
            }
        }
    }

    (app_name, port)
}

// ---------------------------------------------------------------------------
// Gradle multi-project scanner
// ---------------------------------------------------------------------------

fn scan_gradle_workspace(canon_root: &Path, out: &mut Vec<String>) {
    let settings = read_bounded_utf8(
        canon_root,
        &canon_root.join("settings.gradle.kts"),
        MAX_MANIFEST_BYTES,
    )
    .or_else(|| {
        read_bounded_utf8(
            canon_root,
            &canon_root.join("settings.gradle"),
            MAX_MANIFEST_BYTES,
        )
    });
    let Some(content) = settings else {
        return;
    };

    let mut project_paths = BTreeSet::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with('*') || !trimmed.contains("include") {
            continue;
        }
        for quoted in extract_quoted_tokens(trimmed) {
            let cleaned = quoted.trim_start_matches(':').trim();
            if cleaned.is_empty() || cleaned.contains("..") || cleaned.contains('/') {
                continue;
            }
            project_paths.insert(cleaned.to_owned());
        }
    }

    if project_paths.is_empty() {
        return;
    }

    let wrapper = if is_regular_file_inside(canon_root, &canon_root.join("gradlew")) {
        "./gradlew"
    } else {
        "gradle"
    };

    let mut runnable = Vec::new();
    for gradle_colon_path in &project_paths {
        let rel_dir = gradle_colon_path.replace(':', "/");
        let Some((canon_dir, clean_rel)) =
            canonical_subdir_inside(canon_root, &canon_root.join(&rel_dir))
        else {
            continue;
        };
        let build_text = read_bounded_utf8(
            canon_root,
            &canon_dir.join("build.gradle.kts"),
            MAX_MANIFEST_BYTES,
        )
        .or_else(|| {
            read_bounded_utf8(
                canon_root,
                &canon_dir.join("build.gradle"),
                MAX_MANIFEST_BYTES,
            )
        })
        .unwrap_or_default();
        let has_boot =
            build_text.contains("org.springframework.boot") || build_text.contains("spring-boot");
        let has_app_plugin = build_text.contains("id(\"application\")")
            || build_text.contains("id 'application'")
            || build_text.contains("application {");
        let main_class = find_spring_boot_main_class(canon_root, &canon_dir);

        if main_class.is_some() || has_boot || has_app_plugin {
            let (app_name, port) = extract_spring_config_metadata(canon_root, &canon_dir);
            let mut parts = vec![
                format!("project=:{gradle_colon_path}"),
                format!("path={clean_rel}"),
            ];
            if let Some(mc) = main_class {
                parts.push(format!("mainClass={mc}"));
            }
            if let Some(name) = app_name {
                parts.push(format!("appName={name}"));
            }
            if let Some(p) = port {
                parts.push(format!("port={p}"));
            }
            if has_boot {
                parts.push("task=bootRun".into());
            } else if has_app_plugin {
                parts.push("task=run".into());
            }
            runnable.push(format!("gradle-service: {}", parts.join(" ")));
        }
    }

    out.push(format!(
        "gradle-root: path=. wrapper={wrapper} totalProjects={} runnableServices={}",
        project_paths.len(),
        runnable.len()
    ));
    out.extend(runnable.into_iter().take(MAX_REPORTED_SERVICES));
}

fn extract_quoted_tokens(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            let start = i + 1;
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            if i < bytes.len() && i > start {
                tokens.push(input[start..i].to_owned());
            }
        }
        i += 1;
    }
    tokens
}

// ---------------------------------------------------------------------------
// Node / pnpm / npm / yarn workspace scanner
// ---------------------------------------------------------------------------

fn scan_node_workspace(canon_root: &Path, out: &mut Vec<String>) {
    let mut patterns = Vec::new();
    let mut manager = "npm";

    if let Some(pnpm_yaml) = read_bounded_utf8(
        canon_root,
        &canon_root.join("pnpm-workspace.yaml"),
        MAX_MANIFEST_BYTES,
    ) {
        manager = "pnpm";
        let mut in_packages = false;
        for raw_line in pnpm_yaml.lines() {
            let no_comment = raw_line.split('#').next().unwrap_or("");
            let trimmed = no_comment.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed == "packages:" {
                in_packages = true;
                continue;
            }
            if !raw_line.starts_with(' ')
                && !raw_line.starts_with('\t')
                && !trimmed.starts_with('-')
            {
                in_packages = false;
                continue;
            }
            if in_packages && let Some(item) = trimmed.strip_prefix('-') {
                let pat = item.trim().trim_matches(|c| c == '"' || c == '\'');
                if !pat.is_empty() && !pat.starts_with('!') {
                    patterns.push(pat.to_owned());
                }
            }
        }
    } else if let Some(root_pkg) = read_bounded_utf8(
        canon_root,
        &canon_root.join("package.json"),
        MAX_MANIFEST_BYTES,
    ) && let Ok(val) = serde_json::from_str::<serde_json::Value>(&root_pkg)
    {
        if is_regular_file_inside(canon_root, &canon_root.join("pnpm-lock.yaml")) {
            manager = "pnpm";
        } else if is_regular_file_inside(canon_root, &canon_root.join("yarn.lock")) {
            manager = "yarn";
        }
        let arr = val
            .get("workspaces")
            .and_then(|w| w.as_array().or_else(|| w.get("packages")?.as_array()));
        if let Some(items) = arr {
            for item in items {
                if let Some(pat) = item.as_str()
                    && !pat.is_empty()
                    && !pat.starts_with('!')
                {
                    patterns.push(pat.to_owned());
                }
            }
        }
    }

    if patterns.is_empty() {
        return;
    }

    let pkg_dirs = expand_workspace_globs(canon_root, &patterns, 40);
    let mut pkg_lines = Vec::new();
    for (canon_dir, rel_path) in pkg_dirs {
        let Some(raw) = read_bounded_utf8(
            canon_root,
            &canon_dir.join("package.json"),
            MAX_MANIFEST_BYTES,
        ) else {
            continue;
        };
        let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        let Some(scripts_obj) = val.get("scripts").and_then(serde_json::Value::as_object) else {
            continue;
        };
        let mut script_names: Vec<&str> = scripts_obj
            .keys()
            .map(String::as_str)
            .filter(|k| {
                matches!(
                    *k,
                    "dev" | "start" | "serve" | "preview" | "watch" | "build" | "desktop"
                ) || k.starts_with("dev:")
                    || k.starts_with("start:")
                    || k.starts_with("build:")
            })
            .collect();
        script_names.sort_unstable();
        if script_names.is_empty() {
            continue;
        }
        let pkg_name = val
            .get("name")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty() && s.len() <= 64)
            .unwrap_or(&rel_path);
        pkg_lines.push(format!(
            "node-workspace-pkg: path={rel_path} name={pkg_name} scripts={}",
            script_names
                .into_iter()
                .take(6)
                .collect::<Vec<_>>()
                .join(",")
        ));
    }

    if pkg_lines.len() >= 2 {
        out.push(format!(
            "node-workspace-root: path=. manager={manager} packages={}",
            pkg_lines.len()
        ));
        out.extend(pkg_lines.into_iter().take(24));
    }
}

fn expand_workspace_globs(
    canon_root: &Path,
    patterns: &[String],
    max_dirs: usize,
) -> Vec<(PathBuf, String)> {
    let mut seen = HashSet::new();
    let mut results = Vec::new();

    for raw_pat in patterns {
        if results.len() >= max_dirs {
            break;
        }
        let pat = raw_pat
            .trim()
            .trim_start_matches("./")
            .trim_end_matches('/');
        if pat.is_empty()
            || pat.contains("..")
            || pat.starts_with('/')
            || pat.starts_with('~')
            || Path::new(pat).is_absolute()
        {
            continue;
        }
        if let Some(prefix) = pat.strip_suffix("/*").or_else(|| pat.strip_suffix("/**")) {
            let base = if prefix.is_empty() {
                canon_root.to_path_buf()
            } else {
                let Some((canon_prefix, _)) =
                    canonical_subdir_inside(canon_root, &canon_root.join(prefix))
                else {
                    continue;
                };
                canon_prefix
            };
            let Ok(entries) = fs::read_dir(&base) else {
                continue;
            };
            let mut children: Vec<PathBuf> =
                entries.filter_map(Result::ok).map(|e| e.path()).collect();
            children.sort();
            for child in children {
                if results.len() >= max_dirs {
                    break;
                }
                let Some(name) = child.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };
                if name.starts_with('.') || IGNORED_DIRS.contains(&name) {
                    continue;
                }
                if let Some((canon, rel)) = canonical_subdir_inside(canon_root, &child)
                    && !rel.is_empty()
                    && seen.insert(canon.clone())
                {
                    results.push((canon, rel));
                }
            }
        } else if !pat.contains('*')
            && let Some((canon, rel)) = canonical_subdir_inside(canon_root, &canon_root.join(pat))
            && !rel.is_empty()
            && seen.insert(canon.clone())
        {
            results.push((canon, rel));
        }
    }

    results.sort_by(|a, b| a.1.cmp(&b.1));
    results
}

// ---------------------------------------------------------------------------
// Cargo workspace scanner
// ---------------------------------------------------------------------------

fn scan_cargo_workspace(canon_root: &Path, out: &mut Vec<String>) {
    let Some(raw) = read_bounded_utf8(
        canon_root,
        &canon_root.join("Cargo.toml"),
        MAX_MANIFEST_BYTES,
    ) else {
        return;
    };
    if !raw.contains("[workspace]") {
        return;
    }
    let Some(ws_start) = raw.find("[workspace]") else {
        return;
    };
    let ws_section = &raw[ws_start..];
    let Some(mem_start) = ws_section.find("members") else {
        return;
    };
    let after_mem = &ws_section[mem_start..];
    let Some(open_bracket) = after_mem.find('[') else {
        return;
    };
    let Some(close_bracket) = after_mem[open_bracket..].find(']') else {
        return;
    };
    let patterns =
        extract_quoted_tokens(&after_mem[open_bracket + 1..open_bracket + close_bracket]);
    if patterns.is_empty() {
        return;
    }

    let dirs = expand_workspace_globs(canon_root, &patterns, 32);
    let mut bins = Vec::new();
    for (canon_dir, rel_path) in dirs {
        let Some(member_toml) = read_bounded_utf8(
            canon_root,
            &canon_dir.join("Cargo.toml"),
            MAX_MANIFEST_BYTES,
        ) else {
            continue;
        };
        let has_bin = is_regular_file_inside(canon_root, &canon_dir.join("src/main.rs"))
            || member_toml.contains("[[bin]]");
        if !has_bin {
            continue;
        }
        let pkg_name = extract_toml_package_name(&member_toml).unwrap_or_else(|| rel_path.clone());
        bins.push(format!("cargo-bin: path={rel_path} package={pkg_name}"));
    }

    if !bins.is_empty() {
        out.push(format!(
            "cargo-workspace-root: path=. runnableBins={}",
            bins.len()
        ));
        out.extend(bins.into_iter().take(24));
    }
}

fn extract_toml_package_name(toml: &str) -> Option<String> {
    let mut in_package = false;
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if in_package && let Some(rest) = trimmed.strip_prefix("name") {
            let rest = rest.trim_start();
            if let Some(after_eq) = rest.strip_prefix('=') {
                let val = after_eq.trim().trim_matches(|c| c == '"' || c == '\'');
                if is_safe_identifier(val) {
                    return Some(val.to_owned());
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Go workspace scanner
// ---------------------------------------------------------------------------

fn scan_go_workspace(canon_root: &Path, out: &mut Vec<String>) {
    let Some(raw) = read_bounded_utf8(canon_root, &canon_root.join("go.work"), MAX_MANIFEST_BYTES)
    else {
        return;
    };
    let mut dirs = Vec::new();
    let mut in_use_block = false;
    for raw_line in raw.lines() {
        let line = raw_line.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("use (") || line == "use(" {
            in_use_block = true;
            continue;
        }
        if in_use_block {
            if line == ")" {
                in_use_block = false;
                continue;
            }
            dirs.push(line.trim_matches('"').to_owned());
        } else if let Some(rest) = line.strip_prefix("use ") {
            dirs.push(rest.trim().trim_matches('"').to_owned());
        }
    }

    let mut mods = Vec::new();
    for raw_dir in dirs {
        let pat = raw_dir.trim_start_matches("./");
        if pat.is_empty()
            || pat.contains("..")
            || pat.starts_with('/')
            || Path::new(pat).is_absolute()
        {
            continue;
        }
        let Some((canon_dir, rel_path)) =
            canonical_subdir_inside(canon_root, &canon_root.join(pat))
        else {
            continue;
        };
        let mut mains = Vec::new();
        if is_regular_file_inside(canon_root, &canon_dir.join("main.go")) {
            mains.push(".".to_owned());
        }
        if let Some((cmd_dir, _)) = canonical_subdir_inside(canon_root, &canon_dir.join("cmd"))
            && let Ok(entries) = fs::read_dir(&cmd_dir)
        {
            let mut cmd_names: Vec<String> = entries
                .filter_map(Result::ok)
                .filter_map(|e| {
                    let (child_canon, _) = canonical_subdir_inside(canon_root, &e.path())?;
                    is_regular_file_inside(canon_root, &child_canon.join("main.go"))
                        .then(|| format!("cmd/{}", e.file_name().to_string_lossy()))
                })
                .collect();
            cmd_names.sort();
            mains.extend(cmd_names);
        }
        if !mains.is_empty() {
            mods.push(format!(
                "go-module: path={rel_path} mains={}",
                mains.join(",")
            ));
        }
    }
    if !mods.is_empty() {
        out.push(format!("go-workspace-root: path=. modules={}", mods.len()));
        out.extend(mods.into_iter().take(24));
    }
}
