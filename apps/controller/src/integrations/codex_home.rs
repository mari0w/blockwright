use std::path::{Path, PathBuf};

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

pub async fn prepare_project_codex_home(
    runtime_home: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tokio::fs::create_dir_all(runtime_home).await?;
    sync_template(runtime_home).await?;
    ensure_auth_link(runtime_home).await?;
    Ok(())
}

pub fn packaged_skill_count() -> usize {
    PACKAGED_CODEX_HOME_FILES
        .iter()
        .filter(|(path, _)| path.ends_with("/SKILL.md"))
        .count()
}

async fn sync_template(
    runtime_home: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let runtime_skills = runtime_home.join("skills");
    if tokio::fs::try_exists(&runtime_skills).await? {
        tokio::fs::remove_dir_all(&runtime_skills).await?;
    }

    for (relative_path, content) in PACKAGED_CODEX_HOME_FILES {
        let target_path = runtime_home.join(relative_path);
        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(target_path, content).await?;
    }
    Ok(())
}

async fn ensure_auth_link(
    runtime_home: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let target = runtime_home.join("auth.json");
    if tokio::fs::try_exists(&target).await? {
        return Ok(());
    }

    let Some(source) = host_auth_json_path() else {
        tracing::warn!(
            runtime_home = %runtime_home.display(),
            "host codex auth.json was not found; codex cli may require login"
        );
        return Ok(());
    };

    link_or_copy_auth(&source, &target)?;
    tracing::info!(
        source = %source.display(),
        target = %target.display(),
        "prepared isolated codex auth link"
    );
    Ok(())
}

fn host_auth_json_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("BLOCKWRIGHT_CODEX_AUTH_JSON") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }

    let home = std::env::var("HOME").ok()?;
    let path = PathBuf::from(home).join(".codex").join("auth.json");
    path.exists().then_some(path)
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

    #[test]
    fn packages_blockwright_skills() {
        assert!(packaged_skill_count() >= 5);
        assert!(PACKAGED_CODEX_HOME_FILES.iter().any(|(path, content)| path
            == &"skills/blockwright-build-planning/SKILL.md"
            && content.contains("Blockwright Building Planning")));
    }
}
