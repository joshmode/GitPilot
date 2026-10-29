//! Structured local command execution. There is intentionally no free-form
//! shell parser: callers provide an executable and argument vector.

use crate::domain::PermissionPolicy;
use crate::domain::{CommandResult, CommandSpec};
use crate::error::{GitPilotError, Result, redact_secrets};
use crate::permissions::{Permission, require};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

#[cfg(target_os = "macos")]
fn sandbox_profile() -> &'static str {
    "(version 1)
     (allow process-exec)
     (allow process-fork)
     (allow file-read*)
     (allow file-write* (subpath \"/tmp\") (subpath \"/private/tmp\") (subpath \"/private/var/folders\"))
     (deny network*))"
}

pub const DEFAULT_TIMEOUT_SECONDS: u64 = 180;
pub const MAX_TIMEOUT_SECONDS: u64 = 900;
pub const MAX_OUTPUT_BYTES: usize = 512 * 1024;
pub const MAX_ARGUMENTS: usize = 128;
pub const MAX_ARGUMENT_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone)]
pub struct CommandPolicy {
    pub allowed_executables: Vec<String>,
    pub allow_network: bool,
    pub allow_file_writes: bool,
    pub timeout_seconds: u64,
}

impl Default for CommandPolicy {
    fn default() -> Self {
        Self {
            allowed_executables: vec![
                "cargo".into(),
                "git".into(),
                "npm".into(),
                "pnpm".into(),
                "yarn".into(),
                "go".into(),
                "pytest".into(),
                "python".into(),
                "rustc".into(),
                "rustfmt".into(),
                "clippy-driver".into(),
            ],
            allow_network: false,
            allow_file_writes: false,
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
        }
    }
}

#[derive(Clone)]
pub struct CommandRunner {
    policy: CommandPolicy,
}

impl CommandRunner {
    pub fn new(policy: CommandPolicy) -> Self {
        Self { policy }
    }

    pub fn policy(&self) -> &CommandPolicy {
        &self.policy
    }

