use crate::components;
use crate::nav::{self, Screen, TopBarActions};
use crate::screens;
use crate::state::{BusyState, UiEvent, UiState};
use crate::theme::{self, Theme, Tokens};
use eframe::egui::{self, Align, Layout, RichText};
use gitpilot_core::{AppService, AppSettings, Bootstrap, ThemeMode};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub struct GitPilotApp {
    pub(crate) service: Arc<AppService>,
    pub(crate) runtime: tokio::runtime::Handle,
    pub(crate) tx: mpsc::UnboundedSender<UiEvent>,
    rx: mpsc::UnboundedReceiver<UiEvent>,
    pub(crate) state: UiState,
    pub(crate) tokens: Tokens,
    shutdown: CancellationToken,
    last_tick: Instant,
    last_sync_tick: Instant,
    last_device_poll: Instant,
    sync_inflight: bool,
    last_automation_tick: Instant,
    started: Instant,
    pub(crate) modal: Modal,
    palette: Option<Palette>,
    scope_picker: bool,
    toast: Option<(String, bool, Instant)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modal {
    None,
    ConnectGitHub,
    Notifications,
    Jobs,
    Shortcuts,
    ReviewPolicy,
    ProviderSetup,
    ConfirmDelete,
    ConfirmLineComment,
}

#[derive(Debug, Clone)]
pub struct Palette {
    pub query: String,
    pub results: Vec<gitpilot_core::SearchHit>,
    pub selected: usize,
    pub kind: PaletteKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteKind {
    Command,
    Search,
}

impl GitPilotApp {
    pub fn new(
        service: Arc<AppService>,
        runtime: tokio::runtime::Handle,
        bootstrap: Bootstrap,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let theme = theme_for(&bootstrap.settings);
        let tokens = match theme {
            Theme::Dark => Tokens::dark(),
            Theme::Light => Tokens::light(),
        };
        let mut state = UiState::from_bootstrap(bootstrap);
        if !state.bootstrap.settings.onboarding_complete {
            state.screen = Screen::Onboarding;
        }
        state.native_notification_status =
            gitpilot_core::native_notifications::status().label().into();
        Self {
            service,
            runtime,
            tx,
            rx,
            state,
            tokens,
            shutdown: CancellationToken::new(),
            last_tick: Instant::now(),
            last_sync_tick: Instant::now(),
            last_device_poll: Instant::now(),
            sync_inflight: false,
            last_automation_tick: Instant::now(),
            started: Instant::now(),
            modal: Modal::None,
            palette: None,
            scope_picker: false,
            toast: None,
        }
    }

    fn refresh(&mut self) {
        self.state.busy = Some(BusyState::Loading("Refreshing workspace data".into()));
        let service = Arc::clone(&self.service);
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            match service.bootstrap().await {
                Ok(bootstrap) => {
                    let _ = tx.send(UiEvent::Bootstrap(Box::new(bootstrap)));
                }
                Err(error) => {
                    error!(%error, "bootstrap refresh failed");
                    let _ = tx.send(UiEvent::Error(error.user_message()));
                }
            }
        });
    }

    pub(crate) fn connect_github(&mut self, token: String, host: String) {
        if token.trim().is_empty() {
            self.state.error = Some("Enter a GitHub access token.".into());
            return;
        }
        self.state
            .busy
            .replace(BusyState::Loading("Validating GitHub credential".into()));
        let service = Arc::clone(&self.service);
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            match service.connect_github(&token, &host).await {
                Ok(account) => {
                    let _ = tx.send(UiEvent::Connected(account));
                }
                Err(error) => {
                    let _ = tx.send(UiEvent::Error(error.user_message()));
                }
            }
        });
    }

    pub(crate) fn discover(&mut self, account_id: String) {
        self.state
            .busy
            .replace(BusyState::Loading("Discovering repositories".into()));
        let service = Arc::clone(&self.service);
        let tx = self.tx.clone();
        let cancel = self.shutdown.child_token();
        self.runtime.spawn(async move {
            match service.discover_repositories(&account_id, &cancel).await {
                Ok(repos) => {
                    let _ = tx.send(UiEvent::RepositoriesDiscovered(repos));
                }
                Err(error) => {
                    let _ = tx.send(UiEvent::Error(error.user_message()));
                }
            }
        });
    }

    fn sync_now(&mut self) {
        if self.sync_inflight
            || self.state.bootstrap.accounts.is_empty()
            || self
                .state
                .bootstrap
                .accounts
                .iter()
                .all(|account| account.id == "account-0001")
        {
            return;
        }
        self.sync_inflight = true;
        let service = Arc::clone(&self.service);
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            match service.enqueue_sync().await {
                Ok(job_id) => {
                    let _ = tx.send(UiEvent::Toast(format!(
                        "Sync queued ({job_id}); it will run through the persisted job dispatcher."
                    )));
                    let _ = tx.send(UiEvent::BootstrapRefreshed);
                }
                Err(error) => {
                    let _ = tx.send(UiEvent::Error(error.user_message()));
                }
            }
        });
    }

    pub(crate) fn refresh_jobs(&mut self, _ctx: &egui::Context) {
        let service = Arc::clone(&self.service);
        let tx = self.tx.clone();
        self.runtime.spawn(async move {
            if let Ok(jobs) = service.store.jobs(100).await {
                let _ = tx.send(UiEvent::Jobs(jobs));
            }
        });
    }

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                UiEvent::Bootstrap(bootstrap) => {
                    let theme = theme_for(&bootstrap.settings);
                    let screen = self.state.screen;
                    let recent_search = self.state.recent_search.clone();
                    self.state = UiState::from_bootstrap(*bootstrap);
                    self.state.screen = screen;
                    self.state.recent_search = recent_search;
                    self.state.notifications = self.state.bootstrap.notifications.clone();
                    self.state.jobs = self.state.bootstrap.jobs.clone();
                    self.state.native_notification_status =
                        gitpilot_core::native_notifications::status().label().into();
                    self.tokens = match theme {
                        Theme::Dark => Tokens::dark(),
                        Theme::Light => Tokens::light(),
                    };
                    theme::install(
                        ctx,
                        theme,
                        self.state.bootstrap.settings.font_scale,
                        &self.state.bootstrap.settings.density,
                    );
                    // Re-populate recent palette entries after bootstrap so
                    // the command palette never depends on stale UI state.
                    let service = Arc::clone(&self.service);
                    let tx = self.tx.clone();
                    self.runtime.spawn(async move {
                        if let Ok(items) = service.store.recent_items(24).await {
                            let _ = tx.send(UiEvent::SearchResults(items));
                        }
                    });
                }
                UiEvent::Error(message) => {
                    self.state.error = Some(message);
                    self.state.busy = None;
                    self.state.syncing = false;
                    self.sync_inflight = false;
                }
                UiEvent::Connected(account) => {
                    self.state.bootstrap.accounts.push(account.clone());
                    self.state.busy = None;
                    self.modal = Modal::None;
                    self.toast = Some((
                        format!("Connected {}", account.login),
                        false,
                        Instant::now(),
                    ));
                    self.refresh();
                }
                UiEvent::RepositoriesDiscovered(repos) => {
                    self.state.busy = None;
                    self.toast = Some((
                        format!("Discovered {} repositories.", repos.len()),
                        false,
                        Instant::now(),
                    ));
                    self.refresh();
                }
                UiEvent::Toast(message) => {
                    self.state.busy = None;
                    self.toast = Some((message, false, Instant::now()));
                }
                UiEvent::CommandOutput(output) => {
                    self.state.busy = None;
                    self.state.local_command_output = Some(output);
                }
                UiEvent::BootstrapRefreshed => {
                    self.state.busy = None;
                    self.sync_inflight = false;
                    self.refresh();
                }
                UiEvent::Jobs(jobs) => {
                    self.state.jobs = jobs;
                }
                UiEvent::Models(models) => {
                    self.state.provider_models = models;
                }
                UiEvent::EmbeddingStatus {
                    enabled,
                    provider_id,
                    count,
                } => {
                    self.state.embedding_provider = provider_id;
                    self.state.embedding_status = if enabled {
                        format!("hybrid retrieval enabled · {count} embeddings")
                    } else {
                        format!("FTS5 only · {count} embeddings cached")
                    };
                }
                UiEvent::FindingPublicationPreview(rows) => {
                    self.state.finding_publish_preview = rows;
                }
                UiEvent::ChangeImpactSignals(signals) => {
                    self.state.change_impact_signals = signals;
                }
                UiEvent::AiStreamDelta(delta) => {
                    self.state.ai_stream_text.push_str(&delta);
                }
                UiEvent::DeviceFlow(start) => {
                    self.state.github_device_code = start.device_code.clone();
                    self.state.github_user_code = start.user_code.clone();
                    self.state.github_verification_uri = start.verification_uri.clone();
                    let _ = webbrowser::open(&start.verification_uri);
                    self.toast = Some((
                        format!("Authorize GitPilot using code {}", start.user_code),
                        false,
                        Instant::now(),
                    ));
                }
                UiEvent::DeviceFlowAuthorized(account) => {
                    self.state.bootstrap.accounts.push(account.clone());
                    self.state.github_device_code.clear();
                    self.state.github_user_code.clear();
                    self.modal = Modal::None;
                    self.toast = Some((
                        format!("Connected {}", account.login),
                        false,
                        Instant::now(),
                    ));
                    self.refresh();
                }
                UiEvent::SearchResults(hits) => {
                    self.state.recent_search = hits.clone();
                    let selected = self.palette.as_ref().map(|_| 0).unwrap_or(0);
                    if let Some(palette) = &mut self.palette {
                        palette.results = hits;
                        palette.selected = selected;
                    }
                }
                UiEvent::SavedViews(views) => {
                    self.state.saved_pr_views = views;
                }
                UiEvent::PrDetail(detail) => {
                    // Ignore a late response for a PR the user already left.
                    if self.state.selected_pr.as_deref() == Some(detail.pull_request.id.as_str()) {
                        if let Some(run) = detail.latest_review_run.clone() {
                            self.state.review_run = Some(run);
                        }
                        self.state.pr_detail = Some(*detail);
                        self.state.busy = None;
                    }
                }
                UiEvent::ReviewHistory(runs) => {
                    self.state.review_history = runs;
                }
                UiEvent::ReviewerRecommendations(rows) => {
                    self.state.reviewer_recommendations = rows;
                }
                UiEvent::PrList { rows, total } => {
                    self.state.pr_rows = rows;
                    self.state.pr_total = total;
                    self.state.pr_loading = false;
                }
                UiEvent::Explanation(explanation) => {
                    self.state.explanation = Some(explanation);
                    self.state.busy = None;
                }
                UiEvent::Brief(brief) => {
                    self.state.brief = Some(brief);
                    self.state.busy = None;
                }
                UiEvent::Contributors(contributors) => {
                    self.state.contributors = contributors;
                    self.state.busy = None;
                }
                UiEvent::Commits(commits) => {
                    self.state.commits = commits;
                    self.state.busy = None;
                }
                UiEvent::CommitFiles(files) => {
                    self.state.commit_files = files;
                    self.state.busy = None;
                }
                UiEvent::CommitDetail(commit) => {
                    self.state.selected_commit_detail = Some(*commit);
                    self.state.busy = None;
                }
                UiEvent::Issues(issues) => {
                    self.state.issues = issues;
                    self.state.busy = None;
                }
                UiEvent::Automations(automations) => {
                    self.state.automations = automations;
                    self.state.busy = None;
                }
                UiEvent::AutomationRuns(runs) => {
                    self.state.automation_runs = runs;
                }
                UiEvent::WebhookEvents(events) => {
                    self.state.webhook_events = events;
                }
                UiEvent::Health(health) => {
                    self.state.health = health;
                    self.state.busy = None;
                }
                UiEvent::LocalRepos(repos) => {
                    self.state.local_repos = repos;
                    self.state.busy = None;
                }
                UiEvent::Milestones(rows) => {
                    self.state.milestones = rows;
                }
                UiEvent::SecurityAlerts(alerts) => {
                    self.state.security_alerts = alerts;
                }
                UiEvent::ReleaseReport(report) => {
                    self.state.release_report = Some(*report);
                }
                UiEvent::LocalStatus(status) => {
                    self.state.local_status = Some(status);
                    self.state.busy = None;
                }
                UiEvent::LocalBranches { path, branches } => {
                    self.state.local_branches = branches;
                    self.state.local_branches_repo = Some(path);
                }
                UiEvent::LocalSearch(results) => {
                    self.state.local_search_results = results;
                }
                UiEvent::RepositoryFile(content) => {
                    if let Some((_, value)) = &mut self.state.source_preview {
                        *value = content;
                    }
                    self.state.busy = None;
                }
                UiEvent::CompareResult(result) => {
                    self.state.compare_result = Some(result);
                }
                UiEvent::Audit(events) => {
                    self.state.audit = events;
                    self.state.busy = None;
                }
                UiEvent::Usage((tokens, requests, cost)) => {
                    self.state.bootstrap.usage.tokens = tokens;
                    self.state.bootstrap.usage.requests = requests;
                    self.state.bootstrap.usage.estimated_cost_usd = cost;
                }
            }
        }
    }

    fn command_palette(&mut self, ctx: &egui::Context) {
        let Some(mut palette) = self.palette.take() else {
            return;
        };
        let mut keep_open = true;
        egui::Window::new("command_palette")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
            .max_width(600.0)
            .frame(components::modal_frame(&self.tokens))
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(match palette.kind {
                        PaletteKind::Command => "Command palette",
                        PaletteKind::Search => "Search workspace",
                    })
                    .size(11.0_f32)
                    .color(self.tokens.text_faint),
                );
                let response = ui.add(
                    egui::TextEdit::singleline(&mut palette.query)
                        .hint_text("Type a command or search repositories, PRs, issues, knowledge…")
                        .desired_width(f32::INFINITY)
                        .font(egui::FontId::proportional(14.0)),
                );
                response.request_focus();
                if palette.kind == PaletteKind::Search
                    && response.changed()
                    && palette.query.len() >= 2
                {
                    let service = Arc::clone(&self.service);
                    let tx = self.tx.clone();
                    let query = palette.query.clone();
                    self.runtime.spawn(async move {
                        match service.search(&query, None).await {
                            Ok(hits) => {
                                let _ = tx.send(UiEvent::SearchResults(hits));
                            }
                            Err(error) => {
                                let _ = tx.send(UiEvent::Error(error.user_message()));
                            }
                        }
                    });
                }
                if palette.kind == PaletteKind::Command {
                    let commands = command_items();
                    let filtered: Vec<_> = commands
                        .iter()
                        .filter(|(label, _)| {
                            palette.query.trim().is_empty()
                                || label
                                    .to_ascii_lowercase()
                                    .contains(&palette.query.to_ascii_lowercase())
                        })
                        .collect();
                    for (index, (label, screen)) in filtered.iter().enumerate() {
                        let selected = index == palette.selected;
                        let response = ui.add(
                            egui::Button::new(
                                RichText::new(format!("{}   {}", (*screen).icon(), label))
                                    .size(12.0),
                            )
                            .fill(if selected {
                                self.tokens.accent_soft
                            } else {
                                self.tokens.surface
                            })
                            .frame(true),
                        );
                        if response.clicked() {
                            self.state.screen = *screen;
                            keep_open = false;
                        }
                    }
                } else {
                    if palette.query.trim().is_empty() {
                        ui.label(
                            RichText::new("Recent items")
                                .size(9.0)
                                .strong()
                                .color(self.tokens.text_faint),
                        );
                    } else if palette.results.is_empty() {
                        ui.add_space(8.0);
                        components::loading(
                            ui,
                            &self.tokens,
                            if palette.query.len() < 2 {
                                "Type at least two characters"
                            } else {
                                "Searching"
                            },
                        );
                    }
                    for (index, hit) in palette.results.iter().take(10).enumerate() {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!(
                                        "{}  ·  {}\n{}",
                                        hit.kind, hit.title, hit.subtitle
                                    ))
                                    .size(11.5),
                                )
                                .fill(
                                    if index == palette.selected {
                                        self.tokens.accent_soft
                                    } else {
                                        self.tokens.surface
                                    },
                                ),
                            )
                            .clicked()
                        {
                            self.open_search_hit(hit.clone());
                            keep_open = false;
                        }
                    }
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new("↑↓ move   ↵ open   esc close")
                        .size(9.5)
                        .color(self.tokens.text_faint),
                );
                if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    keep_open = false;
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                    keep_open = false;
                }
            });
        if keep_open {
            self.palette = Some(palette);
        }
    }

    fn scope_picker(&mut self, ctx: &egui::Context) {
        if !self.scope_picker {
            return;
        }
        let mut close = false;
        let mut choice: Option<Option<String>> = None;
        egui::Window::new("scope_picker")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
            .fixed_size([360.0, 0.0])
            .frame(components::modal_frame(&self.tokens))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Repository scope")
                            .size(15.0)
                            .strong()
                            .color(self.tokens.text),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                    });
                });
                if ui.button("All repositories").clicked() {
                    choice = Some(None);
                }
                for repo in &self.state.bootstrap.repositories {
                    if ui
                        .selectable_label(
                            self.state.repository_scope.as_ref() == Some(&repo.id),
                            format!("{}  ·  {}", repo.full_name, repo.origin.label()),
                        )
                        .clicked()
                    {
                        choice = Some(Some(repo.id.clone()));
                    }
                }
            });
        if close {
            self.scope_picker = false;
        }
        if let Some(choice) = choice {
            self.state.repository_scope = choice;
            self.state.pr_page = 0;
            self.state.pr_rows.clear();
            self.state.issues.clear();
            self.state.commits.clear();
            self.state.milestones.clear();
            self.state.security_alerts.clear();
            self.scope_picker = false;
            self.refresh();
        }
    }

    fn open_search_hit(&mut self, hit: gitpilot_core::SearchHit) {
        match hit.kind.as_str() {
            "pull_request" => {
                self.state.screen = Screen::PullRequests;
                self.state.selected_pr = Some(hit.id);
            }
            "issue" => self.state.screen = Screen::Issues,
            "commit" => {
                self.state.screen = Screen::Commits;
                self.state.selected_commit = Some(hit.id);
            }
            "repository" => self.state.screen = Screen::Repositories,
            "knowledge" => self.state.screen = Screen::AiWorkspace,
            _ => {}
        }
    }

    fn keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        let (cmd, shift, key_k, key_f, key_r, key_b, key_comma, key_p, key_shift_r) =
            ctx.input(|i| {
                (
                    i.modifiers.command,
                    i.modifiers.shift,
                    i.key_pressed(egui::Key::K),
                    i.key_pressed(egui::Key::F),
                    i.key_pressed(egui::Key::R),
                    i.key_pressed(egui::Key::B),
                    i.key_pressed(egui::Key::Comma),
                    i.key_pressed(egui::Key::P),
                    i.key_pressed(egui::Key::R),
                )
            });
        if cmd && key_k {
            self.palette = Some(Palette {
                query: String::new(),
                results: vec![],
                selected: 0,
                kind: PaletteKind::Command,
            });
        }
        if cmd && key_f {
            self.palette = Some(Palette {
                query: String::new(),
                results: self.state.recent_search.clone(),
                selected: 0,
                kind: PaletteKind::Search,
            });
        }
        if cmd && key_r {
            self.refresh();
        }
        if cmd && key_b {
            self.state.sidebar_collapsed = !self.state.sidebar_collapsed;
        }
        if cmd && key_comma {
            self.state.screen = Screen::Settings;
        }
        if cmd && shift && key_p {
            self.state.screen = Screen::PullRequests;
        }
        if cmd && shift && key_shift_r {
            self.state.screen = Screen::Repositories;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.modal = Modal::None;
            self.palette = None;
        }
    }

    fn top_right_actions(&mut self, ui: &mut egui::Ui, actions: &TopBarActions) {
        if actions.open_search {
            self.palette = Some(Palette {
                query: String::new(),
                results: self.state.recent_search.clone(),
                selected: 0,
                kind: PaletteKind::Search,
            });
        }
        if actions.open_scope {
            self.scope_picker = true;
        }
        if actions.open_notifications {
            self.modal = Modal::Notifications;
        }
        if actions.open_jobs {
            self.modal = Modal::Jobs;
        }
        if actions.open_account {
            self.modal = Modal::ConnectGitHub;
        }
        let _ = ui;
    }
}

