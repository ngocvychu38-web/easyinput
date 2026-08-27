use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledApplication {
    pub name: String,
    pub path: String,
}

pub fn list_installed_applications() -> Result<Vec<InstalledApplication>, String> {
    #[cfg(target_os = "macos")]
    {
        let mut paths = BTreeSet::new();
        if let Ok(output) = std::process::Command::new("/usr/bin/mdfind")
            .arg("kMDItemContentType == 'com.apple.application-bundle'")
            .output()
        {
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    add_application_path(Path::new(line.trim()), &mut paths);
                }
            }
        }

        let mut roots = vec![PathBuf::from("/Applications"), PathBuf::from("/System/Applications")];
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join("Applications"));
        }
        for root in roots {
            collect_applications(&root, 2, &mut paths);
        }

        let mut applications: Vec<_> = paths
            .into_iter()
            .filter_map(|path| {
                let name = application_name(&path)?;
                Some(InstalledApplication { name, path: path.to_string_lossy().into_owned() })
            })
            .collect();
        applications.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.path.cmp(&right.path))
        });
        return Ok(applications);
    }

    #[cfg(not(target_os = "macos"))]
    Ok(Vec::new())
}

#[cfg(target_os = "macos")]
fn add_application_path(path: &Path, output: &mut BTreeSet<PathBuf>) {
    let is_app = path.extension().and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case("app"));
    if !is_app || !path.is_dir() {
        return;
    }
    let nested_app_count = path
        .components()
        .filter(|component| Path::new(component.as_os_str()).extension().and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case("app")))
        .count();
    if nested_app_count == 1 {
        output.insert(path.to_path_buf());
    }
}

#[cfg(target_os = "macos")]
fn collect_applications(root: &Path, depth: usize, output: &mut BTreeSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case("app")) {
            add_application_path(&path, output);
        } else if depth > 0 && path.is_dir() {
            collect_applications(&path, depth - 1, output);
        }
    }
}

fn application_name(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy();
    let name = file_name.strip_suffix(".app").or_else(|| file_name.strip_suffix(".APP")).unwrap_or(&file_name);
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_application_name_from_bundle_path() {
        assert_eq!(application_name(Path::new("/Applications/Safari.app")).as_deref(), Some("Safari"));
        assert_eq!(application_name(Path::new("/Applications/微信.app")).as_deref(), Some("微信"));
    }
}
