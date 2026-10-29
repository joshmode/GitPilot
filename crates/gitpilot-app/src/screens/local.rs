use crate::components;
use crate::state::{CommandPreset, UiEvent, UiState};
use crate::theme::Tokens;
use eframe::egui::{self, Align, Layout, RichText};
use gitpilot_core::{AppService, CommandSpec, DataOrigin};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

pub fn show(
    ui: &mut egui::Ui,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    if state.local_repos.is_empty() {
        let service = Arc::clone(service);
        let tx = tx.clone();
        runtime.spawn(async move {
            if let Ok(repos) = service.load_local_repositories().await {
                let _ = tx.send(UiEvent::LocalRepos(repos));
            }
        });
    }
    components::page_header(
        ui,
        tokens,
        "Local repository workspace",
        "Read-only Git inspection and guarded, allow-listed tool execution",
        |ui| {
            ui.label(
                RichText::new("No shell interpolation · no automatic execution")
                    .size(10.0)
                    .color(tokens.text_faint),
            );
        },
    );
    components::panel(tokens).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.local_path)
                    .hint_text("/path/to/repository")
                    .desired_width(420.0),
            );
            if ui.button("Connect local Git").clicked() {
                let path = state.local_path.clone();
                let service = Arc::clone(service);
                let tx = tx.clone();
                runtime.spawn(async move {
                    let cancel = CancellationToken::new();
                    match service.connect_local_repository(&path, &cancel).await {
                        Ok(_) => {
                            if let Ok(repos) = service.load_local_repositories().await {
                                let _ = tx.send(UiEvent::LocalRepos(repos));
                            }
                        }
                        Err(error) => {
                            let _ = tx.send(UiEvent::Error(error.user_message()));
                        }
                    }
                });
            }
        });
        ui.label(
            RichText::new(
                "The path must be a Git working tree. GitPilot does not clone, index automatically or execute repository code on open.",
            )
            .size(10.5)
            .color(tokens.text_muted),
        );
    });
    if let Some(output) = state.local_command_output.clone() {
        let mut dismiss = false;
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("COMMAND OUTPUT")
                        .size(9.5)
                        .strong()
                        .color(tokens.text_faint),
                );
                if ui.small_button("Dismiss").clicked() {
                    dismiss = true;
                }
            });
            egui::ScrollArea::both()
                .id_salt("local-output")
                .max_height(240.0)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(output)
                            .monospace()
                            .size(9.5)
                            .color(tokens.text_muted),
                    );
                });
        });
        ui.add_space(6.0);
        if dismiss {
            state.local_command_output = None;
        }
    }
    ui.add_space(8.0);
    if state.local_repos.is_empty() {
        components::empty_state(
            ui,
            tokens,
            "No local repository connected",
            "Connect a repository to inspect status, branches, commits and comparisons.",
            None,
        );
    }
    for repo in state.local_repos.clone() {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                components::origin_badge(ui, tokens, DataOrigin::LocalGit);
                ui.label(
                    RichText::new(&repo.name)
                        .size(13.0)
                        .strong()
                        .color(tokens.text),
                );
                ui.label(
                    RichText::new(&repo.path)
                        .monospace()
                        .size(10.0)
                        .color(tokens.text_faint),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("Status").clicked() {
                        let path = repo.path.clone();
                        let service = Arc::clone(service);
                        let tx = tx.clone();
                        runtime.spawn(async move {
                            let cancel = CancellationToken::new();
                            match service.local_status(&path, &cancel).await {
                                Ok(status) => {
                                    let _ = tx.send(UiEvent::LocalStatus(status));
                                }
                                Err(error) => {
                                    let _ = tx.send(UiEvent::Error(error.user_message()));
                                }
                            }
                        });
                    }
                    if ui.small_button("Index docs").clicked() {
                        let path = repo.path.clone();
                        let repo_id = repo.id.clone();
                        let service = Arc::clone(service);
                        let tx = tx.clone();
                        runtime.spawn(async move {
                            match service.index_local_documents(&repo_id, &path).await {
                                Ok(count) => {
                                    let _ = tx.send(UiEvent::Toast(format!(
                                        "Indexed {count} documentation records."
                                    )));
                                }
                                Err(error) => {
                                    let _ = tx.send(UiEvent::Error(error.user_message()));
                                }
                            }
                        });
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                components::badge(
                    ui,
                    tokens,
                    &format!("branch {}", repo.default_branch),
                    tokens.info,
                );
                components::badge(ui, tokens, "read-only inspection", tokens.success);
            });
        });
        ui.add_space(6.0);
    }
    if let Some(repo) = state.local_repos.first().cloned()
        && (state.local_branches.is_empty()
            || state.local_branches_repo.as_deref() != Some(&repo.path))
    {
        let path = repo.path.clone();
        let tx = tx.clone();
        runtime.spawn(async move {
            let cancel = CancellationToken::new();
            if let Ok(branches) = gitpilot_core::local_git::LocalGit::branches(&path, &cancel).await
            {
                let _ = tx.send(UiEvent::LocalBranches { path, branches });
            }
        });
    }
    if let Some(repo) = state.local_repos.first().cloned() {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("Branches · {}", repo.name))
                        .size(12.0)
                        .strong()
                        .color(tokens.text),
                );
                if ui.small_button("Refresh").clicked() {
                    state.local_branches.clear();
                }
            });
            if state.local_branches.is_empty() {
                ui.label(
                    RichText::new("Loading branches…")
                        .size(10.0)
                        .color(tokens.text_faint),
                );
            }
            ui.horizontal_wrapped(|ui| {
                for branch in state.local_branches.iter().take(40) {
                    components::badge(ui, tokens, branch, tokens.info);
                }
            });
        });
        ui.add_space(6.0);
    }
    if let Some(repo) = state.local_repos.first().cloned() {
        components::panel(tokens).show(ui, |ui| {
            ui.label(
                RichText::new(format!("Search code in {}", repo.name))
                    .size(12.0)
                    .strong()
                    .color(tokens.text),
            );
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut state.local_search_query)
                        .hint_text("function, type, error code")
                        .desired_width(260.0),
                );
                if ui.button("Search").clicked() {
                    let query = state.local_search_query.clone();
                    let path = repo.path.clone();
                    let service = Arc::clone(service);
                    let tx = tx.clone();
                    let cancel = CancellationToken::new();
                    runtime.spawn(async move {
                        match service.local_search(&path, &query, &cancel).await {
                            Ok(results) => {
                                let _ = tx.send(UiEvent::LocalSearch(results));
                            }
                            Err(error) => {
                                let _ = tx.send(UiEvent::Error(error.user_message()));
                            }
                        }
                    });
                }
            });
            egui::ScrollArea::vertical()
                .id_salt("local-search")
                .max_height(160.0)
                .show(ui, |ui| {
                    for result in state.local_search_results.iter().take(80) {
                        ui.label(
                            RichText::new(result)
                                .monospace()
                                .size(9.5)
                                .color(tokens.text_muted),
                        );
                    }
                });
        });
        ui.add_space(6.0);
    }
    if let Some(status) = &state.local_status {
        components::section_header(ui, tokens, "Working tree status", None);
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                components::badge(
                    ui,
                    tokens,
                    &format!("branch {}", status.branch),
                    tokens.info,
                );
                components::badge(
                    ui,
                    tokens,
                    &format!("↑{} ↓{}", status.ahead, status.behind),
                    tokens.text_muted,
                );
                components::badge(
                    ui,
                    tokens,
                    &format!("{} staged", status.staged),
                    tokens.success,
                );
                components::badge(
                    ui,
                    tokens,
                    &format!("{} modified", status.modified),
                    tokens.warning,
                );
                components::badge(
                    ui,
                    tokens,
                    &format!("{} untracked", status.untracked),
                    tokens.text_muted,
                );
                components::badge(
                    ui,
                    tokens,
                    &format!("{} conflicted", status.conflicted),
                    tokens.danger,
                );
            });
            for entry in status.entries.iter().take(30) {
                ui.label(
                    RichText::new(entry)
                        .monospace()
                        .size(10.0)
                        .color(tokens.text_muted),
                );
            }
        });
    }
    ui.add_space(10.0);
    components::section_header(
        ui,
        tokens,
        "Guarded command runner",
        Some("Structured executable + argument vector; no free-form shell"),
    );
    components::panel(tokens).show(ui, |ui| {
        ui.label(
            RichText::new(
                "Local execution is denied by default. Enable it in Settings → Privacy. Network and file modifications remain disabled unless separately allowed.",
            )
            .size(10.5)
            .color(tokens.warning),
        );
            if let Some(output) = &state.local_command_output {
                egui::ScrollArea::vertical()
                    .id_salt("command_output")
                    .max_height(180.0)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(output)
                                .monospace()
                                .size(9.5)
                                .color(tokens.text_muted),
                        );
                    });
                ui.add_space(5.0);
            }
        let Some(repo) = state.local_repos.first().cloned() else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            for (preset, label) in [
                (CommandPreset::Test, "cargo test"),
                (CommandPreset::Clippy, "cargo clippy"),
                (CommandPreset::FmtCheck, "cargo fmt --check"),
            ] {
                if ui.button(label).clicked() {
                    let spec = match preset {
                        CommandPreset::Test => CommandSpec {
                            executable: "cargo".into(),
                            args: vec!["test".into()],
                            working_directory: repo.path.clone(),
                            network: false,
                            modifies_files: false,
                            privileged: false,
                            timeout_seconds: 300,
                            description: "Run tests".into(),
                        },
                        CommandPreset::Clippy => CommandSpec {
                            executable: "cargo".into(),
                            args: vec![
                                "clippy".into(),
                                "--all-targets".into(),
                                "--".into(),
                                "-D".into(),
                                "warnings".into(),
                            ],
                            working_directory: repo.path.clone(),
                            network: false,
                            modifies_files: false,
                            privileged: false,
                            timeout_seconds: 300,
                            description: "Run lint".into(),
                        },
                        CommandPreset::FmtCheck => CommandSpec {
                            executable: "cargo".into(),
                            args: vec!["fmt".into(), "--check".into()],
                            working_directory: repo.path.clone(),
                            network: false,
                            modifies_files: false,
                            privileged: false,
                            timeout_seconds: 120,
                            description: "Check formatting".into(),
                        },
                    };
                    state.local_command_output = Some(format!(
                        "Starting {label} in the configured sandbox. This command was selected explicitly by the user; no model-generated shell is executed."
                    ));
                    let service = Arc::clone(service);
                    let tx = tx.clone();
                    runtime.spawn(async move {
                        let cancel = CancellationToken::new();
                        match service.run_command(&spec, &cancel).await {
                            Ok(result) => {
                                let _ = tx.send(UiEvent::CommandOutput(format!(
                                    "{} · exit {:?} · {}ms\n\n{}\n{}",
                                    result.command,
                                    result.exit_code,
                                    result.duration_ms,
                                    result.stdout.chars().take(12_000).collect::<String>(),
                                    result.stderr.chars().take(4_000).collect::<String>()
                                )));
                            }
                            Err(error) => {
                                let _ = tx.send(UiEvent::Error(error.user_message()));
                            }
                        }
                    });
                }
            }
        });
    });
}
