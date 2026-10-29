use crate::components;
use crate::state::{DetailTab, DiffMode, UiEvent, UiState};
use crate::theme::Tokens;
use eframe::egui::{self, Align, Layout, RichText};
use gitpilot_core::{
    AppService, DiffFile, DiffLineKind, EvidenceKind, FindingLifecycle, PrQuery, PrView,
    PullRequest,
};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

pub fn show(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    if state.pr_rows.is_empty() && !state.pr_loading {
        load_list(state, service, runtime, tx);
    }
    if state.saved_pr_views.is_empty() {
        let service = Arc::clone(service);
        let tx = tx.clone();
        runtime.spawn(async move {
            if let Ok(views) = service.store.saved_views("pull_requests").await {
                let _ = tx.send(UiEvent::SavedViews(views));
            }
        });
    }
    if let Some(selected) = state.selected_pr.clone() {
        if state.pr_detail.as_ref().map(|d| d.pull_request.id.as_str()) != Some(&selected) {
            load_detail(selected, state, service, runtime, tx);
        }
        detail_view(ui, ctx, state, tokens, service, runtime, tx);
        return;
    }
    components::page_header(
        ui,
        tokens,
        "Pull request cockpit",
        "Triage review load, inspect diffs and run evidence-backed reviews",
        |ui| {
            if ui.button("Refresh").clicked() {
                load_list(state, service, runtime, tx);
            }
            if ui.button("Review policy").clicked() {
                state.review_policy_draft = state.bootstrap.review_policy.clone();
                state.palette_hint = "review_policy".into();
            }
        },
    );
    ui.horizontal_wrapped(|ui| {
        let views = [
            PrView::All,
            PrView::Mine,
            PrView::NeedsReview,
            PrView::Blocked,
            PrView::CiFailing,
            PrView::Ready,
            PrView::Stale,
            PrView::HighRisk,
        ];
        for view in views {
            if ui
                .selectable_label(state.pr_view == view, view.label())
                .clicked()
            {
                state.pr_view = view;
                state.pr_page = 0;
                load_list(state, service, runtime, tx);
            }
        }
    });
    ui.horizontal(|ui| {
        let search = ui.add(
            egui::TextEdit::singleline(&mut state.pr_search)
                .hint_text("Search title, description or author")
                .desired_width(320.0),
        );
        if search.changed() {
            state.pr_page = 0;
            load_list(state, service, runtime, tx);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!(
                    "{} results · page {}",
                    state.pr_total,
                    state.pr_page + 1
                ))
                .size(10.5)
                .color(tokens.text_faint),
            );
            if ui
                .add_enabled(state.pr_page > 0, egui::Button::new("Previous"))
                .clicked()
            {
                state.pr_page -= 1;
                load_list(state, service, runtime, tx);
            }
            if ui
                .add_enabled(state.pr_rows.len() == 40, egui::Button::new("Next"))
                .clicked()
            {
                state.pr_page += 1;
                load_list(state, service, runtime, tx);
            }
            if ui.small_button("Save view").clicked() {
                let name = format!(
                    "{} · {}",
                    state.pr_view.label(),
                    if state.pr_search.trim().is_empty() {
                        "no search"
                    } else {
                        state.pr_search.trim()
                    }
                );
                let query = state.pr_search.clone();
                let service = Arc::clone(service);
                let tx = tx.clone();
                runtime.spawn(async move {
                    match service.save_view("pull_requests", &name, &query).await {
                        Ok(()) => {
                            let _ = tx.send(UiEvent::Toast(format!("Saved view “{name}”")));
                        }
                        Err(error) => {
                            let _ = tx.send(UiEvent::Error(error.user_message()));
                        }
                    }
                });
            }
        });
    });
    if let Some(run) = state.review_run.as_ref()
        && let Some(error) = &run.error
    {
        ui.label(
            RichText::new(format!("Last review run error: {error}"))
                .size(10.5)
                .color(tokens.warning),
        );
    }
    ui.add_space(8.0);
    if !state.saved_pr_views.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("SAVED")
                    .size(9.0)
                    .strong()
                    .color(tokens.text_faint),
            );
            for view in state.saved_pr_views.clone() {
                if ui.small_button(&view.name).clicked() {
                    state.pr_search = view.query;
                    load_list(state, service, runtime, tx);
                }
            }
        });
    }
    if state.pr_loading {
        components::loading(ui, tokens, "Loading pull requests from the local cache");
        return;
    }
    if state.pr_rows.is_empty() {
        components::empty_state(
            ui,
            tokens,
            "No pull requests match this view",
            "Adjust filters, synchronize repositories, or switch to demo mode.",
            None,
        );
        return;
    }
    // Rows are intentionally information-dense and use a stable card rhythm
    // rather than a table that collapses awkwardly on laptop widths.
    egui::ScrollArea::vertical()
        .id_salt("pr_list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for pr in state.pr_rows.clone() {
                pr_row(ui, tokens, state, &pr);
            }
        });
}

