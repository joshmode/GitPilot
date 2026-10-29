use crate::components;
use crate::state::{UiEvent, UiState};
use crate::theme::Tokens;
use eframe::egui::{self, Layout, RichText};
use gitpilot_core::AppService;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

pub fn show(
    ui: &mut egui::Ui,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    let mut create_confirmed = false;
    let mut cancel_create = false;
    if state.issues.is_empty() {
        let service = Arc::clone(service);
        let tx = tx.clone();
        let repository_id = state.repository_scope.clone();
        runtime.spawn(async move {
            match service.issues(repository_id.as_deref()).await {
                Ok(issues) => {
                    let _ = tx.send(UiEvent::Issues(issues));
                }
                Err(error) => {
                    let _ = tx.send(UiEvent::Error(error.user_message()));
                }
            }
        });
    }
    components::page_header(
        ui,
        tokens,
        "Issues",
        "Outstanding work, labels, assignments and milestone context",
        |ui| {
            ui.checkbox(&mut state.issue_open_only, "Open only");
        },
    );
    let live_repos: Vec<_> = state
        .bootstrap
        .repositories
        .iter()
        .filter(|repo| repo.origin == gitpilot_core::DataOrigin::GitHub)
        .cloned()
        .collect();
    if !live_repos.is_empty() {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Create issue")
                        .size(12.5)
                        .strong()
                        .color(tokens.text),
                );
                components::badge(ui, tokens, "REQUIRES WRITE PERMISSION", tokens.warning);
            });
            ui.horizontal(|ui| {
                let selected = state
                    .issue_repository
                    .clone()
                    .or_else(|| live_repos.first().map(|r| r.id.clone()));
                egui::ComboBox::from_id_salt("issue_repository")
                    .selected_text(
                        selected
                            .as_ref()
                            .and_then(|id| live_repos.iter().find(|r| &r.id == id))
                            .map(|r| r.full_name.as_str())
                            .unwrap_or("Select repository"),
                    )
                    .show_ui(ui, |ui| {
                        for repo in &live_repos {
                            if ui
                                .selectable_label(
                                    selected.as_deref() == Some(&repo.id),
                                    &repo.full_name,
                                )
                                .clicked()
                            {
                                state.issue_repository = Some(repo.id.clone());
                            }
                        }
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut state.issue_title)
                        .hint_text("Issue title")
                        .desired_width(280.0),
                );
            });
            ui.add(
                egui::TextEdit::multiline(&mut state.issue_body)
                    .hint_text("Describe the problem and evidence")
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut state.issue_labels)
                        .hint_text("labels, comma separated")
                        .desired_width(180.0),
                );
                let enabled = state.bootstrap.permissions.modify_github
                    && !state.issue_title.trim().is_empty()
                    && state.issue_repository.is_some();
                if ui
                    .add_enabled(enabled, egui::Button::new("Review & create"))
                    .on_hover_text(
                        "Requires Settings → Privacy → Modify GitHub resources. A confirmation is shown next.",
                    )
                    .clicked()
                {
                    state.confirm_action = Some("create_issue".into());
                }
            });
            if !state.bootstrap.permissions.modify_github {
                ui.label(
                    RichText::new(
                        "GitHub writes are denied by policy. This control stays disabled until you enable the separate write permission.",
                    )
                    .size(10.0_f32)
                    .color(tokens.warning),
                );
            }
        });
        ui.add_space(8.0);
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.global_search)
                .hint_text("Filter issues")
                .desired_width(300.0),
        );
        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{} issues", state.issues.len()))
                    .size(10.5)
                    .color(tokens.text_faint),
            );
        });
    });
    ui.add_space(8.0);
    egui::ScrollArea::vertical()
        .id_salt("issue_list")
        .show(ui, |ui| {
            for issue in state
                .issues
                .iter()
                .filter(|i| state.issue_open_only == (i.state == "open"))
                .filter(|i| {
                    state.global_search.trim().is_empty()
                        || i.title
                            .to_ascii_lowercase()
                            .contains(&state.global_search.to_ascii_lowercase())
                })
            {
                components::panel(tokens).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        components::status_dot(
                            ui,
                            if issue.state == "open" {
                                tokens.success
                            } else {
                                tokens.text_faint
                            },
                        );
                        ui.label(
                            RichText::new(format!("#{}", issue.number))
                                .monospace()
                                .size(10.5)
                                .color(tokens.text_faint),
                        );
                        ui.label(
                            RichText::new(&issue.title)
                                .size(12.5)
                                .strong()
                                .color(tokens.text),
                        );
                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(&issue.repository_name)
                                    .size(10.0)
                                    .color(tokens.text_faint),
                            );
                        });
                    });
                    ui.horizontal_wrapped(|ui| {
                        for label in &issue.labels {
                            components::tag(ui, tokens, label);
                        }
                        if !issue.assignees.is_empty() {
                            components::badge(
                                ui,
                                tokens,
                                &format!("assigned to {}", issue.assignees.join(", ")),
                                tokens.info,
                            );
                        }
                        if let Some(milestone) = &issue.milestone {
                            components::badge(ui, tokens, milestone, tokens.warning);
                        }
                        ui.label(
                            RichText::new(format!(
                                "{} · {} comments",
                                components::short_time(issue.updated_at),
                                issue.comments
                            ))
                            .size(10.0)
                            .color(tokens.text_faint),
                        );
                    });
                });
                ui.add_space(5.0);
            }
        });
    if state.confirm_action.as_deref() == Some("create_issue") {
        components::centered_modal(ui.ctx(), tokens, 520.0, |ui| {
            ui.label(
                RichText::new("Confirm GitHub issue creation")
                    .size(17.0)
                    .strong()
                    .color(tokens.text),
            );
            ui.label(
                RichText::new(
                    "This will create a visible issue in the selected GitHub repository. It is an external mutation and will be written to the audit log.",
                )
                .size(11.0)
                .color(tokens.text_muted),
            );
            let repo = state
                .issue_repository
                .as_ref()
                .and_then(|id| state.bootstrap.repositories.iter().find(|r| &r.id == id));
            components::inset(tokens).show(ui, |ui| {
                components::kv(
                    ui,
                    tokens,
                    "Repository",
                    repo.map(|r| r.full_name.as_str()).unwrap_or("—"),
                );
                components::kv(ui, tokens, "Title", &state.issue_title);
                components::kv(
                    ui,
                    tokens,
                    "Labels",
                    if state.issue_labels.trim().is_empty() {
                        "none"
                    } else {
                        &state.issue_labels
                    },
                );
            });
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    cancel_create = true;
                }
                if ui
                    .add(egui::Button::new("Create issue").fill(tokens.accent))
                    .clicked()
                {
                    create_confirmed = true;
                }
            });
        });
    }
    if cancel_create {
        state.confirm_action = None;
    }
    if create_confirmed {
        if let (Some(repository_id), Some(repo)) = (
            state.issue_repository.clone(),
            state
                .issue_repository
                .as_ref()
                .and_then(|id| state.bootstrap.repositories.iter().find(|r| &r.id == id)),
        ) {
            let title = state.issue_title.trim().to_owned();
            let body = state.issue_body.clone();
            let labels: Vec<String> = state
                .issue_labels
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            let service = Arc::clone(service);
            let tx = tx.clone();
            let _ = repo;
            runtime.spawn(async move {
                match service
                    .create_issue(&repository_id, &title, &body, &labels, true)
                    .await
                {
                    Ok(()) => {
                        let _ = tx.send(UiEvent::Toast(
                            "Issue created on GitHub. Refresh to include it.".into(),
                        ));
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
        state.confirm_action = None;
        state.issue_title.clear();
        state.issue_body.clear();
        state.issue_labels.clear();
    }
}
