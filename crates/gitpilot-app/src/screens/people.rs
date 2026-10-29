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
    if state.contributors.is_empty() {
        let service = Arc::clone(service);
        let tx = tx.clone();
        runtime.spawn(async move {
            match service.contributors().await {
                Ok(contributors) => {
                    let _ = tx.send(UiEvent::Contributors(contributors));
                }
                Err(error) => {
                    let _ = tx.send(UiEvent::Error(error.user_message()));
                }
            }
        });
    }
    if state.change_impact_signals.is_empty() {
        let service = Arc::clone(service);
        let tx = tx.clone();
        runtime.spawn(async move {
            if let Ok(signals) = service.change_impact_signals().await {
                let _ = tx.send(UiEvent::ChangeImpactSignals(signals));
            }
        });
    }
    components::page_header(
        ui,
        tokens,
        "People & collaboration",
        "Team-level workload and review bottlenecks, not productivity rankings",
        |ui| {
            if ui.button("Privacy settings").clicked() {
                state.settings_tab = crate::state::SettingsTab::Privacy;
                state.screen = crate::nav::Screen::Settings;
            }
        },
    );
    components::inset(tokens).show(ui, |ui| {
        ui.label(
            RichText::new(
                "Visibility is team-level by default. GitPilot does not rank people by commit volume, lines changed, comment count or activity frequency.",
            )
            .size(10.5)
            .color(tokens.text_muted),
        );
    });
    ui.add_space(8.0);
    if state.contributors.is_empty() {
        components::loading(ui, tokens, "Deriving review and assignment workload");
        return;
    }
    let mut sorted = state.contributors.clone();
    sorted.sort_by_key(|c| std::cmp::Reverse(c.pending_reviews + c.review_requests));
    egui::ScrollArea::vertical()
        .id_salt("people")
        .show(ui, |ui| {
            for person in &sorted {
                components::panel(tokens).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        components::status_dot(
                            ui,
                            if person.pending_reviews > 0 {
                                tokens.warning
                            } else {
                                tokens.success
                            },
                        );
                        ui.label(
                            RichText::new(&person.login)
                                .size(14.0)
                                .strong()
                                .color(tokens.text),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} review requests · {} pending",
                                    person.review_requests, person.pending_reviews
                                ))
                                .size(10.5)
                                .color(tokens.text_faint),
                            );
                        });
                    });
                    ui.horizontal_wrapped(|ui| {
                        components::badge(
                            ui,
                            tokens,
                            &format!("{} open PRs", person.open_prs),
                            tokens.accent,
                        );
                        components::badge(
                            ui,
                            tokens,
                            &format!("{} assigned issues", person.assigned_issues),
                            tokens.info,
                        );
                        if state.bootstrap.settings.individual_analytics_enabled {
                            components::badge(
                                ui,
                                tokens,
                                &format!("{} recent commits (context only)", person.commits_30d),
                                tokens.text_muted,
                            );
                        }
                    });
                    if person.pending_reviews >= 2 {
                        ui.label(
                            RichText::new(
                                "This person has multiple pending review requests. Consider redistributing review load.",
                            )
                            .size(10.5)
                            .color(tokens.warning),
                        );
                    }
                });
                ui.add_space(4.0);
            }
        });
    ui.add_space(10.0);
    components::section_header(
        ui,
        tokens,
        "Cross-PR change signals",
        Some("Deterministic overlap; never labelled a conflict without evidence"),
    );
    if state.change_impact_signals.is_empty() {
        ui.label(
            RichText::new(
                "No shared-file, dependency, migration or API-schema signals were found in cached diffs.",
            )
            .size(10.5)
            .color(tokens.text_faint),
        );
    }
    for signal in state.change_impact_signals.iter().take(20) {
        components::inset(tokens).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                components::badge(ui, tokens, &signal.kind, tokens.warning);
                components::badge(
                    ui,
                    tokens,
                    &format!("{:.0}% confidence", signal.confidence * 100.0),
                    tokens.text_muted,
                );
                components::badge(
                    ui,
                    tokens,
                    if signal.is_conflict {
                        "conflict signal"
                    } else {
                        "review prompt"
                    },
                    tokens.info,
                );
                ui.label(RichText::new(&signal.title).size(11.0).color(tokens.text));
            });
            for evidence in signal.evidence.iter().take(5) {
                ui.label(
                    RichText::new(format!("· {evidence}"))
                        .monospace()
                        .size(9.5)
                        .color(tokens.text_faint),
                );
            }
        });
        ui.add_space(4.0);
    }
    components::section_header(
        ui,
        tokens,
        "Review bottlenecks",
        Some("Operational signal; suggestions never auto-assign reviewers"),
    );
    let mut backlog: Vec<_> = state
        .bootstrap
        .dashboard
        .health
        .iter()
        .filter(|h| h.review_backlog > 0)
        .collect();
    backlog.sort_by_key(|h| std::cmp::Reverse(h.review_backlog));
    if backlog.is_empty() {
        ui.label(
            RichText::new("No review backlog identified in the current snapshot.")
                .size(11.0_f32)
                .color(tokens.text_muted),
        );
    }
    for health in backlog {
        components::inset(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&health.name).size(11.5).color(tokens.text));
                ui.label(
                    RichText::new(format!(
                        "{} PRs awaiting human review",
                        health.review_backlog
                    ))
                    .size(10.5)
                    .color(tokens.warning),
                );
            });
        });
    }
}