fn pr_row(ui: &mut egui::Ui, tokens: &Tokens, state: &mut UiState, pr: &PullRequest) {
    let mut open = false;
    components::panel(tokens).show(ui, |ui| {
        ui.horizontal(|ui| {
            components::check_badge(ui, tokens, pr.check_state);
            components::review_badge(ui, tokens, pr.review_state);
            if pr.draft {
                components::badge(ui, tokens, "draft", tokens.text_faint);
            }
            if pr.risk_score >= 60 {
                components::badge(ui, tokens, "high risk", tokens.danger);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(pr.repository_name.as_str())
                        .size(10.0)
                        .color(tokens.text_faint),
                );
                ui.label(
                    RichText::new(format!("#{}", pr.number))
                        .size(10.5)
                        .color(tokens.text_faint),
                );
            });
        });
        ui.add_space(3.0);
        let title = ui
            .add(
                egui::Label::new(
                    RichText::new(&pr.title)
                        .size(13.0)
                        .strong()
                        .color(tokens.text),
                )
                .sense(egui::Sense::click()),
            )
            .on_hover_text("Open PR cockpit");
        if title.clicked() {
            open = true;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} · {} files · +{} −{}",
                    pr.author, pr.changed_files, pr.additions, pr.deletions
                ))
                .size(10.5)
                .color(tokens.text_muted),
            );
            ui.label(
                RichText::new(format!("updated {}", components::short_time(pr.updated_at)))
                    .size(10.0)
                    .color(tokens.text_faint),
            );
            components::merge_badge(ui, tokens, pr.mergeable);
            for label in pr.labels.iter().take(3) {
                components::tag(ui, tokens, label);
            }
        });
    });
    if open {
        state.selected_pr = Some(pr.id.clone());
        state.pr_detail = None;
        state.explanation = None;
        state.review_run = None;
        state.pr_tab = DetailTab::Overview;
        state.diff_file = None;
    }
    ui.add_space(5.0);
}

fn load_list(
    state: &mut UiState,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    state.pr_loading = true;
    let query = PrQuery {
        repository_id: state.repository_scope.clone(),
        search: state.pr_search.clone(),
        view: state.pr_view,
        viewer: state.bootstrap.accounts.first().map(|a| a.login.clone()),
        page: state.pr_page,
        page_size: 40,
    };
    let service = Arc::clone(service);
    let tx = tx.clone();
    runtime.spawn(async move {
        match service.pull_requests(&query).await {
            Ok((rows, total)) => {
                let _ = tx.send(UiEvent::PrList { rows, total });
            }
            Err(error) => {
                let _ = tx.send(UiEvent::Error(error.user_message()));
            }
        }
    });
}

fn load_detail(
    id: String,
    state: &mut UiState,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    state.pr_loading = true;
    let service = Arc::clone(service);
    let tx = tx.clone();
    runtime.spawn(async move {
        match service.pr_detail(&id).await {
            Ok(detail) => {
                let _ = tx.send(UiEvent::PrDetail(Box::new(detail)));
            }
            Err(error) => {
                let _ = tx.send(UiEvent::Error(error.user_message()));
            }
        }
    });
}

