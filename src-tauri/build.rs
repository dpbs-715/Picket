use std::{fs, path::Path};

fn extract_package_dependency_version(package_json: &str, dep_name: &str) -> String {
    let needle = format!("\"{dep_name}\"");
    for line in package_json.lines() {
        let Some(after_key) = line.split_once(&needle).map(|(_, rest)| rest) else {
            continue;
        };
        let Some((_, after_colon)) = after_key.split_once(':') else {
            continue;
        };
        let trimmed = after_colon.trim().trim_end_matches(',').trim();
        if let Some(version) = trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
            && !version.is_empty()
        {
            return version.to_owned();
        }
    }
    panic!("Unable to locate `{dep_name}` version in ../package.json");
}

fn main() {
    let package_json_path = Path::new("../package.json");
    println!("cargo:rerun-if-changed={}", package_json_path.display());
    let package_json = fs::read_to_string(package_json_path)
        .expect("Failed to read ../package.json for Pi Core version");
    let pi_core_version =
        extract_package_dependency_version(&package_json, "@earendil-works/pi-agent-core");
    println!("cargo:rustc-env=PICKET_PI_CORE_VERSION={pi_core_version}");
    println!(
        "cargo:rustc-env=PICKET_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
    tauri_build::build();
}
