use crate::nav::Screen;
use crate::state::{UiEvent, UiState};
use crate::theme::Tokens;
use gitpilot_core::AppService;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

pub mod automations;
pub mod commits;
pub mod issues;
pub mod local;
pub mod overview;
pub mod people;
pub mod projects;
pub mod pull_requests;
pub mod quality;
pub mod repositories;
pub mod settings;
pub mod workspace;

pub fn show(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    // The global search is a filter on list screens and a palette trigger in
    // the top bar; screens consume it without performing ad-hoc SQL.
    match state.screen {
        Screen::Overview => overview::show(ui, state, tokens, service, runtime, tx),
        Screen::Projects => projects::show(ui, state, tokens, service, runtime, tx),
        Screen::Repositories => repositories::show(ui, state, tokens, service, runtime, tx),
        Screen::PullRequests => pull_requests::show(ui, ctx, state, tokens, service, runtime, tx),
        Screen::Commits => commits::show(ui, state, tokens, service, runtime, tx),
        Screen::Issues => issues::show(ui, state, tokens, service, runtime, tx),
        Screen::People => people::show(ui, state, tokens, service, runtime, tx),
        Screen::Quality => quality::show(ui, state, tokens, service, runtime, tx),
        Screen::AiWorkspace => workspace::show(ui, state, tokens, service, runtime, tx),
        Screen::Activity => projects::activity(ui, state, tokens, service, runtime, tx),
        Screen::Automations => automations::show(ui, state, tokens, service, runtime, tx),
        Screen::Settings => settings::show(ui, state, tokens, service, runtime, tx),
        Screen::Onboarding => projects::onboarding(ui, state, tokens, service, runtime, tx),
        Screen::Health => settings::health(ui, state, tokens, service, runtime, tx),
        Screen::LocalWorkspace => local::show(ui, state, tokens, service, runtime, tx),
    }
}