fn detail_view(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    let Some(detail) = state.pr_detail.clone() else {
        components::loading(ui, tokens, "Loading pull request cockpit");
        return;
    };
    let pr = &detail.pull_request;
    let mut back = false;
    let mut explain = false;
    let mut review = false;
    let mut submit_review = false;
    let mut open_external = false;
    ui.horizontal(|ui| {
        if ui.button("← Back to pull requests").clicked() {
            back = true;
        }
        ui.label(
            RichText::new(format!("{}  #{}", pr.repository_name, pr.number))
                .size(11.0_f32)
                .color(tokens.text_faint),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Open on GitHub").clicked() {
                open_external = true;
            }
            if ui.button("Run private review").clicked() {
                review = true;
            }
            if ui.button("Submit review…").clicked() {
                state.review_submit_event = "COMMENT".into();
                state.review_submit_body.clear();
                submit_review = true;
            }
            if ui.button("Explain this PR").clicked() {
                explain = true;
            }
        });
    });
    ui.add_space(5.0);
    ui.label(
        RichText::new(&pr.title)
            .size(21.0_f32)
            .strong()
            .color(tokens.text),
    );
    ui.horizontal_wrapped(|ui| {
        components::origin_badge(ui, tokens, pr.origin.clone());
        components::check_badge(ui, tokens, pr.check_state);
        components::review_badge(ui, tokens, pr.review_state);
        components::merge_badge(ui, tokens, pr.mergeable);
        components::badge(
            ui,
            tokens,
            &format!("risk {}", pr.risk_score),
            if pr.risk_score >= 60 {
                tokens.danger
            } else {
                tokens.text_muted
            },
        );
        ui.label(
            RichText::new(format!(
                "{} → {} · {} commits · +{} −{}",
                pr.base_ref, pr.head_ref, pr.commit_count, pr.additions, pr.deletions
            ))
            .size(10.5)
            .color(tokens.text_muted),
        );
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        for (tab, label) in [
            (DetailTab::Overview, "Overview"),
            (DetailTab::Files, "Files & diff"),
            (DetailTab::Review, "AI review"),
            (DetailTab::Commits, "Commits"),
            (DetailTab::Checks, "Checks"),
            (DetailTab::Discussion, "Discussion"),
        ] {
            if ui.selectable_label(state.pr_tab == tab, label).clicked() {
                state.pr_tab = tab;
            }
        }
    });
    components::divider(ui, tokens);
    match state.pr_tab {
        DetailTab::Overview => overview_tab(ui, tokens, state, &detail),
        DetailTab::Files => files_tab(ui, tokens, state, &detail, service, runtime, tx),
        DetailTab::Review => review_tab(ui, tokens, state, &detail, service, runtime, tx),
        DetailTab::Commits => commits_tab(ui, tokens, &detail),
        DetailTab::Checks => checks_tab(ui, tokens, &detail),
        DetailTab::Discussion => discussion_tab(ui, tokens, &detail),
    }
    if back {
        state.selected_pr = None;
        state.pr_detail = None;
        state.explanation = None;
        state.review_run = None;
    }
    if open_external {
        let _ = webbrowser::open(&pr.html_url);
    }
    if explain {
        let id = pr.id.clone();
        let service = Arc::clone(service);
        let tx = tx.clone();
        let cancel = CancellationToken::new();
        runtime.spawn(async move {
            match service.explain_pr(&id, &cancel).await {
                Ok(explanation) => {
                    let _ = tx.send(UiEvent::Explanation(explanation));
                }
                Err(error) => {
                    let _ = tx.send(UiEvent::Error(error.user_message()));
                }
            }
        });
    }
    if review {
        let id = pr.id.clone();
        let service = Arc::clone(service);
        let tx = tx.clone();
        runtime.spawn(async move {
            match service.enqueue_review(&id).await {
                Ok(job_id) => {
                    let _ = tx.send(UiEvent::Toast(format!(
                        "Private review queued ({job_id}). Track it in Background jobs."
                    )));
                }
                Err(error) => {
                    let _ = tx.send(UiEvent::Error(error.user_message()));
                }
            }
        });
    }
    if let Some(path) = state.review_comment_path.clone()
        && state.confirm_action.is_none()
    {
        let line = state.review_comment_line.unwrap_or_default();
        components::centered_modal(ctx, tokens, 500.0, |ui| {
            ui.label(
                RichText::new("Publish line comment")
                    .size(17.0)
                    .strong()
                    .color(tokens.text),
            );
            ui.label(
                RichText::new(format!(
                    "Publish the prepared comment at {path}:{line}? This is an externally visible GitHub mutation and requires the publish-review permission.",
                ))
                .size(10.5)
                .color(tokens.warning),
            );
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    state.review_comment_path = None;
                }
                if ui
                    .add(egui::Button::new("Publish comment").fill(tokens.accent))
                    .clicked()
                {
                    let service = Arc::clone(service);
                    let tx = tx.clone();
                    let pr_id = detail.pull_request.id.clone();
                    let body = state.review_comment_body.clone();
                    let path = path.clone();
                    runtime.spawn(async move {
                        match service
                            .publish_review_comment(&pr_id, &path, line, "RIGHT", &body, true)
                            .await
                        {
                            Ok(()) => {
                                let _ = tx.send(UiEvent::Toast("Review comment published.".into()));
                            }
                            Err(error) => {
                                let _ = tx.send(UiEvent::Error(error.user_message()));
                            }
                        }
                    });
                    state.review_comment_path = None;
                }
            });
        });
    }
    if submit_review {
        let mut event = state.review_submit_event.clone();
        let mut body = state.review_submit_body.clone();
        let mut confirm = false;
        let mut close = false;
        components::centered_modal(ctx, tokens, 520.0, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Submit GitHub review")
                        .size(17.0)
                        .strong()
                        .color(tokens.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
            ui.label(
                RichText::new(
                    "This is an external mutation. It requires the publish-review permission and, for approval, the approve/merge permission. Branch protection is never bypassed.",
                )
                .size(10.5)
                .color(tokens.warning),
            );
            ui.horizontal(|ui| {
                ui.selectable_value(&mut event, "COMMENT".into(), "Comment");
                ui.selectable_value(&mut event, "REQUEST_CHANGES".into(), "Request changes");
                ui.selectable_value(&mut event, "APPROVE".into(), "Approve");
            });
            ui.add(
                egui::TextEdit::multiline(&mut body)
                    .desired_rows(6)
                    .desired_width(f32::INFINITY)
                    .hint_text("Review summary and evidence"),
            );
            ui.horizontal(|ui| {
                if ui.button("Submit").clicked() {
                    confirm = true;
                }
                ui.label(
                    RichText::new(format!(
                        "Policy: publish={} approve={}",
                        state.bootstrap.permissions.publish_reviews,
                        state.bootstrap.permissions.approve_or_merge
                    ))
                    .size(9.5)
                    .color(tokens.text_faint),
                );
            });
        });
        let _ = close;
        state.review_submit_body = body.clone();
        state.review_submit_event = event.clone();
        if confirm {
            let id = pr.id.clone();
            let service = Arc::clone(service);
            let tx = tx.clone();
            runtime.spawn(async move {
                match service
                    .submit_pull_request_review(&id, &event, &body, true)
                    .await
                {
                    Ok(()) => {
                        let _ = tx.send(UiEvent::Toast(
                            "GitHub review submitted. Refresh to see the new decision.".into(),
                        ));
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
    }
    let _ = ctx;
}

fn overview_tab(
    ui: &mut egui::Ui,
    tokens: &Tokens,
    state: &UiState,
    detail: &gitpilot_core::PrDetail,
) {
    ui.columns(2, |cols| {
        let left = &mut cols[0];
        components::section_header(
            left,
            tokens,
            "What the repository says",
            Some("PR description is source data; generated analysis is separate"),
        );
        components::panel(tokens).show(left, |ui| {
            if detail.pull_request.body.trim().is_empty() {
                ui.label(
                    RichText::new("No PR description was provided.")
                        .size(11.5)
                        .color(tokens.warning),
                );
            } else {
                ui.label(
                    RichText::new(&detail.pull_request.body)
                        .size(11.5)
                        .color(tokens.text_muted),
                );
            }
        });
        left.add_space(8.0);
        components::section_header(left, tokens, "Review focus", Some("Derived from changed paths and CI"));
        components::panel(tokens).show(left, |ui| {
            let high_risk: Vec<_> = detail
                .files
                .iter()
                .filter(|f| {
                    f.path.contains("auth")
                        || f.path.contains("migration")
                        || f.path.contains("schema")
                })
                .collect();
            if high_risk.is_empty() {
                ui.label(
                    RichText::new("No high-risk path pattern was detected.")
                        .size(11.0_f32)
                        .color(tokens.text_muted),
                );
            }
            for file in high_risk {
                ui.horizontal(|ui| {
                    components::badge(ui, tokens, "review", tokens.warning);
                    ui.label(
                        RichText::new(&file.path)
                            .monospace()
                            .size(11.0_f32)
                            .color(tokens.text),
                    );
                });
            }
            if detail.pull_request.check_state == gitpilot_core::CheckState::Failure {
                ui.label(
                    RichText::new("CI is failing; inspect the checks tab before reviewing logic.")
                        .size(11.0_f32)
                        .color(tokens.danger),
                );
            }
        });
        let right = &mut cols[1];
        components::section_header(
            right,
            tokens,
            "Generated explanation",
            Some("AI inference · never treated as an observed fact"),
        );
        if let Some(explanation) = &state.explanation {
            components::panel(tokens).show(right, |ui| {
                ui.horizontal_wrapped(|ui| {
                    components::evidence_chip(ui, tokens, EvidenceKind::Inference, None);
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            explanation.provider, explanation.model
                        ))
                        .size(10.0)
                        .color(tokens.text_faint),
                    );
                });
                ui.label(
                    RichText::new(&explanation.executive)
                        .size(11.5)
                        .color(tokens.text_muted),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new("UNCERTAINTY")
                        .size(9.0)
                        .strong()
                        .color(tokens.text_faint),
                );
                for uncertainty in &explanation.uncertainties {
                    ui.label(
                        RichText::new(format!("· {uncertainty}"))
                            .size(10.5)
                            .color(tokens.text_faint),
                    );
                }
                if !explanation.file_notes.is_empty() {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("FILE-BY-FILE")
                            .size(9.0)
                            .strong()
                            .color(tokens.text_faint),
                    );
                    for note in explanation.file_notes.iter().take(12) {
                        ui.label(
                            RichText::new(format!("{} · {}", note.path, note.summary))
                                .size(10.0)
                                .color(tokens.text_muted),
                        );
                    }
                }
            });
        } else {
            components::empty_state(
                right,
                tokens,
                "Explanation not generated",
                "Generate a summary grounded in the PR body and cached diff. Provider usage and cost are recorded.",
                None,
            );
        }
        right.add_space(8.0);
        components::section_header(right, tokens, "Evidence links", None);
        components::panel(tokens).show(right, |ui| {
            let mut any = false;
            if !detail.linked_issues.is_empty() {
                any = true;
                for issue in &detail.linked_issues {
                    if ui
                        .link(RichText::new(format!("#{} {}", issue.number, issue.title)).size(10.5))
                        .clicked()
                    {
                        let _ = webbrowser::open(&issue.html_url);
                    }
                }
            }
            if !detail.related_prs.is_empty() {
                any = true;
                for related in &detail.related_prs {
                    if ui
                        .link(
                            RichText::new(format!("#{} {}", related.number, related.title))
                                .size(10.5),
                        )
                        .clicked()
                    {
                        let _ = webbrowser::open(&related.html_url);
                    }
                }
            }
            if !any {
                ui.label(
                    RichText::new("No linked issue or related PR was detected from cached metadata.")
                        .size(10.0)
                        .color(tokens.text_faint),
                );
            }
        });
        right.add_space(8.0);
        components::section_header(right, tokens, "Merge readiness", None);
        components::panel(tokens).show(right, |ui| {
            components::kv(ui, tokens, "Human review", detail.pull_request.review_state.label());
            components::kv(ui, tokens, "Checks", detail.pull_request.check_state.label());
            components::kv(ui, tokens, "Mergeability", &format!("{:?}", detail.pull_request.mergeable));
            components::kv(ui, tokens, "Draft", if detail.pull_request.draft { "yes" } else { "no" });
            if detail.pull_request.is_ready() {
                ui.label(
                    RichText::new("Signals indicate ready, but approval/merge still requires explicit action.")
                        .size(10.5)
                        .color(tokens.warning),
                );
            }
        });
    });
}

