use crate::components;
use crate::state::{UiEvent, UiState};
use crate::theme::Tokens;
use eframe::egui::{self, Align, Layout, RichText};
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
    let dashboard = &state.bootstrap.dashboard;
    let mut refresh = false;
    let mut generate_brief = false;
    components::page_header(
        ui,
        tokens,
        "Engineering command centre",
        if dashboard.demo {
            "Synthetic demo workspace · connect GitHub for live project data"
        } else {
            "Live GitHub metadata · AI is optional"
        },
        |ui| {
            if ui.button("Refresh data").clicked() {
                refresh = true;
            }
            if ui.button("Generate brief").clicked() {
                generate_brief = true;
            }
        },
    );
    let metrics = [
        (
            "Active projects",
            dashboard.active_projects.to_string(),
            "grouped workspaces",
            tokens.info,
        ),
        (
            "Open PRs",
            dashboard.open_prs.to_string(),
            "across repositories",
            tokens.accent,
        ),
        (
            "Awaiting review",
            dashboard.awaiting_review.to_string(),
            "human decision missing",
            tokens.warning,
        ),
        (
            "Failing checks",
            dashboard.failing_checks.to_string(),
            "cached CI state",
            tokens.danger,
        ),
        (
            "Stale work",
            dashboard.stale_work.to_string(),
            "14+ days without update",
            tokens.text_muted,
        ),
    ];
    ui.horizontal_wrapped(|ui| {
        for (label, value, hint, color) in metrics {
            components::metric_card(ui, tokens, label, &value, hint, color);
        }
    });
    ui.add_space(12.0);
    ui.columns(2, |columns| {
        let left = &mut columns[0];
        components::section_header(
            left,
            tokens,
            "Attention queue",
            Some("Deterministic signals from current cached metadata"),
        );
        if dashboard.alerts.is_empty() {
            components::empty_state(
                left,
                tokens,
                "No urgent signals",
                "No CI failures, merge conflicts or stale PRs were found in this snapshot.",
                None,
            );
        } else {
            for alert in dashboard.alerts.iter().take(6) {
                components::panel(tokens).show(left, |ui| {
                    ui.horizontal(|ui| {
                        components::severity_badge(ui, tokens, alert.severity);
                        ui.label(
                            RichText::new(&alert.title)
                                .size(12.5)
                                .strong()
                                .color(tokens.text),
                        );
                    });
                    ui.label(
                        RichText::new(&alert.detail)
                            .size(11.0_f32)
                            .color(tokens.text_muted),
                    );
                });
                left.add_space(5.0);
            }
        }
        let right = &mut columns[1];
        components::section_header(
            right,
            tokens,
            "Repository health",
            Some("Derived operational indicators, not a productivity score"),
        );
        for health in dashboard.health.iter().take(4) {
            components::panel(tokens).show(right, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&health.name)
                            .size(12.5)
                            .strong()
                            .color(tokens.text),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(score) = health.health_score {
                            ui.label(RichText::new(format!("health {score}")).size(10.0).color(
                                if score >= 70 {
                                    tokens.success
                                } else {
                                    tokens.warning
                                },
                            ));
                        } else {
                            components::badge(ui, tokens, "no signal", tokens.text_faint);
                        }
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    components::badge(
                        ui,
                        tokens,
                        &format!("{} PRs", health.open_prs),
                        tokens.accent,
                    );
                    components::badge(
                        ui,
                        tokens,
                        &format!("{} review", health.review_backlog),
                        tokens.warning,
                    );
                    components::badge(
                        ui,
                        tokens,
                        &format!("{} CI fail", health.failing_checks),
                        tokens.danger,
                    );
                    components::badge(ui, tokens, health.security_state.label(), tokens.text_muted);
                });
            });
            right.add_space(5.0);
        }
    });
    ui.add_space(12.0);
    components::inset(tokens).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("FILTERS")
                    .size(9.0)
                    .strong()
                    .color(tokens.text_faint),
            );
            if state.repository_scope.is_some() {
                components::badge(
                    ui,
                    tokens,
                    state
                        .repository_scope
                        .as_ref()
                        .and_then(|id| state.bootstrap.repositories.iter().find(|r| &r.id == id))
                        .map(|r| r.full_name.as_str())
                        .unwrap_or("repository"),
                    tokens.accent,
                );
            } else {
                components::badge(ui, tokens, "all repositories", tokens.text_muted);
            }
            for (label, values) in [
                (
                    "Organization",
                    state
                        .bootstrap
                        .repositories
                        .iter()
                        .map(|r| r.owner.clone())
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>(),
                ),
                (
                    "Workspace",
                    state
                        .bootstrap
                        .repositories
                        .iter()
                        .filter_map(|r| r.workspace.clone())
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>(),
                ),
            ] {
                if !values.is_empty() {
                    ui.label(
                        RichText::new(format!("{label}: {}", values.join(", ")))
                            .size(9.5_f32)
                            .color(tokens.text_faint),
                    );
                }
            }
            ui.label(
                RichText::new(format!(
                    "Snapshot: {}",
                    dashboard
                        .last_synced_at
                        .map(components::short_time)
                        .unwrap_or_else(|| "not synchronized".into())
                ))
                .size(9.5_f32)
                .color(tokens.text_faint),
            );
        });
        if !dashboard.missing.is_empty() {
            ui.label(
                RichText::new(dashboard.missing.join(" · "))
                    .size(9.5_f32)
                    .color(tokens.warning),
            );
        }
    });
    ui.add_space(8.0);
    components::section_header(
        ui,
        tokens,
        "AI project brief",
        Some("Facts, derived metrics and inferences stay separate"),
    );
    if let Some(brief) = &state.brief {
        components::panel(tokens).show(ui, |ui| {
            if !brief.missing.is_empty() {
                for missing in &brief.missing {
                    ui.label(
                        RichText::new(format!("△ {}", missing))
                            .size(10.5_f32)
                            .color(tokens.warning),
                    );
                }
            }
            for (title, items) in [
                ("Observed", &brief.facts),
                ("Derived", &brief.metrics),
                ("Inference", &brief.inferences),
                ("Recommendation", &brief.next_steps),
            ] {
                ui.label(
                    RichText::new(title.to_ascii_uppercase())
                        .size(9.0_f32)
                        .strong()
                        .color(tokens.text_faint),
                );
                for item in items.iter().take(4) {
                    ui.horizontal_wrapped(|ui| {
                        components::evidence_chip(ui, tokens, item.kind, None);
                        ui.label(
                            RichText::new(&item.text)
                                .size(11.0_f32)
                                .color(tokens.text_muted),
                        );
                    });
                }
                ui.add_space(4.0_f32);
            }
        });
    } else {
        let mut generate = false;
        components::empty_state(
            ui,
            tokens,
            "Brief not generated",
            "Generate an evidence-labelled snapshot. This runs locally unless you explicitly enable external AI.",
            Some(("Generate brief", &mut generate)),
        );
        generate_brief |= generate;
    }
    ui.add_space(8.0);
    components::section_header(
        ui,
        tokens,
        "Recent activity",
        Some("Newest cached changes across connected repositories"),
    );
    for activity in dashboard.activity.iter().take(8) {
        ui.horizontal(|ui| {
            components::status_dot(ui, tokens.accent);
            ui.label(RichText::new(&activity.title).size(11.5).color(tokens.text));
            ui.label(
                RichText::new(format!("{} · {}", activity.repository, activity.actor))
                    .size(10.0)
                    .color(tokens.text_faint),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(components::short_time(activity.occurred_at))
                        .size(10.0)
                        .color(tokens.text_faint),
                );
            });
        });
    }
    if refresh {
        let service = Arc::clone(service);
        let tx = tx.clone();
        runtime.spawn(async move {
            match service.refresh_dashboard().await {
                Ok(_) => {
                    let _ = tx.send(UiEvent::BootstrapRefreshed);
                }
                Err(error) => {
                    let _ = tx.send(UiEvent::Error(error.user_message()));
                }
            }
        });
    }
    if generate_brief {
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
}
