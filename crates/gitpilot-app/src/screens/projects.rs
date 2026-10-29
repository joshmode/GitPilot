use crate::components;
use crate::state::{UiEvent, UiState};
use crate::theme::Tokens;
use eframe::egui::{self, Align, Layout, RichText};
use gitpilot_core::AppService;
use std::collections::BTreeMap;
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
    if state.milestones.is_empty() {
        let service = Arc::clone(service);
        let tx = tx.clone();
        let repositories = state.bootstrap.repositories.clone();
        runtime.spawn(async move {
            let mut rows = Vec::new();
            for repo in repositories {
                if let Ok(milestones) = service.store.milestones(&repo.id).await {
                    for milestone in milestones {
                        rows.push((repo.full_name.clone(), milestone));
                    }
                }
            }
            let _ = tx.send(UiEvent::Milestones(rows));
        });
    }
    components::page_header(
        ui,
        tokens,
        "Project workspaces",
        "Group repositories into logical projects and track operational progress",
        |ui| {
            if ui.button("New workspace").clicked() {
                state.error = Some(
                    "Workspace assignment is available from the repository card menu; creating an empty workspace is deferred."
                        .into(),
                );
            }
        },
    );
    let mut groups: BTreeMap<String, Vec<_>> = BTreeMap::new();
    for repo in &state.bootstrap.repositories {
        groups
            .entry(repo.workspace.clone().unwrap_or_else(|| "Ungrouped".into()))
            .or_default()
            .push(repo);
    }
    for (name, repos) in groups {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&name).size(15.0).strong().color(tokens.text));
                components::badge(ui, tokens, &format!("{} repos", repos.len()), tokens.info);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new("progress from measurable indicators only")
                            .size(10.0)
                            .color(tokens.text_faint),
                    );
                });
            });
            ui.add_space(5.0);
            for repo in repos {
                ui.horizontal(|ui| {
                    components::status_dot(
                        ui,
                        if repo.origin == gitpilot_core::DataOrigin::Demo {
                            tokens.warning
                        } else {
                            tokens.success
                        },
                    );
                    ui.label(RichText::new(&repo.full_name).size(11.5).color(tokens.text));
                    if let Some(health) = state
                        .bootstrap
                        .dashboard
                        .health
                        .iter()
                        .find(|h| h.repository_id == repo.id)
                    {
                        ui.label(
                            RichText::new(format!(
                                "{} open PRs · {} review · {} CI fail",
                                health.open_prs, health.review_backlog, health.failing_checks
                            ))
                            .size(10.0)
                            .color(tokens.text_faint),
                        );
                    }
                });
            }
        });
        ui.add_space(7.0);
    }
    components::section_header(
        ui,
        tokens,
        "Milestones",
        Some("GitHub-reported open and closed issue counts"),
    );
    let milestone_rows = state
        .milestones
        .iter()
        .map(|(repo, milestone)| (repo.clone(), milestone.clone()))
        .collect::<Vec<_>>();
    if milestone_rows.is_empty() {
        ui.label(
            RichText::new(
                "No milestone snapshots cached. Synchronize a live repository or use demo data.",
            )
            .size(10.5)
            .color(tokens.text_faint),
        );
    }
    for (repo, milestone) in milestone_rows.into_iter().take(20) {
        let total = milestone.open_issues + milestone.closed_issues;
        let ratio = if total > 0 {
            milestone.closed_issues as f32 / total as f32
        } else {
            0.0
        };
        components::inset(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&milestone.title)
                        .size(11.5)
                        .strong()
                        .color(tokens.text),
                );
                components::badge(ui, tokens, &repo, tokens.text_faint);
                components::badge(
                    ui,
                    tokens,
                    &format!("{:.0}% complete", ratio * 100.0),
                    if ratio >= 0.7 {
                        tokens.success
                    } else {
                        tokens.warning
                    },
                );
                ui.label(
                    RichText::new(format!(
                        "{} open · {} closed",
                        milestone.open_issues, milestone.closed_issues
                    ))
                    .size(10.0)
                    .color(tokens.text_muted),
                );
                if let Some(due) = milestone.due_on {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("due {}", due.format("%b %d")))
                                .size(10.0)
                                .color(if due < chrono::Utc::now() {
                                    tokens.danger
                                } else {
                                    tokens.text_faint
                                }),
                        );
                    });
                }
            });
            ui.add(
                egui::ProgressBar::new(ratio)
                    .desired_width(ui.available_width())
                    .fill(tokens.accent),
            );
        });
        ui.add_space(4.0);
    }
    components::inset(tokens).show(ui, |ui| {
        ui.label(
            RichText::new(
                "Progress note: GitHub Project completion and inferred project progress are different measures. GitPilot only reports milestone ratios when both open and closed issue data are cached.",
            )
            .size(10.5)
            .color(tokens.text_muted),
        );
    });
    components::section_header(
        ui,
        tokens,
        "GitHub Projects v2",
        Some("Repository-owner projects fetched through the GitHub GraphQL API"),
    );
    if state.bootstrap.github_projects.is_empty() {
        ui.label(
            RichText::new(
                "No Projects v2 data is cached. Synchronize a live repository whose owner exposes Projects v2, or verify token access. Unavailable project data is not treated as zero progress.",
            )
            .size(10.5)
            .color(tokens.text_faint),
        );
    }
    for project in state.bootstrap.github_projects.iter().take(20) {
        let total = project.items.len();
        let closed = project
            .items
            .iter()
            .filter(|item| {
                matches!(
                    item.state.to_ascii_lowercase().as_str(),
                    "closed" | "done" | "merged"
                )
            })
            .count();
        let ratio = if total == 0 {
            None
        } else {
            Some(closed as f32 / total as f32)
        };
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&project.title)
                        .size(12.5)
                        .strong()
                        .color(tokens.text),
                );
                components::badge(
                    ui,
                    tokens,
                    &format!("#{}", project.number),
                    tokens.text_faint,
                );
                components::badge(
                    ui,
                    tokens,
                    &format!("{} items", project.items.len()),
                    tokens.info,
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("Open").clicked() {
                        let _ = webbrowser::open(&project.url);
                    }
                });
            });
            ui.label(
                RichText::new(format!(
                    "{} · GitHub-provided status · synced {}",
                    project.owner_login,
                    components::short_time(project.synced_at)
                ))
                .size(10.0)
                .color(tokens.text_faint),
            );
            match ratio {
                Some(ratio) => {
                    ui.add(
                        egui::ProgressBar::new(ratio)
                            .desired_width(ui.available_width())
                            .text(format!(
                                "{closed}/{total} items in a closed/done state (GitHub status)"
                            ))
                            .fill(tokens.accent),
                    );
                }
                None => {
                    ui.label(
                        RichText::new("No project items returned; completion is unavailable.")
                            .size(10.0)
                            .color(tokens.text_faint),
                    );
                }
            }
            for item in project.items.iter().take(8) {
                ui.horizontal(|ui| {
                    components::badge(ui, tokens, &item.state, tokens.text_muted);
                    if let Some(url) = &item.url {
                        if ui
                            .link(RichText::new(&item.title).size(10.5))
                            .on_hover_text("Open on GitHub")
                            .clicked()
                        {
                            let _ = webbrowser::open(url);
                        }
                    } else {
                        ui.label(
                            RichText::new(&item.title)
                                .size(10.5)
                                .color(tokens.text_muted),
                        );
                    }
                    if let Some(repo) = &item.repository_name {
                        ui.label(
                            RichText::new(format!("· {repo}"))
                                .size(9.5)
                                .color(tokens.text_faint),
                        );
                    }
                });
            }
        });
        ui.add_space(5.0);
    }
    let _ = (runtime, tx);
}