fn files_tab(
    ui: &mut egui::Ui,
    tokens: &Tokens,
    state: &mut UiState,
    detail: &gitpilot_core::PrDetail,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.diff_mode, DiffMode::Unified, "Unified");
        ui.selectable_value(&mut state.diff_mode, DiffMode::SideBySide, "Side by side");
        ui.checkbox(&mut state.diff_show_generated, "Show generated files");
        ui.checkbox(&mut state.diff_ignore_whitespace, "Whitespace changes");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{} files cached", detail.files.len()))
                    .size(10.0)
                    .color(tokens.text_faint),
            );
        });
    });
    let files: Vec<_> = detail
        .files
        .iter()
        .filter(|f| state.diff_show_generated || !f.generated)
        .collect();
    if detail.files.len() > 300 {
        components::inset(tokens).show(ui, |ui| {
            ui.label(
                RichText::new(format!(
                    "Large PR: {} cached files. The viewer shows files lazily by selection; the GitHub API can omit patches beyond its file limit.",
                    detail.files.len()
                ))
                .size(10.0)
                .color(tokens.warning),
            );
        });
        ui.add_space(5.0);
    }
    if files.is_empty() {
        components::empty_state(
            ui,
            tokens,
            "No cached diff",
            "Synchronize the PR from GitHub to load file patches. Large files may be omitted by the API.",
            None,
        );
        return;
    }
    let selected = state
        .diff_file
        .clone()
        .or_else(|| files.first().map(|f| f.path.clone()));
    ui.columns(2, |cols| {
        let left = &mut cols[0];
        components::section_header(
            left,
            tokens,
            "Files",
            Some("Click a file to inspect its hunks"),
        );
        egui::ScrollArea::vertical()
            .id_salt("diff_files")
            .max_height(560.0)
            .show(left, |ui| {
                for file in &files {
                    let response = ui
                        .selectable_label(
                            selected.as_deref() == Some(file.path.as_str()),
                            RichText::new(format!(
                                "{}  {}\n+{}  −{}",
                                match file.status {
                                    gitpilot_core::DiffStatus::Added => "A",
                                    gitpilot_core::DiffStatus::Deleted => "D",
                                    gitpilot_core::DiffStatus::Renamed => "R",
                                    gitpilot_core::DiffStatus::Modified => "M",
                                },
                                file.path,
                                file.additions,
                                file.deletions
                            ))
                            .monospace()
                            .size(10.5),
                        )
                        .on_hover_text(&file.path);
                    if response.clicked() {
                        state.diff_file = Some(file.path.clone());
                    }
                    if file.binary {
                        ui.label(
                            RichText::new("binary · content not rendered")
                                .size(9.5)
                                .color(tokens.text_faint),
                        );
                    }
                }
            });
        let right = &mut cols[1];
        if let Some(file) = selected
            .as_deref()
            .and_then(|path| files.iter().find(|f| f.path == path))
        {
            components::section_header(
                right,
                tokens,
                &file.path,
                Some(&format!(
                    "{} · +{} −{}",
                    file.status_label(),
                    file.additions,
                    file.deletions
                )),
            );
            if detail.pull_request.origin == gitpilot_core::DataOrigin::GitHub
                && right.small_button("Inspect source").clicked()
            {
                let path = file.path.clone();
                let repository_id = detail.pull_request.repository_id.clone();
                let base_sha = detail.pull_request.base_sha.clone();
                state.source_preview = Some((path.clone(), "Loading…".into()));
                let service = Arc::clone(service);
                let tx = tx.clone();
                runtime.spawn(async move {
                    match service
                        .repository_file(&repository_id, &path, &base_sha)
                        .await
                    {
                        Ok(content) => {
                            let _ = tx.send(UiEvent::RepositoryFile(content));
                        }
                        Err(error) => {
                            let _ = tx.send(UiEvent::Error(error.user_message()));
                        }
                    }
                });
            }
            if let Some((path, line)) = right.ctx().memory(|memory| {
                memory
                    .data
                    .get_temp::<(String, i64)>(egui::Id::new("review_comment_anchor"))
            }) && path == file.path
            {
                right.label(
                    RichText::new(format!("Comment anchor: {path}:{line}"))
                        .monospace()
                        .size(10.0)
                        .color(tokens.info),
                );
                let mut body = state.review_comment_body.clone();
                let response = right.add(
                    egui::TextEdit::multiline(&mut body)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text("Comment anchored to this validated diff line"),
                );
                state.review_comment_body = body;
                if right.small_button("Publish line comment…").clicked() {
                    state.review_comment_path = Some(path);
                    state.review_comment_line = Some(line);
                    state.confirm_action = Some("publish_line_comment".into());
                }
                let _ = response;
            }
            if let Some((preview_path, content)) = &state.source_preview
                && preview_path == &file.path
            {
                egui::ScrollArea::both()
                    .id_salt("source_preview")
                    .max_height(140.0)
                    .show(right, |ui| {
                        ui.label(
                            RichText::new(content)
                                .monospace()
                                .size(9.0)
                                .color(tokens.text_muted),
                        );
                    });
            }
            egui::ScrollArea::both()
                .id_salt("diff_content")
                .max_height(570.0)
                .show(right, |ui| {
                    if file.binary {
                        components::empty_state(
                            ui,
                            tokens,
                            "Binary file",
                            "This file has no text patch. Open it on GitHub for a trusted preview.",
                            None,
                        );
                    } else if file.hunks.is_empty() {
                        ui.label(
                            RichText::new("Patch data was unavailable for this file.")
                                .size(11.0_f32)
                                .color(tokens.warning),
                        );
                    } else {
                        render_diff(ui, tokens, file, state.diff_mode);
                    }
                });
        }
    });
}