    pub async fn run(
        &self,
        spec: &CommandSpec,
        permissions: &PermissionPolicy,
        cancel: &CancellationToken,
    ) -> Result<CommandResult> {
        require(permissions, Permission::LocalExecution)?;
        if spec.executable.trim().is_empty()
            || spec.executable.contains('/')
            || spec.executable.contains('\\')
            || spec.executable.contains('\0')
        {
            return Err(GitPilotError::Validation(
                "executable must be a bare allow-listed command name".into(),
            ));
        }
        if spec.args.len() > MAX_ARGUMENTS
            || spec
                .args
                .iter()
                .any(|arg| arg.len() > MAX_ARGUMENT_BYTES || arg.contains('\0'))
        {
            return Err(GitPilotError::Validation(
                "command arguments exceed the safety limits".into(),
            ));
        }
        if !self
            .policy
            .allowed_executables
            .iter()
            .any(|allowed| allowed == &spec.executable)
        {
            return Err(GitPilotError::PermissionDenied(format!(
                "executable `{}` is not on the allow-list",
                spec.executable
            )));
        }
        if spec.network && !self.policy.allow_network {
            return Err(GitPilotError::PermissionDenied(
                "network access is disabled for local commands".into(),
            ));
        }
        if spec.modifies_files && !self.policy.allow_file_writes {
            return Err(GitPilotError::PermissionDenied(
                "file-modifying commands are disabled".into(),
            ));
        }
        if spec.privileged {
            return Err(GitPilotError::PermissionDenied(
                "privileged execution is not supported by the host process guard".into(),
            ));
        }
        let workdir = canonical_workdir(&spec.working_directory).await?;
        let timeout = spec
            .timeout_seconds
            .min(self.policy.timeout_seconds)
            .clamp(1, MAX_TIMEOUT_SECONDS);
        #[cfg(target_os = "macos")]
        let mut command = if self.policy.allow_network {
            Command::new(&spec.executable)
        } else {
            let mut command = Command::new("/usr/bin/sandbox-exec");
            command.args(["-p", sandbox_profile()]);
            command
        };
        #[cfg(not(target_os = "macos"))]
        let mut command = Command::new(&spec.executable);
        #[cfg(target_os = "macos")]
        if !self.policy.allow_network {
            command.arg(&spec.executable);
        }
        command
            .args(&spec.args)
            .current_dir(&workdir)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", workdir.as_os_str())
            .env("CI", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let started = std::time::Instant::now();
        let child = command.spawn().map_err(|error| {
            GitPilotError::Git(format!("could not start {}: {error}", spec.executable))
        })?;
        let token = cancel.clone();
        let task = run_child(child, token);
        let timed_out = tokio::time::timeout(std::time::Duration::from_secs(timeout), task).await;
        let (timed_out, output) = match timed_out {
            Ok(result) => (false, result?),
            Err(_) => {
                return Err(GitPilotError::Git(format!(
                    "command timed out after {timeout} seconds"
                )));
            }
        };
        let stdout = bounded_string(&output.stdout);
        let stderr = bounded_string(&output.stderr);
        Ok(CommandResult {
            command: format!("{} {}", spec.executable, spec.args.join(" ")),
            exit_code: output.status.code(),
            stdout: redact_secrets(&stdout),
            stderr: redact_secrets(&stderr),
            duration_ms: started.elapsed().as_millis() as i64,
            timed_out,
            truncated: output.stdout.len() > MAX_OUTPUT_BYTES
                || output.stderr.len() > MAX_OUTPUT_BYTES,
        })
    }
}

async fn run_child(
    child: tokio::process::Child,
    token: CancellationToken,
) -> Result<std::process::Output> {
    let mut child = child;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| GitPilotError::Git("command stdout pipe unavailable".into()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| GitPilotError::Git("command stderr pipe unavailable".into()))?;
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

fn bounded_string(bytes: &[u8]) -> String {
    let truncated = &bytes[..bytes.len().min(MAX_OUTPUT_BYTES)];
    String::from_utf8_lossy(truncated).into_owned()
}

async fn canonical_workdir(path: &str) -> Result<PathBuf> {
    let path = Path::new(path);
    let canonical = tokio::fs::canonicalize(path).await.map_err(|_| {
        GitPilotError::Validation(format!(
            "working directory does not exist: {}",
            path.display()
        ))
    })?;
    let metadata = tokio::fs::metadata(&canonical).await?;
    if !metadata.is_dir() {
        return Err(GitPilotError::Validation(
            "working directory is not a directory".into(),
        ));
    }
    if let Some(home) = directories_home()
        && canonical == home
    {
        return Err(GitPilotError::PermissionDenied(
            "refusing to run commands with the home directory as the workspace".into(),
        ));
    }
    Ok(canonical)
}

fn directories_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub fn validate_relative_path(base: &Path, candidate: &Path) -> Result<PathBuf> {
    if candidate.is_absolute() {
        return Err(GitPilotError::PermissionDenied(
            "absolute paths are not accepted in repository tools".into(),
        ));
    }
    if candidate
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(GitPilotError::PermissionDenied(
            "path traversal is not accepted in repository tools".into(),
        ));
    }
    Ok(base.join(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal() {
        let base = Path::new("/tmp/work");
        assert!(validate_relative_path(base, Path::new("../etc/passwd")).is_err());
        assert!(validate_relative_path(base, Path::new("src/lib.rs")).is_ok());
    }

    #[test]
    fn output_is_bounded() {
        let bytes = vec![b'x'; MAX_OUTPUT_BYTES + 10];
        assert_eq!(bounded_string(&bytes).len(), MAX_OUTPUT_BYTES);
    }

    #[tokio::test]
    async fn rejects_path_executables_and_privileged_commands() {
        let runner = CommandRunner::new(CommandPolicy::default());
        let policy = PermissionPolicy {
            local_execution: true,
            ..Default::default()
        };
        let cancel = CancellationToken::new();
        let mut spec = CommandSpec {
            executable: "/bin/sh".into(),
            args: vec!["-c".into(), "echo unsafe".into()],
            working_directory: "/tmp".into(),
            network: false,
            modifies_files: false,
            privileged: false,
            timeout_seconds: 5,
            description: "probe".into(),
        };
        assert!(matches!(
            runner.run(&spec, &policy, &cancel).await,
            Err(GitPilotError::Validation(_))
        ));
        spec.executable = "git".into();
        spec.privileged = true;
        assert!(matches!(
            runner.run(&spec, &policy, &cancel).await,
            Err(GitPilotError::PermissionDenied(_))
        ));
    }
}
