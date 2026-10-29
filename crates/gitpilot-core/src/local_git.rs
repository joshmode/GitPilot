//! Read-oriented local Git workspace. Commands are argument vectors and never
//! passed through a shell.

use crate::domain::{LocalRepository, LocalStatus};
use crate::error::{GitPilotError, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
pub struct LocalGit;

impl LocalGit {
    pub async fn inspect(
        path: impl AsRef<Path>,
        cancel: &CancellationToken,
    ) -> Result<LocalRepository> {
        let path = path.as_ref();
        let canonical = tokio::fs::canonicalize(path)
            .await
            .map_err(|_| GitPilotError::Git("local repository path does not exist".into()))?;
        let git_dir = canonical.join(".git");
        if !git_dir.exists() {
            return Err(GitPilotError::Git(
                "selected directory is not a Git working tree".into(),
            ));
        }
        let default_branch = git_output(
            &canonical.to_string_lossy(),
            &["symbolic-ref", "--short", "HEAD"],
            cancel,
        )
        .await
        .unwrap_or_else(|_| "main".into());
        let name = canonical
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("repository")
            .to_string();
        Ok(LocalRepository {
            id: format!("local-{}", stable_hash(&canonical.to_string_lossy())),
            name,
            path: canonical.to_string_lossy().into_owned(),
            default_branch: default_branch.trim().to_string(),
            connected_at: chrono::Utc::now(),
            last_opened_at: Some(chrono::Utc::now()),
        })
    }

    pub async fn status(path: &str, cancel: &CancellationToken) -> Result<LocalStatus> {
        let output = git_output(path, &["status", "--porcelain=v1", "--branch"], cancel).await?;
        let mut status = LocalStatus {
            branch: "unknown".into(),
            ahead: 0,
            behind: 0,
            staged: 0,
            modified: 0,
            untracked: 0,
            conflicted: 0,
            entries: vec![],
        };
        for line in output.lines() {
            if let Some(branch) = line.strip_prefix("## ") {
                status.branch = branch.split("...").next().unwrap_or(branch).to_string();
                if let Some(ahead) = branch.split("ahead ").nth(1) {
                    status.ahead = ahead
                        .split(',')
                        .next()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                }
                if let Some(behind) = branch.split("behind ").nth(1) {
                    status.behind = behind
                        .split(']')
                        .next()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                }
                continue;
            }
            if line.len() < 3 {
                continue;
            }
            status.entries.push(line.into());
            let code = &line[..2];
            if code.contains('?') {
                status.untracked += 1;
            }
            if code.contains('U') {
                status.conflicted += 1;
            } else {
                if code.chars().next().is_some_and(|c| c != ' ') {
                    status.staged += 1;
                }
                if code.chars().nth(1).is_some_and(|c| c != ' ') {
                    status.modified += 1;
                }
            }
        }
        Ok(status)
    }

    pub async fn recent_commits(
        path: &str,
        limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<String>> {
        let size = limit.clamp(1, 100).to_string();
        let output = git_output(
            path,
            &[
                "log",
                "-n",
                &size,
                "--date=iso-strict",
                "--pretty=format:%h%x09%an%x09%ad%x09%s",
            ],
            cancel,
        )
        .await?;
        Ok(output.lines().map(str::to_string).collect())
    }

    pub async fn branches(path: &str, cancel: &CancellationToken) -> Result<Vec<String>> {
        Ok(
            git_output(path, &["branch", "--format=%(refname:short)"], cancel)
                .await?
                .lines()
                .filter(|v| !v.trim().is_empty())
                .map(str::to_string)
                .collect(),
        )
    }

    pub async fn refs(path: &str, cancel: &CancellationToken) -> Result<Vec<String>> {
        Ok(git_output(path, &["tag", "--sort=-creatordate"], cancel)
            .await?
            .lines()
            .take(50)
            .map(str::to_string)
            .collect())
    }

    pub async fn diff(
        path: &str,
        left: &str,
        right: &str,
        cancel: &CancellationToken,
    ) -> Result<String> {
        validate_revision(left)?;
        validate_revision(right)?;
        // Revisions must be a range, not pathspec entries. Without `--`, a
        // revision containing a crafted path cannot be interpreted as a file.
        git_output(
            path,
            &["diff", "--no-ext-diff", &format!("{left}...{right}")],
            cancel,
        )
        .await
    }

    pub async fn compare(
        path: &str,
        left: &str,
        right: &str,
        cancel: &CancellationToken,
    ) -> Result<String> {
        validate_revision(left)?;
        validate_revision(right)?;
        let range = format!("{left}...{right}");
        git_output(path, &["diff", "--stat", &range], cancel).await
    }

    pub async fn search(
        path: &str,
        needle: &str,
        cancel: &CancellationToken,
    ) -> Result<Vec<String>> {
        if needle.len() < 2 || needle.len() > 100 {
            return Err(GitPilotError::InvalidInput(
                "search term must be between 2 and 100 characters".into(),
            ));
        }
        let output = git_output(path, &["grep", "-n", "-I", "-F", "--", needle], cancel)
            .await
            .unwrap_or_default();
        Ok(output.lines().take(200).map(str::to_string).collect())
    }
}

fn validate_revision(revision: &str) -> Result<()> {
    if revision.is_empty()
        || revision.len() > 200
        || revision.starts_with('-')
        || revision.contains('\0')
        || revision.contains("..")
    {
        return Err(GitPilotError::InvalidInput(
            "invalid Git revision reference".into(),
        ));
    }
    Ok(())
}

async fn git_output(path: &str, args: &[&str], cancel: &CancellationToken) -> Result<String> {
    let canonical = tokio::fs::canonicalize(path)
        .await
        .map_err(|_| GitPilotError::Git("repository path does not exist".into()))?;
    if !canonical.join(".git").exists() {
        return Err(GitPilotError::Git("not a Git repository".into()));
    }
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(&canonical)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let child = command
        .spawn()
        .map_err(|error| GitPilotError::Git(format!("could not run git: {error}")))?;
    let token = cancel.clone();
    let task = run_git_child(child, token);
    let output = match tokio::time::timeout(std::time::Duration::from_secs(30), task).await {
        Ok(result) => result?,
        Err(_) => return Err(GitPilotError::Git("git command timed out".into())),
    };
    if !output.status.success() {
        return Err(GitPilotError::Git(
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(500)
                .collect(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn run_git_child(
    child: tokio::process::Child,
    token: CancellationToken,
) -> Result<std::process::Output> {
    let mut child = child;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| GitPilotError::Git("git stdout pipe unavailable".into()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| GitPilotError::Git("git stderr pipe unavailable".into()))?;
    let stdout_task = tokio::spawn(async move {
        let mut buffer = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut stdout, &mut buffer).await?;
        Ok::<_, std::io::Error>(buffer)
    });
    let stderr_task = tokio::spawn(async move {
        let mut buffer = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut stderr, &mut buffer).await?;
        Ok::<_, std::io::Error>(buffer)
    });
    if token.is_cancelled() {
        let _ = child.start_kill();
        let _ = child.wait().await;
        return Err(GitPilotError::Cancelled);
    }
    let status = tokio::select! {
        _ = token.cancelled() => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            Err(GitPilotError::Cancelled)
        }
        result = child.wait() => {
            result.map_err(GitPilotError::Io)
        }
    }?;
    let stdout = stdout_task
        .await
        .map_err(|error| GitPilotError::Git(error.to_string()))??;
    let stderr = stderr_task
        .await
        .map_err(|error| GitPilotError::Git(error.to_string()))??;
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn stable_hash(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(value.as_bytes());
    hex::encode(&digest[..8])
}

pub fn path_is_inside(root: &Path, path: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    path.starts_with(root)
}

/// Quick check used before launching a local command. This is paired with the
/// OS-level inherited-socket restriction where supported.
pub fn command_network_policy(requested_network: bool, allowed_network: bool) -> Result<()> {
    if requested_network && !allowed_network {
        Err(GitPilotError::PermissionDenied(
            "network access is disabled for local commands".into(),
        ))
    } else {
        Ok(())
    }
}

pub fn repository_name_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("repository")
        .to_string()
}

pub fn default_repo_root() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|p| p.join("Projects"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_option_like_revisions() {
        assert!(validate_revision("--upload-pack=evil").is_err());
        assert!(validate_revision("HEAD~1").is_ok());
        assert!(validate_revision("main..evil").is_err());
    }
}
