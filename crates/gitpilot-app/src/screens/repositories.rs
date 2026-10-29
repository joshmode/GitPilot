use crate::components;
use crate::state::{UiEvent, UiState};
use crate::theme::Tokens;
use eframe::egui::{self, Align, Layout, RichText};
use gitpilot_core::AppService;
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
    if !state.branch_overview_repo.as_ref().is_some_and(|repo| {
        state
            .bootstrap
            .repositories
            .iter()
            .any(|row| &row.id == repo)
    }) {
        state.branch_overview_repo = state.repository_scope.clone().or_else(|| {
            state
                .bootstrap
                .repositories
                .first()
                .map(|repo| repo.id.clone())
        });
    }
    if state.branch_overview.is_empty()
        && let Some(repository_id) = state.branch_overview_repo.clone()
    {
        let service = Arc::clone(service);
        let tx = tx.clone();
        runtime.spawn(async move {
            if let Ok(branches) = service.store.repository_branches(&repository_id).await {
                let _ = tx.send(UiEvent::BranchOverview(branches));
            }
        });
    }
    let mut sync_repo = None;
    let mut workspace_repo: Option<String> = None;
    let mut sync_all = false;
    let mut discover = false;
    components::page_header(
        ui,
        tokens,
        "Repositories",
        "Discovery, favorites, project grouping and metadata health",
        |ui| {
            if ui.button("Sync live repositories").clicked() {
                sync_all = true;
            }
            if ui.button("Discover from GitHub").clicked() {
                discover = true;
            }
        },
    );
    if state.bootstrap.accounts.is_empty() {
        components::inset(tokens).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                components::badge(ui, tokens, "DEMO MODE", tokens.warning);
                ui.label(
                    RichText::new(
                        "Repository data below is synthetic. Connect GitHub in Settings to discover live repositories.",
                    )
                    .size(11.0_f32)
                    .color(tokens.text_muted),
                );
            });
        });
        ui.add_space(8.0);
    }
    ui.horizontal(|ui| {
        let mut query = state.global_search.clone();
        let response = ui.add(
            egui::TextEdit::singleline(&mut query)
                .hint_text("Filter repositories")
                .desired_width(300.0),
        );
        if response.changed() {
            state.global_search = query;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!(
                    "{} repositories · {} workspaces",
                    state.bootstrap.repositories.len(),
                    state
                        .bootstrap
                        .repositories
                        .iter()
                        .filter_map(|r| r.workspace.as_ref())
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                ))
                .size(10.5)
                .color(tokens.text_faint),
            );
        });
    });
    if let Some(repository_id) = state.branch_overview_repo.clone() {
        let repository_name = state
            .bootstrap
            .repositories
            .iter()
            .find(|repo| repo.id == repository_id)
            .map(|repo| repo.full_name.clone())
            .unwrap_or_else(|| "repository".into());
        components::section_header(
            ui,
            tokens,
            "Branch overview",
            Some(&format!("GitHub branch heads for {repository_name}")),
        );
        if state.branch_overview.is_empty() {
            ui.label(
                RichText::new(
                    "No branch metadata is cached. Sync the repository to fetch up to 200 branches; this does not imply there are no branches.",
                )
                .size(10.5)
                .color(tokens.text_faint),
            );
        }
        for branch in state.branch_overview.iter().take(50) {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&branch.name)
                        .monospace()
                        .size(10.5)
                        .color(tokens.text),
                );
                components::badge(
                    ui,
                    tokens,
                    &branch.head_sha.chars().take(8).collect::<String>(),
                    tokens.info,
                );
                if branch.protected {
                    components::badge(ui, tokens, "protected", tokens.success);
                }
            });
        }
    }
    ui.add_space(8.0);
    egui::ScrollArea::vertical()
        .id_salt("repo_list")
        .show(ui, |ui| {
            for repo in state.bootstrap.repositories.clone() {
                if !state.global_search.trim().is_empty()
                    && !repo
                        .full_name
                        .to_ascii_lowercase()
                        .contains(&state.global_search.to_ascii_lowercase())
                {
                    continue;
                }
                components::panel(tokens).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(repo.favorite, if repo.favorite { "★" } else { "☆" })
                            .on_hover_text("Pin repository")
                            .clicked()
                        {
                            let service = Arc::clone(service);
                            let repo_id = repo.id.clone();
                            runtime.spawn(async move {
                                let _ = service
                                    .store
                                    .set_repository_favorite(&repo_id, !repo.favorite)
                                    .await;
                            });
                        }
                        ui.label(
                            RichText::new(&repo.full_name)
                                .size(14.0)
                                .strong()
                                .color(tokens.text),
                        );
                        components::origin_badge(ui, tokens, repo.origin.clone());
                        if repo.private {
                            components::badge(ui, tokens, "private", tokens.text_muted);
                        }
                        if repo.archived {
                            components::badge(ui, tokens, "archived", tokens.warning);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button("Sync now").clicked() {
                                sync_repo = Some(repo.id.clone());
                            }
                            if ui.small_button("Group").clicked() {
                                workspace_repo = Some(repo.id.clone());
                            }
                            if ui.small_button("Scope").clicked() {
                                state.repository_scope = Some(repo.id.clone());
                                state.screen = crate::nav::Screen::PullRequests;
                            }
                        });
                    });
                    ui.label(
                        RichText::new(repo.description.as_deref().unwrap_or("No description"))
                            .size(11.0_f32)
                            .color(tokens.text_muted),
                    );
                    ui.horizontal_wrapped(|ui| {
                        components::badge(
                            ui,
                            tokens,
                            repo.workspace.as_deref().unwrap_or("Ungrouped"),
                            tokens.info,
                        );
                        components::badge(
                            ui,
                            tokens,
                            repo.language.as_deref().unwrap_or("Unknown language"),
                            tokens.text_muted,
                        );
                        components::badge(
                            ui,
                            tokens,
                            &format!("{} stars", repo.stars),
                            tokens.text_muted,
                        );
                        components::badge(
                            ui,
                            tokens,
                            &format!("{} open issues", repo.open_issues),
                            tokens.text_muted,
                        );
                        ui.label(
                            RichText::new(format!("branch {}", repo.default_branch))
                                .monospace()
                                .size(10.0)
                                .color(tokens.text_faint),
                        );
                    });
                });
                ui.add_space(6.0);
            }
        });
    if let Some(repo_id) = workspace_repo.clone() {
        let mut workspace = state
            .bootstrap
            .repositories
            .iter()
            .find(|r| r.id == repo_id)
            .and_then(|r| r.workspace.clone())
            .unwrap_or_default();
        let mut save = false;
        components::centered_modal(ui.ctx(), tokens, 420.0, |ui| {
            ui.label(
                RichText::new("Project workspace")
                    .size(16.0)
                    .strong()
                    .color(tokens.text),
            );
            ui.label(
                RichText::new(
                    "Group repositories that belong to one logical project. The assignment is local metadata and is persisted.",
                )
                .size(10.5)
                .color(tokens.text_muted),
            );
            ui.text_edit_singleline(&mut workspace);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    save = true;
                }
                if ui.button("Clear").clicked() {
                    workspace.clear();
                    save = true;
                }
            });
        });
        if save {
            let service = Arc::clone(service);
            let tx = tx.clone();
            let value = (!workspace.trim().is_empty()).then(|| workspace.trim().to_string());
            runtime.spawn(async move {
                match service
                    .store
                    .set_repository_workspace(&repo_id, value.as_deref())
                    .await
                {
                    Ok(()) => {
                        let _ = tx.send(UiEvent::BootstrapRefreshed);
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
    }
    if sync_all {
        if state.bootstrap.accounts.is_empty() {
            state.error = Some("Connect a GitHub account before syncing.".into());
        } else if state
            .bootstrap
            .accounts
            .iter()
            .all(|account| account.id == "account-0001")
        {
            state.error = Some(
                "Demo repositories are synthetic. Connect GitHub and discover live repositories to sync.".into(),
            );
        } else {
            let service = Arc::clone(service);
            let tx = tx.clone();
            let count = state
                .bootstrap
                .repositories
                .iter()
                .filter(|r| r.origin == gitpilot_core::DataOrigin::GitHub)
                .count();
            runtime.spawn(async move {
                match service.enqueue_sync().await {
                    Ok(job_id) => {
                        let _ = tx.send(UiEvent::Toast(format!(
                            "Sync queued for {count} repositories ({job_id})."
                        )));
                        let _ = tx.send(UiEvent::BootstrapRefreshed);
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
    }
    if discover {
        if let Some(account) = state.bootstrap.accounts.first() {
            let account_id = account.id.clone();
            let service = Arc::clone(service);
            let tx = tx.clone();
            let cancel = CancellationToken::new();
            runtime.spawn(async move {
                match service.discover_repositories(&account_id, &cancel).await {
                    Ok(repos) => {
                        let _ = tx.send(UiEvent::RepositoriesDiscovered(repos));
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        } else {
            state.error = Some("Connect a GitHub account before discovering repositories.".into());
        }
    }
    if let Some(id) = sync_repo {
        let service = Arc::clone(service);
        let tx = tx.clone();
        let cancel = CancellationToken::new();
        runtime.spawn(async move {
            match service.sync_repository(&id, &cancel).await {
                Ok(_) => {
                    let _ = tx.send(UiEvent::BootstrapRefreshed);
                }
                Err(error) => {
                    let _ = tx.send(UiEvent::Error(error.user_message()));
                }
            }
        });
    }
}
