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
    if state.commits.is_empty() {
        let service = Arc::clone(service);
        let tx = tx.clone();
        let repository_id = state.repository_scope.clone();
        runtime.spawn(async move {
            match service.commits(repository_id.as_deref(), 100).await {
                Ok(commits) => {
                    let _ = tx.send(UiEvent::Commits(commits));
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
        "Commits & change intelligence",
        "Actual commit metadata, comparison entry points and AI explanations",
        |ui| {
            ui.label(
                RichText::new("Read-only Git semantics")
                    .size(10.0)
                    .color(tokens.text_faint),
            );
        },
    );
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.commit_search)
                .hint_text("Search commit message, SHA or author")
                .desired_width(320.0),
        );
        ui.label(
            RichText::new(format!("{} commits cached", state.commits.len()))
                .size(10.5)
                .color(tokens.text_faint),
        );
    });
    components::divider(ui, tokens);
    components::inset(tokens).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("COMPARE")
                    .size(9.0)
                    .strong()
                    .color(tokens.text_faint),
            );
            ui.add(
                egui::TextEdit::singleline(&mut state.commit_compare_left)
                    .hint_text("base ref or SHA")
                    .desired_width(150.0),
            );
            ui.label("→");
            ui.add(
                egui::TextEdit::singleline(&mut state.commit_compare_right)
                    .hint_text("head ref or SHA")
                    .desired_width(150.0),
            );
            if ui.button("Compare via local Git").clicked() {
                if let Some(local) = state.local_repos.first().cloned() {
                    let left = state.commit_compare_left.clone();
                    let right = state.commit_compare_right.clone();
                    let tx = tx.clone();
                    runtime.spawn(async move {
                        let cancel = tokio_util::sync::CancellationToken::new();
                        match gitpilot_core::local_git::LocalGit::compare(
                            &local.path,
                            &left,
                            &right,
                            &cancel,
                        )
                        .await
                        {
                            Ok(diff) => {
                                let _ = tx.send(UiEvent::CompareResult(diff));
                            }
                            Err(error) => {
                                let _ = tx.send(UiEvent::Error(error.user_message()));
                            }
                        }
                    });
                } else {
                    state.error =
                        Some("Connect a local repository before comparing revisions.".into());
                }
            }
            if ui.button("Compare via GitHub").clicked() {
                if let Some(repo) = state
                    .bootstrap
                    .repositories
                    .iter()
                    .find(|repo| repo.origin == gitpilot_core::DataOrigin::GitHub)
                    .cloned()
                {
                    let left = state.commit_compare_left.clone();
                    let right = state.commit_compare_right.clone();
                    let service = Arc::clone(service);
                    let tx = tx.clone();
                    runtime.spawn(async move {
                        match service.github_compare(&repo.id, &left, &right).await {
                            Ok(value) => {
                                let _ = tx.send(UiEvent::CompareResult(format!(
                                    "GitHub compare {}...{}\n{}",
                                    left, right, value
                                )));
                            }
                            Err(error) => {
                                let _ = tx.send(UiEvent::Error(error.user_message()));
                            }
                        }
                    });
                } else {
                    state.error = Some("No live GitHub repository is available.".into());
                }
            }
        });
    });
    ui.add_space(8.0);
    if let Some(result) = state.compare_result.clone() {
        let mut clear = false;
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Comparison")
                        .size(12.0)
                        .strong()
                        .color(tokens.text),
                );
                if ui.small_button("Clear").clicked() {
                    clear = true;
                }
            });
            egui::ScrollArea::both().max_height(160.0).show(ui, |ui| {
                ui.label(
                    RichText::new(&result)
                        .monospace()
                        .size(9.5)
                        .color(tokens.text_muted),
                );
            });
        });
        if clear {
            state.compare_result = None;
        }
    }
    for commit in state.commits.iter().filter(|c| {
        state.commit_search.trim().is_empty()
            || c.message
                .to_ascii_lowercase()
                .contains(&state.commit_search.to_ascii_lowercase())
            || c.sha
                .to_ascii_lowercase()
                .contains(&state.commit_search.to_ascii_lowercase())
    }) {
        let mut open = false;
        let mut explain = false;
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(commit.sha.chars().take(8).collect::<String>())
                        .monospace()
                        .size(10.0)
                        .color(tokens.info),
                );
                ui.label(
                    RichText::new(commit.message.lines().next().unwrap_or(""))
                        .size(12.0)
                        .strong()
                        .color(tokens.text),
                );
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Explain").clicked() {
                        explain = true;
                    }
                    if ui.small_button("Open").clicked() {
                        open = true;
                    }
                    ui.label(
                        RichText::new(&commit.repository_name)
                            .size(10.0)
                            .color(tokens.text_faint),
                    );
                });
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{} · {} · +{} −{}",
                        commit.author_name,
                        components::short_time(commit.committed_at),
                        commit.additions,
                        commit.deletions
                    ))
                    .size(10.0)
                    .color(tokens.text_faint),
                );
                if let Some(verification) = &commit.verification {
                    components::badge(ui, tokens, verification, tokens.success);
                }
                if let Some(pr) = commit.pr_number {
                    components::badge(ui, tokens, &format!("PR #{pr}"), tokens.accent);
                }
            });
        });
        if open {
            state.selected_commit = Some(commit.id.clone());
            state.commit_files.clear();
            let id = commit.id.clone();
            let service = Arc::clone(service);
            let tx = tx.clone();
            runtime.spawn(async move {
                if let Ok(detail) = service.commit_detail(&id).await {
                    let _ = tx.send(UiEvent::CommitDetail(Box::new(detail)));
                }
                match service.commit_files(&id).await {
                    Ok(files) => {
                        let _ = tx.send(UiEvent::CommitFiles(files));
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
        if explain {
            let id = commit.id.clone();
            let service = Arc::clone(service);
            let tx = tx.clone();
            runtime.spawn(async move {
                let cancel = tokio_util::sync::CancellationToken::new();
                match service.explain_commit(&id, &cancel).await {
                    Ok(explanation) => {
                        let _ = tx.send(UiEvent::Explanation(explanation));
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
        ui.add_space(4.0);
    }
    if let Some(explanation) = &state.explanation {
        components::divider(ui, tokens);
        components::section_header(
            ui,
            tokens,
            "Commit explanation",
            Some("Metadata-derived · AI inference is labelled"),
        );
        components::panel(tokens).show(ui, |ui| {
            ui.label(
                RichText::new(&explanation.executive)
                    .size(11.5)
                    .color(tokens.text_muted),
            );
        });
    }
    if let Some(commit) = &state.selected_commit_detail {
        components::divider(ui, tokens);
        components::section_header(
            ui,
            tokens,
            "Selected commit",
            Some("Metadata from the repository; no authorship inference"),
        );
        components::panel(tokens).show(ui, |ui| {
            components::kv(
                ui,
                tokens,
                "SHA",
                &commit.sha.chars().take(12).collect::<String>(),
            );
            components::kv(ui, tokens, "Author", &commit.author_name);
            components::kv(ui, tokens, "Parents", &commit.parents.join(", "));
            components::kv(ui, tokens, "Committed", &commit.committed_at.to_rfc3339());
            if !commit.message.trim().is_empty() {
                ui.label(
                    RichText::new(&commit.message)
                        .size(10.5)
                        .color(tokens.text_muted),
                );
            }
            if ui.small_button("Open commit on GitHub").clicked() {
                let _ = webbrowser::open(&commit.html_url);
            }
        });
    }
    if !state.commit_files.is_empty() {
        components::divider(ui, tokens);
        components::section_header(
            ui,
            tokens,
            "Commit diff",
            Some("Fetched from the GitHub commit detail endpoint"),
        );
        for file in state.commit_files.iter().take(30) {
            components::panel(tokens).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&file.path)
                            .monospace()
                            .size(10.5)
                            .color(tokens.text),
                    );
                    components::badge(
                        ui,
                        tokens,
                        &format!("+{} −{}", file.additions, file.deletions),
                        tokens.text_muted,
                    );
                });
                if file.binary {
                    ui.label(
                        RichText::new("Binary file; no text patch")
                            .size(10.0)
                            .color(tokens.text_faint),
                    );
                } else {
                    for hunk in file.hunks.iter().take(4) {
                        for line in hunk.lines.iter().take(80) {
                            ui.label(
                                RichText::new(format!(
                                    "{:>4} {:?} {}",
                                    line.new_line.or(line.old_line).unwrap_or(0),
                                    line.kind,
                                    line.content
                                ))
                                .monospace()
                                .size(9.5)
                                .color(tokens.text_muted),
                            );
                        }
                    }
                }
            });
            ui.add_space(4.0);
        }
    }
}
