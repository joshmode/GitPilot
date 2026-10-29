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
            ui.checkbox(&mut state.ai_stream_enabled, "Stream provider output");
        },
    );
    if state
        .embedding_status
        .starts_with("Embedding index not checked")
    {
        let service = Arc::clone(service);
        let tx = tx.clone();
        runtime.spawn(async move {
            if let Ok((enabled, provider_id, count)) = service.embedding_status().await {
                let _ = tx.send(UiEvent::EmbeddingStatus {
                    enabled,
                    provider_id,
                    count,
                });
            }
        });
    }
    components::panel(tokens).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Knowledge retrieval")
                    .size(12.0)
                    .strong()
                    .color(tokens.text),
            );
            components::badge(ui, tokens, &state.embedding_status, tokens.info);
            let eligible = state
                .bootstrap
                .providers
                .iter()
                .filter(|provider| provider.embedding_model.is_some())
                .cloned()
                .collect::<Vec<_>>();
            if !eligible.is_empty() {
                let selected = state
                    .embedding_provider
                    .clone()
                    .or_else(|| eligible.first().map(|provider| provider.id.clone()))
                    .unwrap_or_default();
                egui::ComboBox::from_id_salt("embedding_provider")
                    .selected_text(
                        eligible
                            .iter()
                            .find(|provider| provider.id == selected)
                            .map(|provider| provider.display_name.as_str())
                            .unwrap_or("Embedding provider"),
                    )
                    .show_ui(ui, |ui| {
                        for provider in &eligible {
                            ui.selectable_value(
                                &mut state.embedding_provider,
                                Some(provider.id.clone()),
                                &provider.display_name,
                            );
                        }
                    });
                ui.checkbox(
                    &mut state.bootstrap.settings.embeddings_enabled,
                    "Use hybrid retrieval",
                );
                ui.add(
                    egui::Slider::new(
                        &mut state.bootstrap.settings.hybrid_vector_weight,
                        0.0..=1.0,
                    )
                    .text("vector weight"),
                );
                if state.bootstrap.settings.embedding_provider_id.is_none()
                    || state.bootstrap.settings.embedding_provider_id.as_deref()
                    != Some(selected.as_str())
                {
                    state.bootstrap.settings.embedding_provider_id = Some(selected.clone());
                }
                if ui
                    .button("Save embedding policy")
                    .on_hover_text("Vector search remains optional; FTS5 always works")
                    .clicked()
                {
                    let settings = state.bootstrap.settings.clone();
                    let service = Arc::clone(service);
                    let tx = tx.clone();
                    runtime.spawn(async move {
                        match service.save_settings(&settings).await {
                            Ok(()) => {
                                let _ = tx.send(UiEvent::Toast(
                                    "Embedding policy saved. Index records to enable vector ranking."
                                        .into(),
                                ));
                            }
                            Err(error) => {
                                let _ = tx.send(UiEvent::Error(error.user_message()));
                            }
                        }
                    });
                }
                if ui.button("Index knowledge embeddings").clicked() {
                    let provider_id = selected;
                    let repository_id = state.repository_scope.clone();
                    let service = Arc::clone(service);
                    let tx = tx.clone();
                    runtime.spawn(async move {
                        match service
                            .index_knowledge_embeddings(
                                repository_id.as_deref(),
                                &provider_id,
                                256,
                            )
                            .await
                        {
                            Ok(count) => {
                                let _ = tx.send(UiEvent::Toast(format!(
                                    "Indexed {count} knowledge embedding(s)."
                                )));
                                if let Ok((enabled, provider_id, count)) =
                                    service.embedding_status().await
                                {
                                    let _ = tx.send(UiEvent::EmbeddingStatus {
                                        enabled,
                                        provider_id,
                                        count,
                                    });
                                }
                            }
                            Err(error) => {
                                let _ = tx.send(UiEvent::Error(error.user_message()));
                            }
                        }
                    });
                }
            } else {
                ui.label(
                    RichText::new(
                        "No provider has an embedding model configured. Add or edit a provider to enable optional hybrid retrieval.",
                    )
                    .size(10.0)
                    .color(tokens.text_faint),
                );
            }
        });
        ui.label(
            RichText::new(
                "FTS5 remains the default and works without external services. Embeddings are opt-in, stored locally, invalidated when source content changes, and subject to the same external-AI/local-only policy.",
            )
            .size(10.0)
            .color(tokens.text_muted),
        );
    });
    ui.add_space(8.0);
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
                let stream_enabled = state.ai_stream_enabled;
                let service = Arc::clone(service);
                let tx = tx.clone();
                let cancel = CancellationToken::new();
                state.ai_stream_text.clear();
                runtime.spawn(async move {
                    let stream_enabled = use_ai && stream_enabled;
                    if stream_enabled {
                        let (stream_tx, mut stream_rx) =
                            tokio::sync::mpsc::channel::<
                                gitpilot_core::Result<gitpilot_core::ai::StreamChunk>,
                            >(32);
                        let stream_ui = tx.clone();
                        let stream_task = tokio::spawn(async move {
                            while let Some(chunk) = stream_rx.recv().await {
                                match chunk {
                                    Ok(chunk) => {
                                        let _ = stream_ui
                                            .send(UiEvent::AiStreamDelta(chunk.delta));
                                    }
                                    Err(error) => {
                                        let _ = stream_ui
                                            .send(UiEvent::Error(error.user_message()));
                                        break;
                                    }
                                }
                            }
                        });
                        match service
                            .ask_workspace_stream(
                                &query,
                                repository_id.as_deref(),
                                &cancel,
                                stream_tx,
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
                        let _ = stream_task.await;
                    } else {
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
            if !state.ai_stream_text.is_empty() {
                ui.collapsing("Streaming trace", |ui| {
                    ui.label(
                        RichText::new(&state.ai_stream_text)
                            .size(10.0)
                            .color(tokens.text_faint),
                    );
                });
            }
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