fn render_diff(ui: &mut egui::Ui, tokens: &Tokens, file: &DiffFile, mode: DiffMode) {
    egui::Frame::new()
        .fill(tokens.sidebar)
        .stroke(egui::Stroke::new(1.0_f32, tokens.border))
        .corner_radius(5.0)
        .show(ui, |ui| {
            for hunk in &file.hunks {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width().max(500.0), 20.0),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(rect, 0.0, tokens.raised);
                ui.painter().text(
                    rect.left_center() + egui::vec2(8.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &hunk.header,
                    egui::FontId::monospace(10.0),
                    tokens.info,
                );
                for line in &hunk.lines {
                    let color = match line.kind {
                        DiffLineKind::Added => tokens.success,
                        DiffLineKind::Removed => tokens.danger,
                        DiffLineKind::Context => tokens.text_muted,
                        DiffLineKind::Meta => tokens.text_faint,
                    };
                    let bg = match line.kind {
                        DiffLineKind::Added => {
                            egui::Color32::from_rgba_premultiplied(87, 195, 138, 15)
                        }
                        DiffLineKind::Removed => {
                            egui::Color32::from_rgba_premultiplied(232, 99, 109, 15)
                        }
                        _ => tokens.sidebar,
                    };
                    let marker = match line.kind {
                        DiffLineKind::Added => "+",
                        DiffLineKind::Removed => "−",
                        DiffLineKind::Context => " ",
                        DiffLineKind::Meta => "\\",
                    };
                    let old = line.old_line.map(|n| n.to_string()).unwrap_or_default();
                    let new = line.new_line.map(|n| n.to_string()).unwrap_or_default();
                    let text = match mode {
                        DiffMode::Unified => format!("{old:>4} {new:>4} {marker} {}", line.content),
                        DiffMode::SideBySide => format!(
                            "{old:>4} {marker} {:<48} │ {new:>4} {}",
                            if line.kind == DiffLineKind::Removed {
                                line.content.as_str()
                            } else {
                                ""
                            },
                            if line.kind == DiffLineKind::Removed {
                                ""
                            } else {
                                line.content.as_str()
                            }
                        ),
                    };
                    let galley =
                        ui.painter()
                            .layout_no_wrap(text, egui::FontId::monospace(10.5), color);
                    let height = galley.size().y.max(16.0);
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width().max(500.0), height),
                        egui::Sense::click(),
                    );
                    ui.painter().rect_filled(rect, 0.0, bg);
                    ui.painter()
                        .galley(rect.left_top() + egui::vec2(5.0, 1.0_f32), galley, color);
                    let response = response.on_hover_text(format!(
                        "old line {:?} · new line {:?}",
                        line.old_line, line.new_line
                    ));
                    if response.clicked()
                        && let Some(new_line) = line.new_line
                    {
                        ui.ctx().memory_mut(|memory| {
                            memory.data.insert_temp(
                                egui::Id::new("review_comment_anchor"),
                                (file.path.clone(), new_line),
                            );
                        });
                    }
                }
            }
        });
    ui.label(
        RichText::new("Line numbers are parsed from the actual unified diff. Comments are never anchored to invented positions.")
            .size(9.5)
            .color(tokens.text_faint),
    );
}

