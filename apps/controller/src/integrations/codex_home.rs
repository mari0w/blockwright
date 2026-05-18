use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const PACKAGED_SKILLS_MANIFEST: &str = ".blockwright-packaged-skills.json";

const PACKAGED_CODEX_HOME_FILES: &[(&str, &str)] = &[
    (
        "skills/blockwright-build-planning/SKILL.md",
        include_str!("../../codex-home-template/skills/blockwright-build-planning/SKILL.md"),
    ),
    (
        "skills/blockwright-site-selection/SKILL.md",
        include_str!("../../codex-home-template/skills/blockwright-site-selection/SKILL.md"),
    ),
    (
        "skills/blockwright-blueprint-verification/SKILL.md",
        include_str!(
            "../../codex-home-template/skills/blockwright-blueprint-verification/SKILL.md"
        ),
    ),
    (
        "skills/blockwright-existing-build-edit/SKILL.md",
        include_str!("../../codex-home-template/skills/blockwright-existing-build-edit/SKILL.md"),
    ),
    (
        "skills/blockwright-image-to-blueprint/SKILL.md",
        include_str!("../../codex-home-template/skills/blockwright-image-to-blueprint/SKILL.md"),
    ),
    (
        "skills/blockwright-command-actions/SKILL.md",
        include_str!("../../codex-home-template/skills/blockwright-command-actions/SKILL.md"),
    ),
];

#[derive(Debug, Default, Deserialize, Serialize)]
struct PackagedSkillsManifest {
    paths: Vec<String>,
}

pub async fn prepare_project_codex_home(
    runtime_home: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    sync_packaged_files(runtime_home, PACKAGED_CODEX_HOME_FILES).await?;
    ensure_auth_link(runtime_home).await?;
    Ok(())
}

pub fn packaged_skill_count() -> usize {
    PACKAGED_CODEX_HOME_FILES
        .iter()
        .filter(|(path, _)| path.ends_with("/SKILL.md"))
        .count()
}

async fn sync_packaged_files(
    runtime_home: &Path,
    files: &[(&str, &str)],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tokio::fs::create_dir_all(runtime_home).await?;

    let manifest_path = runtime_home.join(PACKAGED_SKILLS_MANIFEST);
    let previous_manifest = read_packaged_manifest(&manifest_path).await;
    let current_paths = files
        .iter()
        .map(|(relative_path, _)| {
            safe_runtime_child(runtime_home, relative_path)
                .map(|_| relative_path.to_string())
                .ok_or_else(|| format!("unsafe packaged codex home path: {relative_path}"))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;

    let runtime_skills = runtime_home.join("skills");
    for previous_path in previous_manifest.paths {
        if current_paths.contains(&previous_path) {
            continue;
        }
        let Some(target_path) = safe_runtime_child(runtime_home, &previous_path) else {
            continue;
        };
        if tokio::fs::remove_file(&target_path).await.is_ok() {
            prune_empty_skill_dirs(target_path.parent(), &runtime_skills).await;
        }
    }

    for (relative_path, content) in files {
        let target_path = safe_runtime_child(runtime_home, relative_path)
            .ok_or_else(|| format!("unsafe packaged codex home path: {relative_path}"))?;
        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if tokio::fs::read_to_string(&target_path)
            .await
            .is_ok_and(|existing| existing == *content)
        {
            continue;
        }
        tokio::fs::write(target_path, content).await?;
    }

    let manifest = PackagedSkillsManifest {
        paths: current_paths.into_iter().collect(),
    };
    tokio::fs::write(manifest_path, serde_json::to_string_pretty(&manifest)?).await?;
    Ok(())
}

async fn read_packaged_manifest(path: &Path) -> PackagedSkillsManifest {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => PackagedSkillsManifest::default(),
    }
}

async fn prune_empty_skill_dirs(start: Option<&Path>, skills_root: &Path) {
    let Some(mut current) = start.map(Path::to_path_buf) else {
        return;
    };

    while current != skills_root && current.starts_with(skills_root) {
        if tokio::fs::remove_dir(&current).await.is_err() {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent.to_path_buf();
    }
}

fn safe_runtime_child(runtime_home: &Path, relative_path: &str) -> Option<PathBuf> {
    let path = Path::new(relative_path);
    if path.is_absolute() {
        return None;
    }

    let mut components = path.components();
    let first = components.next()?;
    if !matches!(first, Component::Normal(name) if name == "skills") {
        return None;
    }
    if components.any(|component| !matches!(component, Component::Normal(_))) {
        return None;
    }

    Some(runtime_home.join(path))
}

async fn ensure_auth_link(
    runtime_home: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let target = runtime_home.join("auth.json");
    let Some(source) = host_auth_json_path() else {
        tracing::warn!(
            runtime_home = %runtime_home.display(),
            "host codex auth.json was not found; codex cli may require login"
        );
        return Ok(());
    };

    if ensure_auth_link_from_source(&source, &target)? {
        tracing::info!(
            source = %source.display(),
            target = %target.display(),
            "prepared isolated codex auth link"
        );
    }
    Ok(())
}

fn host_auth_json_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("BLOCKWRIGHT_CODEX_AUTH_JSON") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(canonical_or_original(path));
        }
    }

    let home = std::env::var("HOME").ok()?;
    let path = PathBuf::from(home).join(".codex").join("auth.json");
    path.exists().then(|| canonical_or_original(path))
}

