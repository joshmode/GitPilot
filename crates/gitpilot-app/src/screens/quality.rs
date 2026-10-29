use crate::components;
use crate::state::UiState;
use crate::theme::Tokens;
use eframe::egui::{self, Layout, RichText};
use gitpilot_core::{AppService, SecurityState, Severity};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

pub fn show(
    ui: &mut egui::Ui,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<crate::state::UiEvent>,
) {
    if state.security_alerts.is_empty() {
        let service = Arc::clone(service);
        let tx = tx.clone();
        let repositories = state.bootstrap.repositories.clone();
        runtime.spawn(async move {
            let mut rows = Vec::new();
            for repo in repositories {
                if let Ok(mut alerts) = service.store.security_alerts(&repo.id).await {
                    rows.append(&mut alerts);
                }
            }
            let _ = tx.send(crate::state::UiEvent::SecurityAlerts(rows));
        });
    }
    components::page_header(
        ui,
        tokens,
        "Quality & security",
        "Evidence states, CI signal, dependency risk and review debt",
        |ui| {
            ui.label(
                RichText::new("Absence of alerts is not proof of security")
                    .size(10.0)
                    .color(tokens.warning),
            );
        },
    );
    ui.columns(2, |columns| {
        let left = &mut columns[0];
        components::section_header(left, tokens, "Security visibility", None);
        for health in &state.bootstrap.dashboard.health {
            components::panel(tokens).show(left, |ui| {
                ui.horizontal(|ui| {
                    let (color, label) = match health.security_state {
                        SecurityState::NotChecked => (tokens.text_faint, "NOT CHECKED"),
                        SecurityState::Unavailable => (tokens.warning, "UNAVAILABLE"),
                        SecurityState::NoFindingsReturned => {
                            (tokens.success, "NO ALERTS RETURNED")
                        }
                        SecurityState::FindingsPresent => (tokens.danger, "FINDINGS PRESENT"),
                    };
                    components::badge(ui, tokens, label, color);
                    ui.label(
                        RichText::new(&health.name)
                            .size(12.0)
                            .color(tokens.text),
                    );
                });
                ui.label(
                    RichText::new(
                        "Distinguish repository permission errors from a checked, empty alert result.",
                    )
                    .size(10.0)
                    .color(tokens.text_faint),
                );
            });
            left.add_space(4.0);
        }
        let right = &mut columns[1];
        components::section_header(right, tokens, "CI & review debt", None);
        for health in &state.bootstrap.dashboard.health {
            components::panel(tokens).show(right, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&health.name)
                            .size(12.0)
                            .strong()
                            .color(tokens.text),
                    );
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if health.failing_checks > 0 {
                            components::badge(
                                ui,
                                tokens,
                                &format!("{} CI failing", health.failing_checks),
                                tokens.danger,
                            );
                        }
                        if health.review_backlog > 0 {
                            components::badge(
                                ui,
                                tokens,
                                &format!("{} review debt", health.review_backlog),
                                tokens.warning,
                            );
                        }
                    });
                });
            });
            right.add_space(4.0);
        }
    });
    ui.add_space(10.0);
    components::section_header(
        ui,
        tokens,
        "Open security alerts",
        Some("Code scanning, Dependabot and secret scanning"),
    );
    if state.security_alerts.is_empty() {
        ui.label(
            RichText::new(
                "No cached alerts. Treat unavailable checks as unknown, not as proof of safety.",
            )
            .size(10.5)
            .color(tokens.text_faint),
        );
    }
    for alert in state.security_alerts.iter().take(30) {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                components::severity_badge(ui, tokens, alert.severity);
                components::badge(ui, tokens, &alert.kind, tokens.info);
                ui.label(
                    RichText::new(&alert.title)
                        .size(12.0)
                        .strong()
                        .color(tokens.text),
                );
            });
            ui.label(
                RichText::new(&alert.description)
                    .size(10.5)
                    .color(tokens.text_muted),
            );
            if let Some(url) = &alert.url
                && ui.small_button("Open on GitHub").clicked()
            {
                let _ = webbrowser::open(url);
            }
        });
        ui.add_space(4.0);
    }
    components::section_header(
        ui,
        tokens,
        "High-confidence findings",
        Some("Persisted private review output with evidence"),
    );
    let mut findings: Vec<_> = state
        .pr_detail
        .as_ref()
        .map(|d| d.findings.clone())
        .unwrap_or_default();
    findings.sort_by(|a, b| {
        severity_rank(b.severity)
            .cmp(&severity_rank(a.severity))
            .then_with(|| {
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    if findings.is_empty() {
        components::empty_state(
            ui,
            tokens,
            "No review findings loaded",
            "Open a PR and run a private review, then return here to inspect quality signals.",
            None,
        );
    }
    for finding in findings.iter().take(20) {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                components::severity_badge(ui, tokens, finding.severity);
                components::badge(
                    ui,
                    tokens,
                    &format!("{}% confidence", (finding.confidence * 100.0) as i64),
                    tokens.text_muted,
                );
                ui.label(
                    RichText::new(&finding.title)
                        .size(12.0)
                        .strong()
                        .color(tokens.text),
                );
            });
            if let Some(path) = &finding.path {
                ui.label(
                    RichText::new(format!(
                        "{}:{}",
                        path,
                        finding.diff_line.map(|l| l.to_string()).unwrap_or_default()
                    ))
                    .monospace()
                    .size(10.0)
                    .color(tokens.info),
                );
            }
        });
        ui.add_space(4.0);
    }
    let _ = (service, runtime, tx);
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Info => 0,
        Severity::Low => 1,
        Severity::Medium => 2,
        Severity::High => 3,
        Severity::Critical => 4,
    }
}