fn review_tab(
    ui: &mut egui::Ui,
    tokens: &Tokens,
    state: &mut UiState,
    detail: &gitpilot_core::PrDetail,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    if state.review_history.is_empty()
        && let Some(pr_id) = state.selected_pr.clone()
    {
        let service = Arc::clone(service);
        let tx = tx.clone();
        runtime.spawn(async move {
            if let Ok(runs) = service.store.review_runs(&pr_id).await {
                let _ = tx.send(UiEvent::ReviewHistory(runs));
            }
        });
    }
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{} findings", detail.findings.len()))
                .size(12.0)
                .strong()
                .color(tokens.text),
        );
        ui.label(
            RichText::new("Private findings · no GitHub review was published")
                .size(10.0)
                .color(tokens.text_faint),
        );
    });
    if let Some(run) = state
        .review_run
        .as_ref()
        .or(detail.latest_review_run.as_ref())
    {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                components::badge(ui, tokens, &format!("{:?}", run.status), tokens.info);
                ui.label(
                    RichText::new(format!(
                        "{} deterministic · {} AI · {} tokens · ${:.4}",
                        run.deterministic_findings, run.ai_findings, run.tokens, run.cost_usd
                    ))
                    .size(10.5)
                    .color(tokens.text_muted),
                );
                if run.provider.is_some() {
                    ui.label(
                        RichText::new(format!(
                            "{} / {}",
                            run.provider.as_deref().unwrap_or(""),
                            run.model.as_deref().unwrap_or("")
                        ))
                        .size(10.0)
                        .color(tokens.text_faint),
                    );
                }
            });
            if let Some(error) = &run.error {
                ui.label(RichText::new(error).size(10.5).color(tokens.warning));
            }
        });
        ui.add_space(6.0);
    }
    if state.review_history.len() > 1 {
        ui.collapsing("Review history", |ui| {
            for run in state.review_history.iter().take(12) {
                ui.horizontal(|ui| {
                    components::badge(ui, tokens, &format!("{:?}", run.status), tokens.info);
                    ui.label(
                        RichText::new(format!(
                            "{} · {} findings · {} / {}",
                            run.started_at.format("%Y-%m-%d %H:%M"),
                            run.deterministic_findings + run.ai_findings,
                            run.provider.as_deref().unwrap_or("—"),
                            run.model.as_deref().unwrap_or("—")
                        ))
                        .size(10.0)
                        .color(tokens.text_muted),
                    );
                });
            }
        });
    }
    if detail.findings.is_empty() {
        components::empty_state(
            ui,
            tokens,
            "No findings yet",
            "Run a private review to apply deterministic checks and optionally AI analysis.",
            None,
        );
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("findings")
        .max_height(580.0)
        .show(ui, |ui| {
            for finding in &detail.findings {
                components::panel(tokens).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        components::severity_badge(ui, tokens, finding.severity);
                        components::badge(
                            ui,
                            tokens,
                            &format!("{}%", (finding.confidence * 100.0) as i64),
                            tokens.text_muted,
                        );
                        components::badge(ui, tokens, &finding.category, tokens.info);
                        components::badge(ui, tokens, &finding.source, tokens.text_faint);
                        ui.label(
                            RichText::new(&finding.title)
                                .size(12.5)
                                .strong()
                                .color(tokens.text),
                        );
                    });
                    ui.label(
                        RichText::new(&finding.explanation)
                            .size(11.0_f32)
                            .color(tokens.text_muted),
                    );
                    ui.label(
                        RichText::new(format!("Why: {}", finding.why_it_matters))
                            .size(10.5)
                            .color(tokens.text_faint),
                    );
                    if let Some(path) = &finding.path {
                        ui.label(
                            RichText::new(format!(
                                "{}:{}",
                                path,
                                finding
                                    .diff_line
                                    .map(|l| l.to_string())
                                    .unwrap_or_else(|| "no anchor".into())
                            ))
                            .monospace()
                            .size(10.0)
                            .color(tokens.info),
                        );
                    }
                    ui.horizontal(|ui| {
                        if ui.small_button("Confirm").clicked() {
                            update_finding(
                                finding.id.clone(),
                                FindingLifecycle::Confirmed,
                                service,
                                runtime,
                                tx,
                            );
                        }
                        if ui.small_button("Dismiss").clicked() {
                            update_finding(
                                finding.id.clone(),
                                FindingLifecycle::Dismissed,
                                service,
                                runtime,
                                tx,
                            );
                        }
                        if ui.small_button("False positive").clicked() {
                            update_finding(
                                finding.id.clone(),
                                FindingLifecycle::FalsePositive,
                                service,
                                runtime,
                                tx,
                            );
                        }
                        ui.label(
                            RichText::new(format!("{:?}", finding.lifecycle))
                                .size(10.0)
                                .color(tokens.text_faint),
                        );
                    });
                });
                ui.add_space(5.0);
            }
        });
}

