//! Structured local command execution. There is intentionally no free-form
//! shell parser: callers provide an executable and argument vector.

use crate::domain::PermissionPolicy;
use crate::domain::{AppSettings, CommandResult, CommandSpec};
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
    pub sandbox_backend: SandboxBackend,
    pub sandbox_image: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackend {
    Host,
    Docker,
    Podman,
}

impl SandboxBackend {
    fn executable(self) -> Option<&'static str> {
        match self {
            Self::Host => None,
            Self::Docker => Some("docker"),
            Self::Podman => Some("podman"),
        }
    }
}

impl CommandPolicy {
    pub fn sandbox_backend_name(&self) -> &'static str {
        match self.sandbox_backend {
            SandboxBackend::Host => "host process guard",
            SandboxBackend::Docker => "docker container",
            SandboxBackend::Podman => "podman container",
        }
    }
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
            sandbox_backend: SandboxBackend::Host,
            sandbox_image: None,
        }
    }
}

impl CommandPolicy {
    pub fn from_settings(settings: &AppSettings) -> Self {
        let mut policy = Self {
            allow_network: settings.sandbox_allow_network,
            allow_file_writes: settings.sandbox_allow_file_writes,
            sandbox_backend: match settings.sandbox_backend.as_str() {
                "docker" => SandboxBackend::Docker,
                "podman" => SandboxBackend::Podman,
                _ => SandboxBackend::Host,
            },
            sandbox_image: (!settings.sandbox_image.trim().is_empty())
                .then(|| settings.sandbox_image.trim().to_owned()),
            ..Self::default()
        };
        let allowed = settings
            .allowed_executables
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .filter(|value| !value.contains('/') && !value.contains('\\') && !value.contains('\0'))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !allowed.is_empty() {
            policy.allowed_executables = allowed;
        }
        policy
    }
}

#[derive(Clone)]
pub struct CommandRunner {
    policy: std::sync::Arc<parking_lot::RwLock<CommandPolicy>>,
}

impl CommandRunner {
    pub fn new(policy: CommandPolicy) -> Self {
        Self {
            policy: std::sync::Arc::new(parking_lot::RwLock::new(policy)),
        }
    }

    pub fn policy(&self) -> CommandPolicy {
        self.policy.read().clone()
    }

    pub fn set_policy(&mut self, policy: CommandPolicy) {
        *self.policy.write() = policy;
    }