fn canonical_or_original(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

#[cfg(unix)]
fn ensure_auth_link_from_source(source: &Path, target: &Path) -> std::io::Result<bool> {
    match std::fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if std::fs::read_link(target).is_ok_and(|link| link == source) {
                return Ok(false);
            }
            std::fs::remove_file(target)?;
        }
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    link_or_copy_auth(source, target)?;
    Ok(true)
}

#[cfg(not(unix))]
fn ensure_auth_link_from_source(source: &Path, target: &Path) -> std::io::Result<bool> {
    if target.exists() {
        return Ok(false);
    }
    link_or_copy_auth(source, target)?;
    Ok(true)
}

#[cfg(unix)]
fn link_or_copy_auth(source: &Path, target: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(not(unix))]
fn link_or_copy_auth(source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::copy(source, target).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_DIR_ID: AtomicU64 = AtomicU64::new(1);

    fn temp_dir(name: &str) -> PathBuf {
        let number = NEXT_DIR_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "blockwright-codex-home-{name}-{}-{number}",
            std::process::id()
        ))
    }

    #[test]
    fn packages_blockwright_skills() {
        assert!(packaged_skill_count() >= 5);
        assert!(PACKAGED_CODEX_HOME_FILES.iter().any(|(path, content)| path
            == &"skills/blockwright-build-planning/SKILL.md"
            && content.contains("Blockwright Building Planning")));
    }

    #[tokio::test]
    async fn sync_preserves_unmanaged_skills_and_updates_packaged_content() {
        let dir = temp_dir("preserve-unmanaged");
        sync_packaged_files(&dir, &[("skills/managed/SKILL.md", "old")])
            .await
            .unwrap();
        let unmanaged = dir.join("skills").join("local-only").join("SKILL.md");
        fs::create_dir_all(unmanaged.parent().unwrap()).unwrap();
        fs::write(&unmanaged, "custom").unwrap();

        sync_packaged_files(&dir, &[("skills/managed/SKILL.md", "new")])
            .await
            .unwrap();

        assert_eq!(
            fs::read_to_string(dir.join("skills/managed/SKILL.md")).unwrap(),
            "new"
        );
        assert_eq!(fs::read_to_string(unmanaged).unwrap(), "custom");
    }

    #[tokio::test]
    async fn sync_removes_only_stale_packaged_files() {
        let dir = temp_dir("remove-stale");
        sync_packaged_files(&dir, &[("skills/old/SKILL.md", "old")])
            .await
            .unwrap();
        sync_packaged_files(&dir, &[("skills/new/SKILL.md", "new")])
            .await
            .unwrap();

        assert!(!dir.join("skills/old/SKILL.md").exists());
        assert_eq!(
            fs::read_to_string(dir.join("skills/new/SKILL.md")).unwrap(),
            "new"
        );
    }

    #[cfg(unix)]
    #[test]
    fn auth_link_updates_wrong_symlink() {
        let dir = temp_dir("auth-link");
        fs::create_dir_all(&dir).unwrap();
        let old_auth = dir.join("old-auth.json");
        let new_auth = dir.join("new-auth.json");
        let target = dir.join("auth.json");
        fs::write(&old_auth, "{}").unwrap();
        fs::write(&new_auth, "{}").unwrap();
        std::os::unix::fs::symlink(&old_auth, &target).unwrap();

        assert!(ensure_auth_link_from_source(&new_auth, &target).unwrap());
        assert_eq!(fs::read_link(&target).unwrap(), new_auth);
        assert!(!ensure_auth_link_from_source(&new_auth, &target).unwrap());
    }
}