fn update_finding(
    id: String,
    lifecycle: FindingLifecycle,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    let service = Arc::clone(service);
    let tx = tx.clone();
    runtime.spawn(async move {
        match service.update_finding(&id, lifecycle).await {
            Ok(()) => {
                let _ = tx.send(UiEvent::BootstrapRefreshed);
            }
            Err(error) => {
                let _ = tx.send(UiEvent::Error(error.user_message()));
            }
        }
    });
}

fn commits_tab(ui: &mut egui::Ui, tokens: &Tokens, detail: &gitpilot_core::PrDetail) {
    if detail.commits.is_empty() {
        components::empty_state(
            ui,
            tokens,
            "No commits indexed for this PR",
            "The GitHub PR commit list is synchronized separately from the global commit cache.",
            None,
        );
        return;
    }
    for commit in &detail.commits {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(commit.sha.chars().take(8).collect::<String>())
                        .monospace()
                        .size(10.5)
                        .color(tokens.info),
                );
                ui.label(
                    RichText::new(commit.message.lines().next().unwrap_or(""))
                        .size(11.5)
                        .color(tokens.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(components::short_time(commit.committed_at))
                            .size(10.0)
                            .color(tokens.text_faint),
                    );
                });
            });
        });
        ui.add_space(4.0);
    }
}