impl eframe::App for GitPilotApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.state.palette_hint == "review_policy" {
            self.state.palette_hint.clear();
            self.modal = Modal::ReviewPolicy;
        } else if self.state.palette_hint == "provider_setup" {
            self.state.palette_hint.clear();
            self.modal = Modal::ProviderSetup;
        } else if self.state.confirm_action.as_deref() == Some("delete_all") {
            self.state.confirm_action = None;
            self.modal = Modal::ConfirmDelete;
        } else if self.state.confirm_action.as_deref() == Some("publish_line_comment") {
            self.state.confirm_action = None;
            self.modal = Modal::ConfirmLineComment;
        } else if self.state.confirm_action.as_deref() == Some("publish_findings") {
            // The PR cockpit renders this confirmation inline with its selected
            // findings; do not route it into the generic line-comment modal.
        }
        self.handle_events(ctx);
        self.keyboard_shortcuts(ctx);
        if self.last_tick.elapsed() > Duration::from_secs(2) {
            self.last_tick = Instant::now();
            ctx.request_repaint_after(Duration::from_secs(2));
        }
        let interval = self.state.bootstrap.settings.sync_interval_minutes.max(1) as u64;
        if self.state.bootstrap.demo_seeded
            && self
                .state
                .bootstrap
                .accounts
                .iter()
                .all(|a| a.id == "account-0001")
        {
            // Demo fixture accounts are not live GitHub credentials. Never send
            // a periodic sync request for them.
        } else if self.last_sync_tick.elapsed() >= Duration::from_secs(interval * 60)
            && !self.state.bootstrap.accounts.is_empty()
        {
            self.last_sync_tick = Instant::now();
            self.sync_now();
            ctx.request_repaint_after(Duration::from_secs(2));
        }
        if self.state.bootstrap.permissions.scheduled_automations
            && self.last_automation_tick.elapsed() >= Duration::from_secs(15 * 60)
        {
            self.last_automation_tick = Instant::now();
            let service = Arc::clone(&self.service);
            let tx = self.tx.clone();
            self.runtime.spawn(async move {
                match service.run_scheduled_automations().await {
                    Ok(count) if count > 0 => {
                        let _ = tx.send(UiEvent::Toast(format!(
                            "Scheduled automations completed {count} run(s)."
                        )));
                        let _ = tx.send(UiEvent::BootstrapRefreshed);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
        if let Some((_, _, when)) = &self.toast
            && when.elapsed() > Duration::from_secs(5)
        {
            self.toast = None;
        }
        if !self.state.github_device_code.is_empty()
            && self.last_device_poll.elapsed() > Duration::from_secs(5)
        {
            self.last_device_poll = Instant::now();
            let service = Arc::clone(&self.service);
            let tx = self.tx.clone();
            let host = self.state.github_host.clone();
            let device_code = self.state.github_device_code.clone();
            self.runtime.spawn(async move {
                match service.poll_github_device_flow(&host, &device_code).await {
                    Ok(gitpilot_core::DeviceFlowPoll::Authorized(account)) => {
                        let _ = tx.send(UiEvent::DeviceFlowAuthorized(account));
                    }
                    Ok(gitpilot_core::DeviceFlowPoll::Expired) => {
                        let _ = tx.send(UiEvent::Error(
                            "GitHub device authorization expired; start again.".into(),
                        ));
                    }
                    Ok(gitpilot_core::DeviceFlowPoll::Denied) => {
                        let _ = tx.send(UiEvent::Error(
                            "GitHub device authorization was denied.".into(),
                        ));
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
        let desired_theme = theme_for(&self.state.bootstrap.settings);
        if (desired_theme == Theme::Light) != !self.tokens.dark {
            self.tokens = match desired_theme {
                Theme::Dark => Tokens::dark(),
                Theme::Light => Tokens::light(),
            };
            theme::install(
                ctx,
                desired_theme,
                self.state.bootstrap.settings.font_scale,
                &self.state.bootstrap.settings.density,
            );
        }
        egui::TopBottomPanel::top("top_bar")
            .exact_height(44.0)
            .frame(
                egui::Frame::new()
                    .fill(self.tokens.sidebar)
                    .stroke(egui::Stroke::new(1.0_f32, self.tokens.border))
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                let unread = self
                    .state
                    .notifications
                    .iter()
                    .filter(|n| n.read_at.is_none())
                    .count() as i64;
                let model_status = if self
                    .state
                    .bootstrap
                    .providers
                    .iter()
                    .any(|p| p.has_credential && p.enabled && p.kind != "mock")
                {
                    "AI ready"
                } else {
                    "AI not configured · demo"
                };
                let scope = self
                    .state
                    .repository_scope
                    .as_ref()
                    .and_then(|id| {
                        self.state
                            .bootstrap
                            .repositories
                            .iter()
                            .find(|r| &r.id == id)
                    })
                    .map(|r| r.full_name.as_str())
                    .unwrap_or("All repositories");
                let actions = nav::top_bar(
                    ui,
                    &self.tokens,
                    &mut self.state.global_search,
                    model_status,
                    scope,
                    self.state
                        .bootstrap
                        .jobs
                        .iter()
                        .filter(|j| j.state == "running")
                        .count(),
                    unread,
                );
                self.top_right_actions(ui, &actions);
            });
        egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(if self.state.sidebar_collapsed {
                62.0
            } else {
                206.0
            })
            .width_range(if self.state.sidebar_collapsed {
                56.0..=70.0
            } else {
                174.0..=280.0
            })
            .frame(
                egui::Frame::new()
                    .fill(self.tokens.sidebar)
                    .stroke(egui::Stroke::new(1.0_f32, self.tokens.border))
                    .inner_margin(egui::Margin::symmetric(6, 2)),
            )
            .show(ctx, |ui| {
                if let Some(screen) = nav::sidebar(
                    ui,
                    &self.tokens,
                    self.state.screen,
                    self.state.sidebar_collapsed,
                    self.state
                        .notifications
                        .iter()
                        .filter(|n| n.read_at.is_none())
                        .count() as i64,
                    self.state.bootstrap.repositories.len() as i64,
                ) {
                    self.state.screen = screen;
                    self.state.selected_pr = None;
                    self.state.selected_commit = None;
                }
            });
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(24.0)
            .frame(
                egui::Frame::new()
                    .fill(self.tokens.sidebar)
                    .stroke(egui::Stroke::new(1.0_f32, self.tokens.border))
                    .inner_margin(egui::Margin::symmetric(12, 3)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if self.state.bootstrap.dashboard.demo {
                            "DEMO PROFILE"
                        } else {
                            "LIVE PROFILE"
                        })
                        .size(9.0)
                        .strong()
                        .color(if self.state.bootstrap.dashboard.demo {
                            self.tokens.warning
                        } else {
                            self.tokens.success
                        }),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} repos · {} PRs · SQLite + FTS5 · v{}",
                            self.state.bootstrap.stats.repositories,
                            self.state.bootstrap.stats.pull_requests,
                            gitpilot_core::APP_VERSION
                        ))
                        .size(9.5)
                        .color(self.tokens.text_faint),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(busy) = &self.state.busy {
                            ui.spinner();
                            ui.label(
                                RichText::new(busy.label())
                                    .size(9.5)
                                    .color(self.tokens.text_faint),
                            );
                        } else {
                            ui.label(
                                RichText::new(format!(
                                    "synced {}",
                                    self.state
                                        .bootstrap
                                        .dashboard
                                        .last_synced_at
                                        .map(components::short_time)
                                        .unwrap_or_else(|| "never".into())
                                ))
                                .size(9.5)
                                .color(self.tokens.text_faint),
                            );
                        }
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(self.tokens.bg)
                    .inner_margin(egui::Margin::symmetric(18, 14)),
            )
            .show(ctx, |ui| {
                if let Some(error) = self.state.error.clone() {
                    let mut retry = false;
                    components::error_banner(ui, &self.tokens, &error, &mut retry);
                    if retry {
                        self.state.error = None;
                        self.refresh();
                    }
                }
                screens::show(
                    ui,
                    ctx,
                    &mut self.state,
                    &self.tokens,
                    &self.service,
                    &self.runtime,
                    &self.tx,
                );
            });
        if self.state.show_shortcuts {
            self.modal = Modal::Shortcuts;
            self.state.show_shortcuts = false;
        }
        self.modals(ctx);
        self.command_palette(ctx);
        self.scope_picker(ctx);
        if let Some((message, is_error, _)) = &self.toast {
            egui::Area::new(egui::Id::new("toast"))
                .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -36.0])
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(self.tokens.surface)
                        .stroke(egui::Stroke::new(
                            1.0_f32,
                            if *is_error {
                                self.tokens.danger
                            } else {
                                self.tokens.border
                            },
                        ))
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::symmetric(14, 9))
                        .show(ui, |ui| {
                            ui.label(RichText::new(message).size(11.5).color(self.tokens.text));
                        });
                });
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shutdown.cancel();
        info!(uptime = ?self.started.elapsed(), "GitPilot exiting");
    }
}

fn theme_for(settings: &AppSettings) -> Theme {
    match settings.theme {
        ThemeMode::Light => Theme::Light,
        ThemeMode::Dark => Theme::Dark,
        ThemeMode::System => Theme::Dark,
    }
}

fn command_items() -> Vec<(&'static str, Screen)> {
    vec![
        ("Overview", Screen::Overview),
        ("Pull requests", Screen::PullRequests),
        ("Repositories", Screen::Repositories),
        ("Commits & changes", Screen::Commits),
        ("Issues", Screen::Issues),
        ("People & collaboration", Screen::People),
        ("Quality & security", Screen::Quality),
        ("AI workspace", Screen::AiWorkspace),
        ("Activity & reports", Screen::Activity),
        ("Automations", Screen::Automations),
        ("Local workspace", Screen::LocalWorkspace),
        ("Settings", Screen::Settings),
        ("Application health", Screen::Health),
    ]
}