    fn snapshot_policy(&self) -> CommandPolicy {
        self.policy.read().clone()
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
        let policy = self.snapshot_policy();
        if !policy
            .allowed_executables
            .iter()
            .any(|allowed| allowed == &spec.executable)
        {
            return Err(GitPilotError::PermissionDenied(format!(
                "executable `{}` is not on the allow-list",
                spec.executable
            )));
        }
        if spec.network && !policy.allow_network {
            return Err(GitPilotError::PermissionDenied(
                "network access is disabled for local commands".into(),
            ));
        }
        if spec.modifies_files && !policy.allow_file_writes {
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
        if let Some(runtime) = policy.sandbox_backend.executable() {
            let image = policy.sandbox_image.as_deref().ok_or_else(|| {
                GitPilotError::Validation(
                    "a container image must be configured before using the container sandbox"
                        .into(),
                )
            })?;
            if image.contains('\0')
                || image.contains(' ')
                || image.len() > 255
                || !image.contains(':')
                || image.starts_with('-')
            {
                return Err(GitPilotError::Validation(
                    "container image reference is invalid".into(),
                ));
            }
            return self
                .run_container(runtime, image, spec, &workdir, cancel, &policy)
                .await;
        }
        let timeout = spec
            .timeout_seconds
            .min(policy.timeout_seconds)
            .clamp(1, MAX_TIMEOUT_SECONDS);
        #[cfg(target_os = "macos")]
        let mut command = if policy.allow_network {
            Command::new(&spec.executable)
        } else {
            let mut command = Command::new("/usr/bin/sandbox-exec");
            command.args(["-p", sandbox_profile()]);
            command
        };
        #[cfg(not(target_os = "macos"))]
        let mut command = Command::new(&spec.executable);
        #[cfg(target_os = "macos")]
        if !policy.allow_network {
            command.arg(&spec.executable);
        }
        command
            .args(&spec.args)
            .current_dir(&workdir)
            .env_clear()
            .env("PATH", safe_path(&workdir))
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

    async fn run_container(
        &self,
        runtime: &str,
        image: &str,
        spec: &CommandSpec,
        workdir: &Path,
        cancel: &CancellationToken,
        policy: &CommandPolicy,
    ) -> Result<CommandResult> {
        let timeout = spec
            .timeout_seconds
            .min(policy.timeout_seconds)
            .clamp(1, MAX_TIMEOUT_SECONDS);
        let name = format!(
            "gitpilot-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..12]
        );
        let mut command = Command::new(runtime);
        let network = if spec.network && policy.allow_network {
            "bridge"
        } else {
            "none"
        };
        let writable = spec.modifies_files && policy.allow_file_writes;
        let mount_mode = if writable { "rw" } else { "ro" };
        let mut args = vec![
            "run".to_string(),
            "--rm".to_string(),
            "--name".to_string(),
            name.clone(),
            "--network".to_string(),
            network.to_string(),
            "--cap-drop".to_string(),
            "ALL".to_string(),
            "--security-opt".to_string(),
            "no-new-privileges".to_string(),
            "--read-only".to_string(),
            "--pids-limit".to_string(),
            "256".to_string(),
            "--memory".to_string(),
            "1g".to_string(),
            "--cpus".to_string(),
            "2".to_string(),
            "--tmpfs".to_string(),
            "/tmp:rw,noexec,nosuid,size=64m".to_string(),
            "--mount".to_string(),
            format!(
                "type=bind,src={},dst=/workspace,{mount_mode}",
                workdir.display()
            ),
            "--workdir".to_string(),
            "/workspace".to_string(),
            "--user".to_string(),
            "65534:65534".to_string(),
            image.to_string(),
            spec.executable.clone(),
        ];
        args.extend(spec.args.iter().cloned());
        let host_path = safe_path(workdir);
        command
            .args(&args)
            .env_clear()
            .env("PATH", &host_path)
            .env(
                "DOCKER_HOST",
                std::env::var("DOCKER_HOST").unwrap_or_default(),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let started = std::time::Instant::now();
        let child = command.spawn().map_err(|error| {
            GitPilotError::Git(format!("container runtime could not start: {error}"))
        })?;
        let task = run_child(child, cancel.clone());
        let result = tokio::time::timeout(std::time::Duration::from_secs(timeout), task).await;
        let output = match result {
            Ok(result) => result?,
            Err(_) => {
                // Best-effort cleanup after timeout. The unique runtime name
                // prevents touching another GitPilot task.
                let _ = Command::new(runtime)
                    .args(["rm", "-f", &name])
                    .env_clear()
                    .env("PATH", &host_path)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await;
                return Err(GitPilotError::Git(format!(
                    "container command timed out after {timeout} seconds"
                )));
            }
        };
        let stdout = bounded_string(&output.stdout);
        let stderr = bounded_string(&output.stderr);
        Ok(CommandResult {
            command: format!(
                "{runtime} run --network {network} {image} {} {}",
                spec.executable,
                spec.args.join(" ")
            ),
            exit_code: output.status.code(),
            stdout: redact_secrets(&stdout),
            stderr: redact_secrets(&stderr),
            duration_ms: started.elapsed().as_millis() as i64,
            timed_out: false,
            truncated: output.stdout.len() > MAX_OUTPUT_BYTES
                || output.stderr.len() > MAX_OUTPUT_BYTES,
        })
    }
}

async fn run_child(
    mut child: tokio::process::Child,
    token: CancellationToken,
) -> Result<std::process::Output> {
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| GitPilotError::Git("command stdout pipe unavailable".into()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| GitPilotError::Git("command stderr pipe unavailable".into()))?;
    let mut stdout_buffer = Vec::with_capacity(MAX_OUTPUT_BYTES);
    let mut stderr_buffer = Vec::with_capacity(MAX_OUTPUT_BYTES);
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
    tokio::io::AsyncReadExt::read_to_end(&mut stdout, &mut stdout_buffer)
        .await
        .map_err(|error| GitPilotError::Git(error.to_string()))?;
    tokio::io::AsyncReadExt::read_to_end(&mut stderr, &mut stderr_buffer)
        .await
        .map_err(|error| GitPilotError::Git(error.to_string()))?;
    Ok(std::process::Output {
        status,
        stdout: stdout_buffer,
        stderr: stderr_buffer,
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
    if canonical == std::path::Path::new("/") {
        return Err(GitPilotError::PermissionDenied(
            "refusing to use the filesystem root as a command workspace".into(),
        ));
    }
    Ok(canonical)
}

fn directories_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Drop empty, relative and workspace-relative PATH entries before execution.
/// An empty PATH segment means "current directory" on POSIX, which would let a
/// hostile repository shadow an allow-listed executable.
fn safe_path(workdir: &Path) -> std::ffi::OsString {
    let value = std::env::var_os("PATH").unwrap_or_default();
    let workdir_canonical = workdir.to_path_buf();
    std::env::split_paths(&value)
        .filter(|entry| !entry.as_os_str().is_empty())
        .filter(|entry| entry.is_absolute())
        .filter(|entry| entry != &workdir_canonical)
        .collect::<Vec<_>>()
        .into_iter()
        .fold(std::ffi::OsString::new(), |mut output, entry| {
            if !output.is_empty() {
                output.push(":");
            }
            output.push(entry);
            output
        })
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

    #[test]
    fn container_policy_is_bounded_and_structured() {
        let settings = AppSettings {
            sandbox_backend: "docker".into(),
            sandbox_image: "rust:1.88-bookworm".into(),
            allowed_executables: vec!["cargo".into(), "../bad".into()],
            ..Default::default()
        };
        let policy = CommandPolicy::from_settings(&settings);
        assert_eq!(policy.sandbox_backend, SandboxBackend::Docker);
        assert_eq!(policy.sandbox_image.as_deref(), Some("rust:1.88-bookworm"));
        assert_eq!(policy.allowed_executables, vec!["cargo".to_string()]);
        assert!(!policy.allow_network);
        assert!(!policy.allow_file_writes);
    }
}
