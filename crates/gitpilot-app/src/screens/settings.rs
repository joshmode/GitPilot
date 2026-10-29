use crate::components;
use crate::state::{SettingsTab, UiEvent, UiState};
use crate::theme::Tokens;
use eframe::egui::{self, Align, Layout, RichText};
use gitpilot_core::{AppService, ThemeMode};
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
    components::page_header(
        ui,
        tokens,
        "Settings",
        "Active policies, providers, privacy and data controls",
        |ui| {
            ui.label(
                RichText::new("Changes are applied immediately where safe")
                    .size(10.0)
                    .color(tokens.text_faint),
            );
        },
    );
    let tabs = [
        (SettingsTab::Appearance, "Appearance"),
        (SettingsTab::GitHub, "GitHub"),
        (SettingsTab::Providers, "Providers"),
        (SettingsTab::Routing, "Routing"),
        (SettingsTab::Review, "Review"),
        (SettingsTab::Privacy, "Privacy & permissions"),
        (SettingsTab::Notifications, "Notifications"),
        (SettingsTab::Sandbox, "Sandbox"),
        (SettingsTab::Data, "Data"),
    ];
    ui.horizontal_wrapped(|ui| {
        for (tab, label) in tabs {
            if ui
                .selectable_label(state.settings_tab == tab, label)
                .clicked()
            {
                state.settings_tab = tab;
            }
        }
    });
    components::divider(ui, tokens);
    match state.settings_tab {
        SettingsTab::Appearance => appearance(ui, state, tokens, service, runtime, tx),
        SettingsTab::GitHub => github(ui, state, tokens),
        SettingsTab::Providers => providers(ui, state, tokens),
        SettingsTab::Routing => routing(ui, state, tokens, service, runtime, tx),
        SettingsTab::Review => review(ui, state, tokens, service, runtime, tx),
        SettingsTab::Privacy => privacy(ui, state, tokens, service, runtime, tx),
        SettingsTab::Notifications => notifications(ui, state, tokens, service, runtime, tx),
        SettingsTab::Sandbox => sandbox(ui, state, tokens, service, runtime, tx),
        SettingsTab::Data => data(ui, state, tokens, service, runtime, tx),
    }
}

fn appearance(
    ui: &mut egui::Ui,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    components::panel(tokens).show(ui, |ui| {
        ui.label(
            RichText::new("Theme")
                .size(13.0)
                .strong()
                .color(tokens.text),
        );
        ui.horizontal(|ui| {
            ui.selectable_value(&mut state.bootstrap.settings.theme, ThemeMode::Dark, "Dark");
            ui.selectable_value(
                &mut state.bootstrap.settings.theme,
                ThemeMode::Light,
                "Light",
            );
            ui.selectable_value(
                &mut state.bootstrap.settings.theme,
                ThemeMode::System,
                "System (dark fallback)",
            );
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new("Density")
                .size(11.0_f32)
                .color(tokens.text_faint),
        );
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut state.bootstrap.settings.density,
                "comfortable".into(),
                "Comfortable",
            );
            ui.selectable_value(
                &mut state.bootstrap.settings.density,
                "compact".into(),
                "Compact",
            );
        });
        ui.label(
            RichText::new("Font scale")
                .size(11.0_f32)
                .color(tokens.text_faint),
        );
        ui.add(
            egui::Slider::new(&mut state.bootstrap.settings.font_scale, 0.8..=1.4).step_by(0.05),
        );
        if ui.button("Save appearance").clicked() {
            save_settings(state, service, runtime, tx);
        }
    });
    components::panel(tokens).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Keyboard reference")
                    .size(12.0)
                    .strong()
                    .color(tokens.text),
            );
            if ui.small_button("Open").clicked() {
                state.show_shortcuts = true;
            }
        });
    });
}

