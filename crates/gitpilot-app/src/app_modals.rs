//! Modal surfaces kept separate from the root app loop for readability.

use crate::app::{GitPilotApp, Modal};
use crate::components;
use crate::nav;
use crate::state::UiEvent;
use eframe::egui::{self, Align, Layout, RichText};
use std::sync::Arc;

impl GitPilotApp {
    pub(crate) fn modals(&mut self, ctx: &egui::Context) {
        match self.modal {
            Modal::None => {}
            Modal::ConnectGitHub => self.github_modal(ctx),
            Modal::Notifications => self.notifications_modal(ctx),
            Modal::Jobs => self.jobs_modal(ctx),
            Modal::Shortcuts => self.shortcuts_modal(ctx),
            Modal::ReviewPolicy => self.review_policy_modal(ctx),
            Modal::ProviderSetup => self.provider_modal(ctx),
            Modal::ConfirmDelete => self.delete_modal(ctx),
        }
    }

    fn github_modal(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let mut connect = false;
        let mut discover = None;
        components::centered_modal(ctx, &self.tokens, 520.0, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Connect GitHub")
                        .size(18.0)
                        .strong()
                        .color(self.tokens.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
            ui.label(
                RichText::new(
                    "Use a fine-grained PAT with read-only repository, issue, pull request, checks and content access. The token is stored in the macOS Keychain, never SQLite.",
                )
                .size(11.5)
                .color(self.tokens.text_muted),
            );
            ui.add_space(10.0);
            ui.label(
                RichText::new("Host")
                    .size(11.0)
                    .color(self.tokens.text_faint),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.state.github_host)
                    .desired_width(f32::INFINITY)
                    .hint_text("github.com"),
            );
            ui.label(
                RichText::new("Personal access token")
                    .size(11.0)
                    .color(self.tokens.text_faint),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.state.github_token)
                    .password(true)
                    .desired_width(f32::INFINITY)
                    .hint_text("github_pat_…"),
            );
            ui.add_space(8.0);
            if !self.state.bootstrap.accounts.is_empty() {
                components::divider(ui, &self.tokens);
                ui.label(
                    RichText::new("CONNECTED ACCOUNTS")
                        .size(9.5)
                        .strong()
                        .color(self.tokens.text_faint),
                );
                for account in &self.state.bootstrap.accounts {
                    ui.horizontal(|ui| {
                        components::status_dot(ui, self.tokens.success);
                        ui.label(
                            RichText::new(format!("{} · {}", account.login, account.host))
                                .size(11.5)
                                .color(self.tokens.text),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button("Discover repositories").clicked() {
                                discover = Some(account.id.clone());
                            }
                        });
                    });
                }
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new("Connect securely").fill(self.tokens.accent))
                    .clicked()
                {
                    connect = true;
                }
                ui.label(
                    RichText::new("Demo mode remains available without connecting.")
                        .size(10.0)
                        .color(self.tokens.text_faint),
                );
            });
        });
        if close {
            self.modal = Modal::None;
        }
        if connect {
            let token = std::mem::take(&mut self.state.github_token);
            let host = self.state.github_host.clone();
            self.connect_github(token, host);
        }
        if let Some(account) = discover {
            self.discover(account);
        }
    }

    fn notifications_modal(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let mut mark_read = None;
        let mut snooze = None;
        components::centered_modal(ctx, &self.tokens, 560.0, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Notifications")
                        .size(18.0)
                        .strong()
                        .color(self.tokens.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
            ui.label(
                RichText::new("Deduplicated in-app alerts. Configure thresholds in Settings.")
                    .size(11.0)
                    .color(self.tokens.text_muted),
            );
            ui.add_space(8.0);
            let notifications = self.state.notifications.clone();
            if notifications.is_empty() {
                components::empty_state(
                    ui,
                    &self.tokens,
                    "No notifications",
                    "CI failures, review requests and stale work will appear here.",
                    None,
                );
            }
            egui::ScrollArea::vertical()
                .max_height(400.0)
                .show(ui, |ui| {
                    for notification in notifications {
                        components::panel(&self.tokens).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                components::severity_badge(ui, &self.tokens, notification.severity);
                                ui.label(
                                    RichText::new(&notification.title)
                                        .size(12.5)
                                        .strong()
                                        .color(self.tokens.text),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if notification.read_at.is_none()
                                        && ui.small_button("Read").clicked()
                                    {
                                        mark_read = Some(notification.id.clone());
                                    }
                                    if ui.small_button("Snooze 4h").clicked() {
                                        snooze = Some(notification.id.clone());
                                    }
                                });
                            });
                            ui.label(
                                RichText::new(&notification.body)
                                    .size(11.0)
                                    .color(self.tokens.text_muted),
                            );
                        });
                        ui.add_space(5.0);
                    }
                });
        });
        if close {
            self.modal = Modal::None;
        }
        if let Some(id) = mark_read {
            let service = Arc::clone(&self.service);
            let tx = self.tx.clone();
            self.runtime.spawn(async move {
                match service.mark_notification_read(&id).await {
                    Ok(()) => {
                        let _ = tx.send(UiEvent::BootstrapRefreshed);
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
        if let Some(id) = snooze {
            let service = Arc::clone(&self.service);
            let tx = self.tx.clone();
            self.runtime.spawn(async move {
                match service.snooze_notification(&id).await {
                    Ok(()) => {
                        let _ = tx.send(UiEvent::BootstrapRefreshed);
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
    }

    fn jobs_modal(&mut self, ctx: &egui::Context) {
        self.refresh_jobs(ctx);
        let mut close = false;
        let mut retry_job = None;
        let mut cancel_job = None;
        components::centered_modal(ctx, &self.tokens, 620.0, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Background jobs")
                        .size(18.0)
                        .strong()
                        .color(self.tokens.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
            ui.label(
                RichText::new(
                    "Persisted queue with restart recovery, bounded retries and cancellation.",
                )
                .size(11.0)
                .color(self.tokens.text_muted),
            );
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    for job in &self.state.jobs {
                        components::panel(&self.tokens).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let color = if job.state == "failed" {
                                    self.tokens.danger
                                } else if job.state == "completed" {
                                    self.tokens.success
                                } else {
                                    self.tokens.warning
                                };
                                components::status_dot(ui, color);
                                ui.label(
                                    RichText::new(&job.kind)
                                        .size(12.0)
                                        .strong()
                                        .color(self.tokens.text),
                                );
                                ui.label(
                                    RichText::new(format!("attempt {}", job.attempts))
                                        .size(10.5)
                                        .color(self.tokens.text_muted),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if job.state == "failed" && ui.small_button("Retry").clicked() {
                                        retry_job = Some(job.id.clone());
                                    }
                                    if matches!(job.state.as_str(), "queued" | "running")
                                        && ui.small_button("Cancel").clicked()
                                    {
                                        cancel_job = Some(job.id.clone());
                                    }
                                });
                            });
                            if let Some(error) = &job.error {
                                ui.label(RichText::new(error).size(10.5).color(self.tokens.danger));
                            }
                        });
                        ui.add_space(4.0);
                    }
                });
        });
        if close {
            self.modal = Modal::None;
        }
        if let Some(id) = retry_job {
            let service = Arc::clone(&self.service);
            let tx = self.tx.clone();
            self.runtime.spawn(async move {
                match service.retry_job(&id).await {
                    Ok(()) => {
                        let _ = tx.send(UiEvent::BootstrapRefreshed);
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
        if let Some(id) = cancel_job {
            let service = Arc::clone(&self.service);
            let tx = self.tx.clone();
            self.runtime.spawn(async move {
                match service.cancel_job(&id).await {
                    Ok(()) => {
                        let _ = tx.send(UiEvent::BootstrapRefreshed);
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
    }

    fn shortcuts_modal(&mut self, ctx: &egui::Context) {
        let mut close = false;
        components::centered_modal(ctx, &self.tokens, 480.0, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Keyboard shortcuts")
                        .size(18.0)
                        .strong()
                        .color(self.tokens.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(8.0);
            nav::shortcut_reference(ui, &self.tokens);
        });
        if close {
            self.modal = Modal::None;
        }
    }

    fn review_policy_modal(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let mut save = false;
        components::centered_modal(ctx, &self.tokens, 560.0, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Review policy")
                        .size(18.0)
                        .strong()
                        .color(self.tokens.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
            let policy = &mut self.state.review_policy_draft;
            ui.checkbox(&mut policy.enabled, "Enable automated review runs");
            ui.checkbox(&mut policy.auto_review_on_open, "Review when a PR opens");
            ui.checkbox(&mut policy.review_on_new_commits, "Review on new commits");
            ui.checkbox(&mut policy.include_drafts, "Include draft PRs");
            ui.horizontal(|ui| {
                ui.label("Depth");
                egui::ComboBox::from_id_salt("review_depth")
                    .selected_text(format!("{:?}", policy.depth))
                    .show_ui(ui, |ui| {
                        for depth in [
                            gitpilot_core::ReviewDepth::Fast,
                            gitpilot_core::ReviewDepth::Balanced,
                            gitpilot_core::ReviewDepth::Deep,
                        ] {
                            ui.selectable_value(&mut policy.depth, depth, format!("{depth:?}"));
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label("Budget (USD)");
                ui.add(
                    egui::DragValue::new(&mut policy.budget_usd)
                        .range(0.0..=10.0)
                        .speed(0.05),
                );
                ui.label("Max findings");
                ui.add(egui::DragValue::new(&mut policy.max_findings).range(1..=100));
            });
            ui.label(
                RichText::new("Publishing is private-only. No review comments are sent to GitHub.")
                    .size(10.5)
                    .color(self.tokens.warning),
            );
            ui.add_space(8.0);
            if ui.button("Save policy").clicked() {
                save = true;
            }
        });
        if close {
            self.modal = Modal::None;
        }
        if save {
            let policy = self.state.review_policy_draft.clone();
            let service = Arc::clone(&self.service);
            let tx = self.tx.clone();
            self.runtime.spawn(async move {
                if let Err(error) = service.save_review_policy(&policy).await {
                    let _ = tx.send(UiEvent::Error(error.user_message()));
                }
            });
            self.modal = Modal::None;
        }
    }

    fn provider_modal(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let mut save = false;
        let mut test = false;
        components::centered_modal(ctx, &self.tokens, 560.0, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Provider setup")
                        .size(18.0)
                        .strong()
                        .color(self.tokens.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
            let selected = self
                .state
                .selected_provider
                .clone()
                .or_else(|| self.state.bootstrap.providers.first().map(|p| p.id.clone()));
            egui::ComboBox::from_id_salt("provider_select")
                .selected_text(
                    selected
                        .as_ref()
                        .and_then(|id| self.state.bootstrap.providers.iter().find(|p| &p.id == id))
                        .map(|p| p.display_name.as_str())
                        .unwrap_or("Select provider"),
                )
                .show_ui(ui, |ui| {
                    for provider in &self.state.bootstrap.providers {
                        if ui
                            .selectable_label(
                                selected.as_deref() == Some(&provider.id),
                                &provider.display_name,
                            )
                            .clicked()
                        {
                            self.state.selected_provider = Some(provider.id.clone());
                            self.state.provider_model_edit = provider.model.clone();
                            self.state.provider_endpoint_edit = provider.endpoint.clone();
                        }
                    }
                });
            if let Some(provider) = selected
                .as_ref()
                .and_then(|id| self.state.bootstrap.providers.iter().find(|p| &p.id == id))
            {
                ui.label(
                    RichText::new(format!(
                        "{} · {} · {}",
                        provider.kind,
                        provider.model,
                        if provider.local_only {
                            "local only"
                        } else {
                            "external API"
                        }
                    ))
                    .size(10.5)
                    .color(self.tokens.text_muted),
                );
                ui.add_space(5.0);
                ui.label(
                    RichText::new("Model")
                        .size(11.0)
                        .color(self.tokens.text_faint),
                );
                if self.state.provider_model_edit.is_empty() {
                    self.state.provider_model_edit = provider.model.clone();
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.state.provider_model_edit)
                        .desired_width(f32::INFINITY),
                );
                ui.label(
                    RichText::new("Endpoint")
                        .size(11.0)
                        .color(self.tokens.text_faint),
                );
                if self.state.provider_endpoint_edit.is_empty() {
                    self.state.provider_endpoint_edit = provider.endpoint.clone();
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.state.provider_endpoint_edit)
                        .desired_width(f32::INFINITY),
                );
                if provider.kind != "ollama" {
                    ui.label(
                        RichText::new("API key")
                            .size(11.0)
                            .color(self.tokens.text_faint),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.state.provider_api_key)
                            .password(true)
                            .desired_width(f32::INFINITY)
                            .hint_text(if provider.has_credential {
                                "Credential stored in OS keychain"
                            } else {
                                "Paste API key"
                            }),
                    );
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save provider").clicked() {
                        save = true;
                    }
                    if ui.button("Test connection").clicked() {
                        test = true;
                    }
                });
            }
        });
        if close {
            self.modal = Modal::None;
        }
        if save
            && let Some(id) = self.state.selected_provider.clone()
            && let Some(mut config) = self
                .state
                .bootstrap
                .providers
                .iter()
                .find(|p| p.id == id)
                .cloned()
        {
            config.model = self.state.provider_model_edit.clone();
            config.endpoint = self.state.provider_endpoint_edit.clone();
            let secret = std::mem::take(&mut self.state.provider_api_key);
            let service = Arc::clone(&self.service);
            let tx = self.tx.clone();
            self.runtime.spawn(async move {
                match service
                    .save_provider(&config, (!secret.is_empty()).then_some(secret.as_str()))
                    .await
                {
                    Ok(()) => {
                        let _ = tx.send(UiEvent::BootstrapRefreshed);
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
            self.modal = Modal::None;
        }
        if test && let Some(id) = self.state.selected_provider.clone() {
            let service = Arc::clone(&self.service);
            let tx = self.tx.clone();
            self.runtime.spawn(async move {
                match service.test_provider(&id).await {
                    Ok(()) => {
                        let _ = tx.send(UiEvent::Toast("Provider connection test passed.".into()));
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
    }

    fn delete_modal(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let mut delete = false;
        components::centered_modal(ctx, &self.tokens, 500.0, |ui| {
            ui.label(
                RichText::new("Delete local GitPilot data")
                    .size(18.0)
                    .strong()
                    .color(self.tokens.danger),
            );
            ui.label(
                RichText::new(
                    "This removes repositories, cached PRs/issues, knowledge, reviews, automations and audit history from SQLite. It does not delete GitHub resources. Credentials remain in the OS keychain and must be disconnected separately.",
                )
                .size(11.5)
                .color(self.tokens.text_muted),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new("Type DELETE to confirm")
                    .size(11.0)
                    .color(self.tokens.text_faint),
            );
            ui.text_edit_singleline(&mut self.state.confirmed_delete);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        self.state.confirmed_delete == "DELETE",
                        egui::Button::new("Delete local data").fill(self.tokens.danger),
                    )
                    .clicked()
                {
                    delete = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if close {
            self.modal = Modal::None;
            self.state.confirmed_delete.clear();
        }
        if delete {
            let service = Arc::clone(&self.service);
            let tx = self.tx.clone();
            self.runtime.spawn(async move {
                match service.delete_all_data().await {
                    Ok(()) => {
                        let _ = tx.send(UiEvent::Toast("Local data deleted.".into()));
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
            self.modal = Modal::None;
            self.state.confirmed_delete.clear();
        }
    }
}