pub fn activity(
    ui: &mut egui::Ui,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    components::page_header(
        ui,
        tokens,
        "Activity & reports",
        "Cross-repository timeline, release history and exportable project evidence",
        |ui| {
            if ui.button("Export brief").clicked() {
                let service = Arc::clone(service);
                let tx = tx.clone();
                let data_dir = state.bootstrap.data_dir.clone();
                runtime.spawn(async move {
                    match service.export_project_brief(&data_dir).await {
                        Ok(path) => {
                            let _ = tx.send(UiEvent::Toast(format!(
                                "Project brief exported to {}",
                                path.display()
                            )));
                        }
                        Err(error) => {
                            let _ = tx.send(UiEvent::Error(error.user_message()));
                        }
                    }
                });
            }
            if ui.button("Generate project brief").clicked() {
                let service = Arc::clone(service);
                let tx = tx.clone();
                runtime.spawn(async move {
                    match service.project_brief().await {
                        Ok(brief) => {
                            let _ = tx.send(UiEvent::Brief(brief));
                        }
                        Err(error) => {
                            let _ = tx.send(UiEvent::Error(error.user_message()));
                        }
                    }
                });
            }
            if ui.button("Generate release report").clicked()
                && let Some(repo) = state
                    .repository_scope
                    .clone()
                    .or_else(|| state.bootstrap.repositories.first().map(|r| r.id.clone()))
            {
                let service = Arc::clone(service);
                let tx = tx.clone();
                runtime.spawn(async move {
                    match service.release_report(&repo).await {
                        Ok(report) => {
                            let _ = tx.send(UiEvent::ReleaseReport(Box::new(report)));
                        }
                        Err(error) => {
                            let _ = tx.send(UiEvent::Error(error.user_message()));
                        }
                    }
                });
            }
            if ui.button("Publish draft…").clicked()
                && let Some(repo) = state
                    .repository_scope
                    .clone()
                    .or_else(|| state.bootstrap.repositories.first().map(|r| r.id.clone()))
            {
                state.release_repository = Some(repo);
                state.release_tag = state
                    .release_report
                    .as_ref()
                    .map(|r| r.tag_name.clone())
                    .unwrap_or_default();
                state.release_draft = true;
                state.release_prerelease = false;
            }
        },
    );
    if let Some(brief) = &state.brief {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                components::badge(ui, tokens, &brief.range, tokens.info);
                components::badge(
                    ui,
                    tokens,
                    &format!(
                        "freshness: {}",
                        brief
                            .freshness
                            .map(components::short_time)
                            .unwrap_or_else(|| "unavailable".into())
                    ),
                    tokens.text_muted,
                );
            });
            for (label, statements) in [
                ("Observed facts", &brief.facts),
                ("Derived metrics", &brief.metrics),
                ("AI inference", &brief.inferences),
                ("Recommendations", &brief.next_steps),
            ] {
                if statements.is_empty() {
                    continue;
                }
                ui.label(
                    RichText::new(label)
                        .size(9.5)
                        .strong()
                        .color(tokens.text_faint),
                );
                for item in statements.iter().take(8) {
                    ui.horizontal_wrapped(|ui| {
                        components::evidence_chip(ui, tokens, item.kind, None);
                        ui.label(
                            RichText::new(&item.text)
                                .size(10.5)
                                .color(tokens.text_muted),
                        );
                        for evidence in item.evidence.iter().take(2) {
                            if let Some(url) = &evidence.url
                                && ui.small_button(&evidence.label).clicked()
                            {
                                let _ = webbrowser::open(url);
                            }
                        }
                    });
                }
            }
            if !brief.missing.is_empty() {
                ui.label(
                    RichText::new(format!("Missing data: {}", brief.missing.join(" · ")))
                        .size(10.0)
                        .color(tokens.warning),
                );
            }
        });
        ui.add_space(8.0);
    }
    if let Some(repo_id) = state.release_repository.clone() {
        let mut tag = state.release_tag.clone();
        let mut name = state.release_name.clone();
        let mut body = state
            .release_report
            .as_ref()
            .map(|r| r.draft_notes.clone())
            .unwrap_or_default();
        let mut draft = state.release_draft;
        let mut prerelease = state.release_prerelease;
        let mut confirm = false;
        let mut close = false;
        components::centered_modal(ui.ctx(), tokens, 620.0, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Publish GitHub release")
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
                    "This is an external mutation. Draft mode creates an unpublished release; publishing requires Modify GitHub permission and confirmation.",
                )
                .size(10.5)
                .color(tokens.warning),
            );
            ui.horizontal(|ui| {
                ui.label("Tag");
                ui.add(egui::TextEdit::singleline(&mut tag).desired_width(180.0));
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut name).desired_width(260.0));
            });
            ui.checkbox(&mut draft, "Create as draft");
            ui.checkbox(&mut prerelease, "Mark as prerelease");
            ui.add(
                egui::TextEdit::multiline(&mut body)
                    .desired_rows(10)
                    .desired_width(f32::INFINITY)
                    .hint_text("Release notes"),
            );
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                if ui
                    .add(egui::Button::new("Create release").fill(tokens.accent))
                    .clicked()
                {
                    confirm = true;
                }
                ui.label(
                    RichText::new(format!(
                        "Repository: {} · Modify GitHub: {}",
                        state
                            .bootstrap
                            .repositories
                            .iter()
                            .find(|r| r.id == repo_id)
                            .map(|r| r.full_name.as_str())
                            .unwrap_or("—"),
                        state.bootstrap.permissions.modify_github
                    ))
                    .size(9.5)
                    .color(tokens.text_faint),
                );
            });
        });
        if close {
            state.release_repository = None;
        }
        if confirm {
            let service = Arc::clone(service);
            let tx = tx.clone();
            runtime.spawn(async move {
                match service
                    .publish_release(&repo_id, &tag, &name, &body, draft, prerelease, true)
                    .await
                {
                    Ok(url) => {
                        let _ = tx.send(UiEvent::Toast(format!("Release created: {url}")));
                        let _ = tx.send(UiEvent::BootstrapRefreshed);
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
            state.release_repository = None;
        }
    }
    if let Some(report) = &state.release_report {
        components::panel(tokens).show(ui, |ui| {
            ui.label(
                RichText::new("Release readiness")
                    .size(14.0)
                    .strong()
                    .color(tokens.text),
            );
            ui.label(
                RichText::new(format!(
                    "{} · {} merged PRs · generated {}",
                    report.tag_name,
                    report.merged_prs.len(),
                    report.generated_at.format("%Y-%m-%d %H:%M")
                ))
                .size(10.0)
                .color(tokens.text_faint),
            );
            for item in &report.readiness {
                ui.horizontal_wrapped(|ui| {
                    components::evidence_chip(ui, tokens, item.kind, None);
                    ui.label(
                        RichText::new(&item.text)
                            .size(10.5)
                            .color(tokens.text_muted),
                    );
                });
            }
            if !report.blockers.is_empty() {
                ui.label(
                    RichText::new("BLOCKERS")
                        .size(9.0)
                        .strong()
                        .color(tokens.danger),
                );
                for item in &report.blockers {
                    ui.label(
                        RichText::new(format!("· {}", item.text))
                            .size(10.0)
                            .color(tokens.warning),
                    );
                }
            }
            if !report.breaking_changes.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "{} potential breaking change(s)",
                        report.breaking_changes.len()
                    ))
                    .size(10.0)
                    .color(tokens.warning),
                );
            }
            if !report.migration_changes.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "{} migration/schema change(s) detected",
                        report.migration_changes.len()
                    ))
                    .size(10.0)
                    .color(tokens.warning),
                );
            }
            if !report.dependency_changes.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "{} dependency-lock change(s) detected",
                        report.dependency_changes.len()
                    ))
                    .size(10.0)
                    .color(tokens.info),
                );
            }
            if !report.draft_notes.is_empty() {
                ui.collapsing("Draft release notes (review before use)", |ui| {
                    ui.label(
                        RichText::new(&report.draft_notes)
                            .monospace()
                            .size(9.5)
                            .color(tokens.text_muted),
                    );
                });
            }
        });
        ui.add_space(8.0);
    }
    for item in &state.bootstrap.dashboard.activity {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                components::badge(ui, tokens, &item.kind, tokens.info);
                ui.label(
                    RichText::new(&item.title)
                        .size(12.0)
                        .strong()
                        .color(tokens.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(components::short_time(item.occurred_at))
                            .size(10.0)
                            .color(tokens.text_faint),
                    );
                });
            });
            ui.label(
                RichText::new(format!("{} · {}", item.repository, item.actor))
                    .size(10.5)
                    .color(tokens.text_muted),
            );
        });
        ui.add_space(4.0);
    }
    components::section_header(ui, tokens, "Recent releases", None);
    for release in &state.bootstrap.dashboard.recent_releases {
        components::inset(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&release.tag_name)
                        .monospace()
                        .size(11.0_f32)
                        .color(tokens.info),
                );
                ui.label(
                    RichText::new(&release.name)
                        .size(12.0)
                        .strong()
                        .color(tokens.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("Open").clicked() {
                        let _ = webbrowser::open(&release.html_url);
                    }
                });
            });
        });
        ui.add_space(4.0);
    }
}