fn checks_tab(ui: &mut egui::Ui, tokens: &Tokens, detail: &gitpilot_core::PrDetail) {
    if detail.checks.is_empty() {
        components::empty_state(
            ui,
            tokens,
            "No checks cached",
            "GitHub may not report checks for this commit, or the token lacks checks:read.",
            None,
        );
        return;
    }
    for check in &detail.checks {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                components::status_dot(ui, tokens.check(check.state));
                ui.label(
                    RichText::new(&check.name)
                        .size(11.5)
                        .strong()
                        .color(tokens.text),
                );
                ui.label(
                    RichText::new(check.conclusion.as_deref().unwrap_or("pending"))
                        .size(10.5)
                        .color(tokens.text_muted),
                );
            });
        });
        ui.add_space(4.0);
    }
}

fn discussion_tab(ui: &mut egui::Ui, tokens: &Tokens, detail: &gitpilot_core::PrDetail) {
    if detail.comments.is_empty() {
        components::empty_state(
            ui,
            tokens,
            "No review discussion cached",
            "Review comments are fetched during PR synchronization.",
            None,
        );
        return;
    }
    for comment in &detail.comments {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                components::status_dot(ui, tokens.accent);
                ui.label(
                    RichText::new(&comment.author)
                        .size(11.5)
                        .strong()
                        .color(tokens.text),
                );
                if let Some(path) = &comment.path {
                    ui.label(
                        RichText::new(format!(
                            "{}:{}",
                            path,
                            comment.line.map(|l| l.to_string()).unwrap_or_default()
                        ))
                        .monospace()
                        .size(10.0)
                        .color(tokens.info),
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(components::short_time(comment.submitted_at))
                            .size(10.0)
                            .color(tokens.text_faint),
                    );
                });
            });
            ui.label(
                RichText::new(&comment.body)
                    .size(11.0_f32)
                    .color(tokens.text_muted),
            );
        });
        ui.add_space(4.0);
    }
}

trait DiffStatusLabel {
    fn status_label(&self) -> &'static str;
}

impl DiffStatusLabel for DiffFile {
    fn status_label(&self) -> &'static str {
        match self.status {
            gitpilot_core::DiffStatus::Added => "added",
            gitpilot_core::DiffStatus::Deleted => "deleted",
            gitpilot_core::DiffStatus::Renamed => "renamed",
            gitpilot_core::DiffStatus::Modified => "modified",
        }
    }
}