fn github(ui: &mut egui::Ui, state: &mut UiState, tokens: &Tokens) {
    components::panel(tokens).show(ui, |ui| {
        ui.label(
            RichText::new("Connected accounts")
                .size(13.0)
                .strong()
                .color(tokens.text),
        );
        if state.bootstrap.accounts.is_empty() {
            ui.label(
                RichText::new("No GitHub account connected. Demo data remains visible.")
                    .size(11.0_f32)
                    .color(tokens.text_muted),
            );
        }
        for account in &state.bootstrap.accounts {
            ui.horizontal(|ui| {
                components::status_dot(ui, tokens.success);
                ui.label(
                    RichText::new(format!("{} · {}", account.login, account.host))
                        .size(11.5)
                        .color(tokens.text),
                );
                if let Some(at) = account.last_validated_at {
                    ui.label(
                        RichText::new(format!("verified {}", components::short_time(at)))
                            .size(10.0)
                            .color(tokens.text_faint),
                    );
                }
            });
        }
        if ui.button("Connect account").clicked() {
            state.account_menu = true;
        }
        ui.label(
            RichText::new(
                "Required scopes are documented in SETUP.md. Tokens live in the OS keychain; the app never writes them to SQLite or logs.",
            )
            .size(10.5)
            .color(tokens.text_faint),
        );
    });
    components::panel(tokens).show(ui, |ui| {
        ui.label(
            RichText::new("Enterprise endpoint")
                .size(13.0)
                .strong()
                .color(tokens.text),
        );
        ui.add(
            egui::TextEdit::singleline(&mut state.github_host)
                .desired_width(f32::INFINITY)
                .hint_text("github.example.com"),
        );
        ui.label(
            RichText::new(
                "GitHub Enterprise Server uses https://HOST/api/v3 and GitHub.com uses https://api.github.com. Plain HTTP is rejected.",
            )
            .size(10.0)
            .color(tokens.text_faint),
        );
    });
}

fn providers(ui: &mut egui::Ui, state: &mut UiState, tokens: &Tokens) {
    for provider in &state.bootstrap.providers {
        let mut selected = false;
        components::panel(tokens).show(ui, |ui| {
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
                    RichText::new(&provider.display_name)
                        .size(13.0)
                        .strong()
                        .color(tokens.text),
                );
                components::badge(ui, tokens, &provider.kind, tokens.info);
                if provider.local_only {
                    components::badge(ui, tokens, "local only", tokens.success);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("Configure").clicked() {
                        selected = true;
                    }
                });
            });
            ui.label(
                RichText::new(format!(
                    "{} · {} · context {}",
                    provider.model,
                    if provider.has_credential {
                        "credential ready"
                    } else {
                        "no credential"
                    },
                    provider.context_limit
                ))
                .size(10.5)
                .color(tokens.text_muted),
            );
        });
        if selected {
            state.selected_provider = Some(provider.id.clone());
            state.palette_hint = "provider_setup".into();
        }
    }
    components::inset(tokens).show(ui, |ui| {
        ui.label(
            RichText::new(
                "Providers use separate documented protocols: OpenAI Responses, Anthropic Messages, Gemini generateContent, OpenAI-compatible chat completions, and Ollama chat. API keys are stored with the OS credential adapter.",
            )
            .size(10.5)
            .color(tokens.text_muted),
        );
    });
}

