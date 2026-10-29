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
    if state.automations.is_empty() {
        let service = Arc::clone(service);
        let tx = tx.clone();
        runtime.spawn(async move {
            let (automations, runs) =
                tokio::join!(service.automations(), service.automation_runs());
            if let Ok(automations) = automations {
                let _ = tx.send(UiEvent::Automations(automations));
            }
            if let Ok(runs) = runs {
                let _ = tx.send(UiEvent::AutomationRuns(runs));
            }
        });
    }
    components::page_header(
        ui,
        tokens,
        "Automations",
        "Event-driven workflows with dry-run, idempotency and explicit permission gates",
        |ui| {
            if ui.button("Create starter automation").clicked() {
                let service = Arc::clone(service);
                let tx = tx.clone();
                runtime.spawn(async move {
                    match service.create_default_automation().await {
                        Ok(_) => {
                            let _ = tx.send(UiEvent::BootstrapRefreshed);
                        }
                        Err(error) => {
                            let _ = tx.send(UiEvent::Error(error.user_message()));
                        }
                    }
                });
            }
        },
    );
    components::inset(tokens).show(ui, |ui| {
        ui.label(
            RichText::new(
                "Triggers: PR opened/synchronized/ready, review submitted, CI failed/passed, issue assigned, label added, release published, schedule, manual. Event ingestion uses polling fallback; a webhook receiver is documented but not bundled.",
            )
            .size(10.5)
            .color(tokens.text_muted),
        );
    });
    ui.add_space(8.0);
    if state.automations.is_empty() {
        components::empty_state(
            ui,
            tokens,
            "No automations",
            "Create a starter automation to see the typed condition/action model. Writes stay blocked unless separately authorized.",
            None,
        );
    }
    for automation in &state.automations {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                components::status_dot(
                    ui,
                    if automation.enabled {
                        tokens.success
                    } else {
                        tokens.text_faint
                    },
                );
                ui.label(
                    RichText::new(&automation.name)
                        .size(13.0)
                        .strong()
                        .color(tokens.text),
                );
                components::badge(
                    ui,
                    tokens,
                    if automation.dry_run {
                        "DRY RUN"
                    } else {
                        "ENABLED"
                    },
                    if automation.dry_run {
                        tokens.info
                    } else {
                        tokens.warning
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("trigger: {}", automation.trigger))
                            .size(10.0)
                            .color(tokens.text_faint),
                    );
                });
            });
            ui.horizontal_wrapped(|ui| {
                for action in &automation.actions {
                    components::badge(ui, tokens, action, tokens.text_muted);
                }
                if automation.requires_approval {
                    components::badge(ui, tokens, "approval required", tokens.warning);
                }
                ui.label(
                    RichText::new(format!(
                        "budget ${:.2} · timeout {}s",
                        automation.budget_usd, automation.timeout_seconds
                    ))
                    .size(10.0)
                    .color(tokens.text_faint),
                );
            });
        });
        ui.add_space(5.0);
    }
    components::section_header(
        ui,
        tokens,
        "Run history",
        Some("Unique event keys prevent duplicate executions"),
    );
    if state.automation_runs.is_empty() {
        ui.label(
            RichText::new("No automation runs have been recorded.")
                .size(11.0_f32)
                .color(tokens.text_muted),
        );
    }
    for run in state.automation_runs.iter().take(20) {
        ui.horizontal(|ui| {
            components::badge(ui, tokens, &run.status, tokens.info);
            ui.label(
                RichText::new(&run.summary)
                    .size(10.5)
                    .color(tokens.text_muted),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(components::short_time(run.started_at))
                        .size(10.0)
                        .color(tokens.text_faint),
                );
            });
        });
    }
}