pub fn onboarding(
    ui: &mut egui::Ui,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    use crate::state::OnboardingStep;
    components::page_header(
        ui,
        tokens,
        "First-run setup",
        "Connect only what you need; demo browsing and deterministic review work without providers",
        |_ui| {},
    );
    let steps = [
        (OnboardingStep::Welcome, "1  Welcome"),
        (OnboardingStep::GitHub, "2  GitHub"),
        (OnboardingStep::Repositories, "3  Repositories"),
        (OnboardingStep::Ai, "4  AI providers"),
        (OnboardingStep::Privacy, "5  Privacy"),
        (OnboardingStep::Review, "6  Review policy"),
        (OnboardingStep::Notifications, "7  Notifications"),
        (OnboardingStep::Finish, "8  Finish"),
    ];
    ui.horizontal_wrapped(|ui| {
        for (step, label) in steps {
            if ui
                .selectable_label(state.onboarding_step == step, label)
                .clicked()
            {
                state.onboarding_step = step;
            }
        }
    });
    components::divider(ui, tokens);
    match state.onboarding_step {
        OnboardingStep::Welcome => {
            components::panel(tokens).show(ui, |ui| {
                ui.label(
                    RichText::new("A command centre, not another dashboard")
                        .size(18.0)
                        .strong()
                        .color(tokens.text),
                );
                ui.label(
                    RichText::new(
                        "GitPilot combines real GitHub metadata, persisted project memory and private AI review. Demo data is labelled everywhere. Nothing is published to GitHub without explicit authorization.",
                    )
                    .size(11.5)
                    .color(tokens.text_muted),
                );
                components::divider(ui, tokens);
                for point in [
                    "Read-only GitHub sync with ETags and rate-limit handling",
                    "Private review findings with validated diff anchors",
                    "OS keychain credentials and explicit AI data-transfer consent",
                    "Local Git inspection and allow-listed structured commands",
                ] {
                    ui.horizontal(|ui| {
                        components::status_dot(ui, tokens.success);
                        ui.label(RichText::new(point).size(11.0_f32).color(tokens.text_muted));
                    });
                }
            });
        }
        OnboardingStep::GitHub => {
            components::panel(tokens).show(ui, |ui| {
                ui.label(
                    RichText::new("Connect GitHub when you are ready")
                        .size(14.0)
                        .strong()
                        .color(tokens.text),
                );
                ui.label(
                    RichText::new(
                        "Fine-grained PAT scopes: Metadata (read), Contents (read), Issues (read), Pull requests (read), Checks (read). Organization SSO may need approval.",
                    )
                    .size(11.0_f32)
                    .color(tokens.text_muted),
                );
                if ui.button("Open connection dialog").clicked() {
                    state.github_token.clear();
                    state.account_menu = true;
                }
                ui.label(
                    RichText::new(format!(
                        "{} account(s) configured",
                        state.bootstrap.accounts.len()
                    ))
                    .size(10.5)
                    .color(tokens.text_faint),
                );
            });
        }
        OnboardingStep::Repositories => {
            components::panel(tokens).show(ui, |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} repositories are available",
                        state.bootstrap.repositories.len()
                    ))
                    .size(13.0)
                    .strong()
                    .color(tokens.text),
                );
                for repo in state.bootstrap.repositories.iter().take(8) {
                    ui.horizontal(|ui| {
                        components::status_dot(ui, tokens.accent);
                        ui.label(
                            RichText::new(&repo.full_name)
                                .size(11.0_f32)
                                .color(tokens.text_muted),
                        );
                        if repo.favorite {
                            components::badge(ui, tokens, "pinned", tokens.warning);
                        }
                    });
                }
                ui.label(
                    RichText::new(
                        "Use Repositories → Discover to fetch live repositories after connecting.",
                    )
                    .size(10.5)
                    .color(tokens.text_faint),
                );
            });
        }
        OnboardingStep::Ai => {
            components::panel(tokens).show(ui, |ui| {
                ui.label(
                    RichText::new("AI is optional")
                        .size(14.0)
                        .strong()
                        .color(tokens.text),
                );
                ui.label(
                    RichText::new(
                        "Configure OpenAI, Anthropic, Gemini, an OpenAI-compatible endpoint or local Ollama in Settings → Providers. Local Ollama can be used under a local-only policy.",
                    )
                    .size(11.0_f32)
                    .color(tokens.text_muted),
                );
                for provider in &state.bootstrap.providers {
                    ui.horizontal(|ui| {
                        components::status_dot(
                            ui,
                            if provider.has_credential {
                                tokens.success
                            } else {
                                tokens.text_faint
                            },
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} · {}",
                                provider.display_name, provider.model
                            ))
                            .size(11.0_f32)
                            .color(tokens.text_muted),
                        );
                    });
                }
            });
        }
        OnboardingStep::Privacy => {
            components::panel(tokens).show(ui, |ui| {
                ui.label(
                    RichText::new("External transfer is denied by default")
                        .size(14.0)
                        .strong()
                        .color(tokens.text),
                );
                ui.label(
                    RichText::new(
                        "Repository content is untrusted evidence. It can never alter tool permissions. Enable external AI only after reviewing which providers receive context.",
                    )
                    .size(11.0_f32)
                    .color(tokens.text_muted),
                );
                components::badge(
                    ui,
                    tokens,
                    if state.bootstrap.permissions.external_ai {
                        "external AI enabled"
                    } else {
                        "local-only"
                    },
                    if state.bootstrap.permissions.external_ai {
                        tokens.warning
                    } else {
                        tokens.success
                    },
                );
            });
        }
        OnboardingStep::Review => {
            components::panel(tokens).show(ui, |ui| {
                ui.label(
                    RichText::new("Private review by default")
                        .size(14.0)
                        .strong()
                        .color(tokens.text),
                );
                ui.checkbox(
                    &mut state.review_policy_draft.auto_review_on_open,
                    "Queue review when a PR opens (still requires provider consent)",
                );
                ui.checkbox(
                    &mut state.review_policy_draft.review_on_new_commits,
                    "Queue review on new commits",
                );
                ui.checkbox(
                    &mut state.review_policy_draft.include_drafts,
                    "Include draft PRs",
                );
                ui.label(
                    RichText::new("Publication and approvals are disabled in this build.")
                        .size(10.5)
                        .color(tokens.warning),
                );
            });
        }
        OnboardingStep::Notifications => {
            components::panel(tokens).show(ui, |ui| {
                ui.label(
                    RichText::new("Reduce noise")
                        .size(14.0)
                        .strong()
                        .color(tokens.text),
                );
                ui.checkbox(
                    &mut state.bootstrap.settings.notifications_enabled,
                    "Enable in-app notifications",
                );
                ui.checkbox(
                    &mut state.bootstrap.settings.native_notifications_enabled,
                    "Enable native OS delivery when the packaged app is permitted",
                );
                ui.label(
                    RichText::new(
                        "Notifications are deduplicated by event key. Native OS delivery is best-effort and requires a packaged, permission-granted app; in-app alerts remain the reliable fallback.",
                    )
                    .size(10.5)
                    .color(tokens.text_muted),
                );
            });
        }
        OnboardingStep::Finish => {
            components::panel(tokens).show(ui, |ui| {
                ui.label(
                    RichText::new("You are ready to investigate")
                        .size(16.0)
                        .strong()
                        .color(tokens.text),
                );
                if ui.button("Finish setup").clicked() {
                    state.onboarding_complete = true;
                    state.bootstrap.settings.onboarding_complete = true;
                    state.screen = crate::nav::Screen::Overview;
                    let settings = state.bootstrap.settings.clone();
                    let service = Arc::clone(service);
                    let tx = tx.clone();
                    runtime.spawn(async move {
                        if let Err(error) = service.save_settings(&settings).await {
                            let _ = tx.send(UiEvent::Error(error.user_message()));
                        }
                        let _ = tx.send(UiEvent::BootstrapRefreshed);
                    });
                }
            });
        }
    }
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                state.onboarding_step != OnboardingStep::Welcome,
                egui::Button::new("← Back"),
            )
            .clicked()
        {
            state.onboarding_step = match state.onboarding_step {
                OnboardingStep::Welcome => OnboardingStep::Welcome,
                OnboardingStep::GitHub => OnboardingStep::Welcome,
                OnboardingStep::Repositories => OnboardingStep::GitHub,
                OnboardingStep::Ai => OnboardingStep::Repositories,
                OnboardingStep::Privacy => OnboardingStep::Ai,
                OnboardingStep::Review => OnboardingStep::Privacy,
                OnboardingStep::Notifications => OnboardingStep::Review,
                OnboardingStep::Finish => OnboardingStep::Notifications,
            };
        }
        if ui
            .add_enabled(
                state.onboarding_step != OnboardingStep::Finish,
                egui::Button::new("Continue →"),
            )
            .clicked()
        {
            state.onboarding_step = match state.onboarding_step {
                OnboardingStep::Welcome => OnboardingStep::GitHub,
                OnboardingStep::GitHub => OnboardingStep::Repositories,
                OnboardingStep::Repositories => OnboardingStep::Ai,
                OnboardingStep::Ai => OnboardingStep::Privacy,
                OnboardingStep::Privacy => OnboardingStep::Review,
                OnboardingStep::Review => OnboardingStep::Notifications,
                OnboardingStep::Notifications => OnboardingStep::Finish,
                OnboardingStep::Finish => OnboardingStep::Finish,
            };
        }
    });
}