fn routing(
    ui: &mut egui::Ui,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    let usage_service = Arc::clone(service);
    let usage_tx = tx.clone();
    runtime.spawn(async move {
        if let Ok(usage) = usage_service.store.usage_summary().await {
            let _ = usage_tx.send(UiEvent::Usage(usage));
        }
    });
    components::panel(tokens).show(ui, |ui| {
        ui.label(
            RichText::new("Task routing")
                .size(13.0)
                .strong()
                .color(tokens.text),
        );
        ui.label(
            RichText::new(
                "Default routing uses the deterministic local mock for review and explanation until you configure a real provider. Fallbacks are disclosed in usage records; a weaker model is never selected silently.",
            )
            .size(10.5)
            .color(tokens.text_muted),
        );
        for provider in &state.bootstrap.providers {
            components::kv(
                ui,
                tokens,
                &provider.display_name,
                if provider.enabled { "enabled" } else { "disabled" },
            );
        }
    });
    let mut save_routing = None;
    for policy in &mut state.routing_drafts {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&policy.task)
                        .monospace()
                        .size(11.5)
                        .color(tokens.text),
                );
                egui::ComboBox::from_id_salt(format!("routing_{}", policy.task))
                    .selected_text(
                        state
                            .bootstrap
                            .providers
                            .iter()
                            .find(|p| p.id == policy.primary_provider_id)
                            .map(|p| p.display_name.as_str())
                            .unwrap_or("missing provider"),
                    )
                    .show_ui(ui, |ui| {
                        for provider in &state.bootstrap.providers {
                            if ui
                                .selectable_label(
                                    policy.primary_provider_id == provider.id,
                                    &provider.display_name,
                                )
                                .clicked()
                            {
                                policy.primary_provider_id = provider.id.clone();
                            }
                        }
                    });
                ui.label("budget $");
                ui.add(
                    egui::DragValue::new(&mut policy.max_cost_usd)
                        .range(0.0..=20.0)
                        .speed(0.01),
                );
                if ui.small_button("Save").clicked() {
                    save_routing = Some(policy.clone());
                }
            });
        });
    }
    if let Some(policy) = save_routing {
        let service = Arc::clone(service);
        let tx = tx.clone();
        runtime.spawn(async move {
            match service.save_routing(&policy).await {
                Ok(()) => {
                    let _ = tx.send(UiEvent::Toast("Routing policy saved.".into()));
                }
                Err(error) => {
                    let _ = tx.send(UiEvent::Error(error.user_message()));
                }
            }
        });
    }
    components::panel(tokens).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Usage")
                    .size(13.0)
                    .strong()
                    .color(tokens.text),
            );
            if ui.small_button("Refresh").clicked() {
                let service = Arc::clone(service);
                let tx = tx.clone();
                runtime.spawn(async move {
                    if let Ok(usage) = service.store.usage_summary().await {
                        let _ = tx.send(UiEvent::Usage(usage));
                    }
                });
            }
        });
        components::kv(
            ui,
            tokens,
            "Requests",
            &state.bootstrap.usage.requests.to_string(),
        );
        components::kv(
            ui,
            tokens,
            "Tokens",
            &state.bootstrap.usage.tokens.to_string(),
        );
        components::kv(
            ui,
            tokens,
            "Estimated cost (labeled estimate)",
            &format!("${:.4}", state.bootstrap.usage.estimated_cost_usd),
        );
    });
}

