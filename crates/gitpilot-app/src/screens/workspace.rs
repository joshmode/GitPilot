use crate::components;
use crate::state::{UiEvent, UiState};
use crate::theme::Tokens;
use eframe::egui::{self, Layout, RichText};
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
    components::page_header(
        ui,
        tokens,
        "AI investigation workspace",
        "Evidence-backed repository questions with bounded, project-scoped context",
        |ui| {
            ui.checkbox(&mut state.ai_use_provider, "Use configured provider");
        },
    );
    components::panel(tokens).show(ui, |ui| {
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::multiline(&mut state.ai_question)
                    .hint_text(
                        "Why did behavior change? Which PRs affect authentication? What blocks the milestone?",
                    )
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            if response.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift)
            {
                response.request_focus();
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Investigate").clicked() && !state.ai_question.trim().is_empty() {
                let query = state.ai_question.clone();
                let repository_id = state.repository_scope.clone();
                let use_ai = state.ai_use_provider;
                let service = Arc::clone(service);
                let tx = tx.clone();
                let cancel = CancellationToken::new();
                runtime.spawn(async move {
                    match service
                        .ask_workspace(
                            &query,
                            repository_id.as_deref(),
                            use_ai,
                            &cancel,
                        )
                        .await
                    {
                        Ok(answer) => {
                            let _ = tx.send(UiEvent::Explanation(answer));
                        }
                        Err(error) => {
                            let _ = tx.send(UiEvent::Error(error.user_message()));
                        }
                    }
                });
            }
            if ui.button("Clear").clicked() {
                state.ai_question.clear();
                state.ai_answer = None;
                state.explanation = None;
            }
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                components::badge(
                    ui,
                    tokens,
                    if state.ai_use_provider {
                        "provider synthesis"
                    } else {
                        "local retrieval"
                    },
                    if state.ai_use_provider {
                        tokens.warning
                    } else {
                        tokens.success
                    },
                );
            });
        });
    });
    ui.add_space(10.0);
    if let Some(answer) = &state.explanation {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                components::evidence_chip(
                    ui,
                    tokens,
                    if state.ai_use_provider {
                        gitpilot_core::EvidenceKind::Inference
                    } else {
                        gitpilot_core::EvidenceKind::Observed
                    },
                    None,
                );
                ui.label(
                    RichText::new(format!("{} · {}", answer.provider, answer.model))
                        .size(10.0)
                        .color(tokens.text_faint),
                );
            });
            ui.label(
                RichText::new(&answer.executive)
                    .size(12.0)
                    .color(tokens.text_muted),
            );
            components::divider(ui, tokens);
            ui.label(
                RichText::new("EVIDENCE")
                    .size(9.0)
                    .strong()
                    .color(tokens.text_faint),
            );
            for citation in &answer.citations {
                ui.horizontal(|ui| {
                    components::evidence_chip(ui, tokens, citation.kind, None);
                    ui.label(RichText::new(&citation.label).size(10.5).color(tokens.text));
                    if let Some(url) = &citation.url
                        && ui.small_button("Open").clicked()
                    {
                        let _ = webbrowser::open(url);
                    }
                });
            }
        });
    } else {
        ui.columns(2, |columns| {
            let left = &mut columns[0];
            components::section_header(
                left,
                tokens,
                "Question starting points",
                Some("Scoped to the selected repository when set"),
            );
            for prompt in [
                "What changed since the last release?",
                "Which PRs affect authentication?",
                "What is blocking this milestone?",
                "Which modules depend on this interface?",
                "What should I review next?",
            ] {
                if left
                    .add(egui::Button::new(RichText::new(prompt).size(11.0_f32)))
                    .clicked()
                {
                    state.ai_question = prompt.into();
                }
            }
            let right = &mut columns[1];
            components::section_header(
                right,
                tokens,
                "Knowledge index",
                Some("Provenance is retained for every record"),
            );
            components::empty_state(
                right,
                tokens,
                "Local-first retrieval",
                "FTS5 search works without embeddings. Index local documentation from the Local workspace, or synchronize PR/issue metadata from GitHub.",
                None,
            );
        });
    }
}