fn review(
    ui: &mut egui::Ui,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    let policy = &mut state.review_policy_draft;
    components::panel(tokens).show(ui, |ui| {
        ui.label(
            RichText::new("Review pipeline")
                .size(13.0)
                .strong()
                .color(tokens.text),
        );
        ui.checkbox(&mut policy.enabled, "Enable review runs");
        ui.checkbox(&mut policy.auto_review_on_open, "On PR opened");
        ui.checkbox(&mut policy.review_on_new_commits, "On new commits");
        ui.checkbox(&mut policy.include_drafts, "Include drafts");
        ui.horizontal(|ui| {
            ui.label("Depth");
            egui::ComboBox::from_id_salt("review_depth_settings")
                .selected_text(format!("{:?}", policy.depth))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut policy.depth,
                        gitpilot_core::ReviewDepth::Fast,
                        "Fast",
                    );
                    ui.selectable_value(
                        &mut policy.depth,
                        gitpilot_core::ReviewDepth::Balanced,
                        "Balanced",
                    );
                    ui.selectable_value(
                        &mut policy.depth,
                        gitpilot_core::ReviewDepth::Deep,
                        "Deep",
                    );
                });
        });
        ui.horizontal(|ui| {
            ui.label("Budget");
            ui.add(
                egui::DragValue::new(&mut policy.budget_usd)
                    .range(0.0..=10.0)
                    .speed(0.05),
            );
            ui.label("Concurrency");
            ui.add(egui::DragValue::new(&mut policy.concurrency).range(1..=8));
        });
        ui.label(
            RichText::new(
                "AI findings remain private. A guarded human review submission exists at the service boundary but is not exposed here.",
            )
            .size(10.5)
            .color(tokens.warning),
        );
        if ui.button("Save review policy").clicked() {
            let policy = policy.clone();
            let service = Arc::clone(service);
            let tx = tx.clone();
            runtime.spawn(async move {
                match service.save_review_policy(&policy).await {
                    Ok(()) => {
                        let _ = tx.send(UiEvent::BootstrapRefreshed);
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
    });
}

fn privacy(
    ui: &mut egui::Ui,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    let permissions = &mut state.bootstrap.permissions;
    let mut save = false;
    components::panel(tokens).show(ui, |ui| {
        ui.label(
            RichText::new("Permission domains")
                .size(13.0)
                .strong()
                .color(tokens.text),
        );
        ui.label(
            RichText::new(
                "These are enforced at the service boundary, not only hidden in the UI. Writes and execution are denied by default.",
            )
            .size(10.5)
            .color(tokens.text_muted),
        );
        ui.add_space(6.0);
        ui.checkbox(&mut permissions.read_github, "Read GitHub data");
        ui.checkbox(
            &mut permissions.index_repository,
            "Index repository content locally",
        );
        ui.checkbox(
            &mut permissions.external_ai,
            "Send selected context to external AI providers",
        );
        ui.checkbox(
            &mut permissions.local_execution,
            "Execute allow-listed local tools",
        );
        ui.checkbox(&mut permissions.modify_local_files, "Modify local files");
        ui.checkbox(&mut permissions.modify_github, "Modify GitHub resources");
        ui.checkbox(&mut permissions.publish_reviews, "Publish review comments");
        ui.checkbox(&mut permissions.approve_or_merge, "Approve or merge PRs");
        ui.checkbox(
            &mut permissions.scheduled_automations,
            "Run scheduled automations",
        );
        ui.checkbox(
            &mut permissions.local_only,
            "Strict local-only mode (overrides external AI)",
        );
        if ui.button("Save permissions").clicked() {
            save = true;
        }
    });
    if save {
        let policy = permissions.clone();
        let service = Arc::clone(service);
        let tx = tx.clone();
        runtime.spawn(async move {
            match service.save_permissions(&policy).await {
                Ok(()) => {
                    let _ = tx.send(UiEvent::Toast("Permission policy saved.".into()));
                }
                Err(error) => {
                    let _ = tx.send(UiEvent::Error(error.user_message()));
                }
            }
        });
    }
    components::panel(tokens).show(ui, |ui| {
        ui.label(
            RichText::new("Permission audit")
                .size(13.0)
                .strong()
                .color(tokens.text),
        );
        for permission in gitpilot_core::permissions::ALL_PERMISSIONS {
            let allowed = permission_allowed(permissions, permission);
            components::kv(
                ui,
                tokens,
                permission.label(),
                if allowed { "allowed" } else { "denied" },
            );
        }
        ui.label(
            RichText::new(format!(
                "Local-only mode: {}",
                if permissions.local_only {
                    "enforced"
                } else {
                    "off"
                }
            ))
            .size(10.0)
            .color(if permissions.local_only {
                tokens.success
            } else {
                tokens.warning
            }),
        );
    });
    components::panel(tokens).show(ui, |ui| {
        ui.label(
            RichText::new("Audit trail")
                .size(13.0)
                .strong()
                .color(tokens.text),
        );
        if state.audit.is_empty() {
            ui.label(
                RichText::new("No audit events loaded.")
                    .size(10.5)
                    .color(tokens.text_faint),
            );
            if ui.small_button("Load audit trail").clicked() {
                let service = Arc::clone(service);
                let tx = tx.clone();
                runtime.spawn(async move {
                    match service.audit_events().await {
                        Ok(events) => {
                            let _ = tx.send(UiEvent::Audit(events));
                        }
                        Err(error) => {
                            let _ = tx.send(UiEvent::Error(error.user_message()));
                        }
                    }
                });
            }
        } else if ui.small_button("Export audit metadata").clicked() {
            let rows = state
                .audit
                .iter()
                .map(|event| {
                    format!(
                        "{}\t{}\t{}\t{}\t{}",
                        event.occurred_at.to_rfc3339(),
                        event.actor,
                        event.action,
                        event.outcome,
                        event.target
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            let path = std::path::Path::new(&state.bootstrap.data_dir).join(format!(
                "audit-export-{}.tsv",
                chrono::Utc::now().format("%Y%m%d-%H%M%S")
            ));
            match std::fs::write(&path, rows) {
                Ok(()) => state.error = Some(format!("Audit exported to {}", path.display())),
                Err(error) => state.error = Some(format!("Audit export failed: {error}")),
            }
        }
        for event in state.audit.iter().take(20) {
            ui.horizontal(|ui| {
                components::badge(
                    ui,
                    tokens,
                    &event.outcome,
                    if event.outcome == "success" {
                        tokens.success
                    } else {
                        tokens.warning
                    },
                );
                ui.label(
                    RichText::new(&event.action)
                        .monospace()
                        .size(10.5)
                        .color(tokens.text),
                );
                ui.label(
                    RichText::new(&event.target)
                        .size(10.0)
                        .color(tokens.text_faint),
                );
            });
        }
    });
}

fn notifications(
    ui: &mut egui::Ui,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    components::panel(tokens).show(ui, |ui| {
        ui.label(
            RichText::new("Notification policy")
                .size(13.0)
                .strong()
                .color(tokens.text),
        );
        ui.checkbox(
            &mut state.bootstrap.settings.notifications_enabled,
            "In-app notifications enabled",
        );
        ui.checkbox(
            &mut state.bootstrap.settings.native_notifications_enabled,
            "Deliver new alerts through the operating system",
        );
        ui.label(
            RichText::new(format!(
                "Native notification adapter: {}",
                state.native_notification_status
            ))
            .size(10.0)
            .color(tokens.text_faint),
        );
        ui.checkbox(
            &mut state.bootstrap.settings.demo_enabled,
            "Demo profile available on first launch",
        );
        ui.label(
            RichText::new("People analytics visibility")
                .size(11.0)
                .color(tokens.text_faint),
        );
        ui.label(
            RichText::new(
                "Contributor views are team-level by default. Optional activity counters are context, never performance scores or rankings.",
            )
            .size(10.0)
            .color(tokens.text_muted),
        );
        ui.checkbox(
            &mut state.bootstrap.settings.individual_analytics_enabled,
            "Show contributor activity counters (not a performance score)",
        );
        let mut per_repo = state
            .bootstrap
            .settings
            .per_repository_notifications
            .clone();
        for repo in state
            .bootstrap
            .repositories
            .iter()
            .filter(|repo| repo.origin == gitpilot_core::DataOrigin::GitHub)
        {
            let enabled = per_repo.entry(repo.id.clone()).or_insert(true);
            ui.checkbox(enabled, format!("Notify for {}", repo.full_name));
        }
        state.bootstrap.settings.per_repository_notifications = per_repo;
        ui.horizontal(|ui| {
            ui.label("Quiet hours");
            let mut value = state
                .bootstrap
                .settings
                .quiet_hours
                .clone()
                .unwrap_or_default();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut value)
                        .hint_text("22:00-07:00")
                        .desired_width(130.0),
                )
                .changed()
            {
                state.bootstrap.settings.quiet_hours =
                    (!value.trim().is_empty()).then(|| value.trim().into());
            }
        });
        ui.horizontal(|ui| {
            ui.label("Sync interval");
            ui.add(
                egui::DragValue::new(&mut state.bootstrap.settings.sync_interval_minutes)
                    .range(1..=1440)
                    .suffix(" min"),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Hybrid retrieval");
            ui.checkbox(
                &mut state.bootstrap.settings.embeddings_enabled,
                "Enable optional embedding ranking",
            );
        ui.checkbox(
            &mut state.bootstrap.settings.external_embeddings_approved,
            "Approve sending indexed text to a configured external embedding provider",
        );
        });
        ui.label(
            RichText::new(format!(
                "Knowledge index: {}",
                state.embedding_status
            ))
            .size(10.0)
            .color(tokens.text_faint),
        );
        ui.label(
            RichText::new(
                "Alerts are deduplicated by stable event key. Periodic sync uses this interval for connected live accounts. Native delivery is best-effort and requires a packaged app with OS permission; quiet hours suppress both in-app alert creation and delivery.",
            )
            .size(10.5)
            .color(tokens.text_muted),
        );
        if ui.button("Save notification settings").clicked() {
            save_settings(state, service, runtime, tx);
        }
        if ui.button("Send test notification").clicked() {
            let service = Arc::clone(service);
            let tx = tx.clone();
            runtime.spawn(async move {
                match service.test_native_notification().await {
                    Ok(()) => {
                        let _ = tx.send(UiEvent::Toast(
                            "Native test notification submitted.".into(),
                        ));
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
    });
}

fn sandbox(
    ui: &mut egui::Ui,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    components::panel(tokens).show(ui, |ui| {
        ui.label(
            RichText::new("Local execution policy")
                .size(13.0)
                .strong()
                .color(tokens.text),
        );
        components::kv(
            ui,
            tokens,
            "Local execution",
            if state.bootstrap.permissions.local_execution {
                "enabled"
            } else {
                "denied by default"
            },
        );
        components::kv(
            ui,
            tokens,
            "File modifications",
            if state.bootstrap.permissions.modify_local_files {
                "enabled"
            } else {
                "denied"
            },
        );
        components::kv(ui, tokens, "Network for commands", "denied by default");
        ui.horizontal(|ui| {
            ui.label("Backend");
            egui::ComboBox::from_id_salt("sandbox_backend")
                .selected_text(&state.bootstrap.settings.sandbox_backend)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut state.bootstrap.settings.sandbox_backend,
                        "host".into(),
                        "Host process guard",
                    );
                    ui.selectable_value(
                        &mut state.bootstrap.settings.sandbox_backend,
                        "docker".into(),
                        "Docker container",
                    );
                    ui.selectable_value(
                        &mut state.bootstrap.settings.sandbox_backend,
                        "podman".into(),
                        "Podman container",
                    );
                });
        });
        ui.horizontal(|ui| {
            ui.label("Container image");
            ui.add(
                egui::TextEdit::singleline(&mut state.bootstrap.settings.sandbox_image)
                    .hint_text("rust:1.88-bookworm")
                    .desired_width(260.0),
            );
        });
        ui.checkbox(
            &mut state.bootstrap.settings.sandbox_allow_network,
            "Allow network for explicitly network-enabled commands",
        );
        ui.checkbox(
            &mut state.bootstrap.settings.sandbox_allow_file_writes,
            "Allow workspace writes for explicitly modifying commands",
        );
        ui.label(
            RichText::new(
                "Host mode uses a process guard; it is not a VM boundary. Container mode runs non-root with a read-only root filesystem, dropped capabilities, no host home mount, no Docker socket, PID/CPU/memory limits and network=none unless separately allowed. The source workspace is mounted read-only unless file writes are explicitly enabled. Docker must already be running and accessible.",
            )
            .size(10.5)
            .color(tokens.text_muted),
        );
        if ui.button("Save sandbox policy").clicked() {
            save_settings(state, service, runtime, tx);
        }
    });
}

fn data(
    ui: &mut egui::Ui,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    if ui.small_button("Check integrity").clicked() {
        let service = Arc::clone(service);
        let tx = tx.clone();
        runtime.spawn(async move {
            match service.store.integrity_check().await {
                Ok(result) => {
                    let _ = tx.send(UiEvent::Toast(format!("SQLite integrity: {result}")));
                }
                Err(error) => {
                    let _ = tx.send(UiEvent::Error(error.user_message()));
                }
            }
        });
    }
    components::panel(tokens).show(ui, |ui| {
        ui.label(
            RichText::new("Local storage")
                .size(13.0)
                .strong()
                .color(tokens.text),
        );
        components::kv(ui, tokens, "Data directory", &state.bootstrap.data_dir);
        components::kv(
            ui,
            tokens,
            "Schema",
            "SQLite WAL · FTS5 · foreign keys · versioned migrations",
        );
        components::kv(
            ui,
            tokens,
            "Credential store",
            if state.bootstrap.credential_store_available {
                "OS keychain available"
            } else {
                "unsupported platform"
            },
        );
        ui.horizontal(|ui| {
            if ui.button("Backup now").clicked() {
                let service = Arc::clone(service);
                let tx = tx.clone();
                runtime.spawn(async move {
                    match service.backup().await {
                        Ok(path) => {
                            let _ = tx.send(UiEvent::Toast(format!(
                                "Backup created at {}",
                                path.display()
                            )));
                        }
                        Err(error) => {
                            let _ = tx.send(UiEvent::Error(error.user_message()));
                        }
                    }
                });
            }
            if ui.button("Delete local data").clicked() {
                state.confirmed_delete.clear();
                state.confirm_action = Some("delete_all".into());
            }
        });
    });
    components::panel(tokens).show(ui, |ui| {
        ui.label(
            RichText::new("Retention")
                .size(13.0)
                .strong()
                .color(tokens.text),
        );
        ui.add(
            egui::DragValue::new(&mut state.bootstrap.settings.log_retention_days)
                .range(1..=365)
                .suffix(" days"),
        );
        ui.label(
            RichText::new(
                "Large Git histories and repository blobs are not stored in SQLite. Audit/provider usage retention is configurable.",
            )
            .size(10.5)
            .color(tokens.text_muted),
        );
        if ui.button("Save retention").clicked() {
            save_settings(state, service, runtime, tx);
        }
    });
}

pub fn health(
    ui: &mut egui::Ui,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    if state.health.is_empty() {
        let service = Arc::clone(service);
        let tx = tx.clone();
        runtime.spawn(async move {
            match service.health().await {
                Ok(health) => {
                    let _ = tx.send(UiEvent::Health(health));
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
        "Application health",
        "Database integrity, credential adapter, provider state and local runtime",
        |ui| {
            if ui.button("Run checks").clicked() {
                let service = Arc::clone(service);
                let tx = tx.clone();
                runtime.spawn(async move {
                    if let Ok(health) = service.health().await {
                        let _ = tx.send(UiEvent::Health(health));
                    }
                });
            }
        },
    );
    for (name, value) in &state.health {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                components::status_dot(
                    ui,
                    if value.contains("error") || value.contains("unsupported") {
                        tokens.warning
                    } else {
                        tokens.success
                    },
                );
                ui.label(RichText::new(name).size(11.5).strong().color(tokens.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(value).size(10.5).color(tokens.text_muted));
                });
            });
        });
        ui.add_space(4.0);
    }
    components::section_header(ui, tokens, "Runtime logs", None);
    ui.label(
        RichText::new(format!(
            "Logs are written below {} with credential redaction. Set RUST_LOG for verbosity.",
            state.bootstrap.data_dir
        ))
        .size(10.5)
        .color(tokens.text_muted),
    );
}

fn save_settings(
    state: &UiState,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    let settings = state.bootstrap.settings.clone();
    let service = Arc::clone(service);
    let tx = tx.clone();
    runtime.spawn(async move {
        match service.save_settings(&settings).await {
            Ok(()) => {
                let _ = tx.send(UiEvent::BootstrapRefreshed);
                let _ = tx.send(UiEvent::Toast("Settings saved.".into()));
            }
            Err(error) => {
                let _ = tx.send(UiEvent::Error(error.user_message()));
            }
        }
    });
}

fn permission_allowed(
    policy: &gitpilot_core::PermissionPolicy,
    permission: gitpilot_core::permissions::Permission,
) -> bool {
    use gitpilot_core::permissions::Permission;
    match permission {
        Permission::ReadGitHub => policy.read_github,
        Permission::IndexRepository => policy.index_repository,
        Permission::ExternalAi => policy.external_ai,
        Permission::LocalExecution => policy.local_execution,
        Permission::ModifyLocalFiles => policy.modify_local_files,
        Permission::ModifyGitHub => policy.modify_github,
        Permission::PublishReviews => policy.publish_reviews,
        Permission::ApproveOrMerge => policy.approve_or_merge,
        Permission::ScheduledAutomations => policy.scheduled_automations,
    }
}
