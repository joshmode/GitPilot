use eframe::egui::{self, Align, Layout, RichText, Sense, Stroke, Ui, Vec2};

use crate::{components, theme::Tokens};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Screen {
    Overview,
    Projects,
    Repositories,
    PullRequests,
    Commits,
    Issues,
    People,
    Quality,
    AiWorkspace,
    Activity,
    Automations,
    Settings,
    Onboarding,
    Health,
    LocalWorkspace,
}

impl Screen {
    pub fn label(self) -> &'static str {
        match self {
            Screen::Overview => "Overview",
            Screen::Projects => "Projects",
            Screen::Repositories => "Repositories",
            Screen::PullRequests => "Pull requests",
            Screen::Commits => "Commits & changes",
            Screen::Issues => "Issues",
            Screen::People => "People",
            Screen::Quality => "Quality & security",
            Screen::AiWorkspace => "AI workspace",
            Screen::Activity => "Activity & reports",
            Screen::Automations => "Automations",
            Screen::Settings => "Settings",
            Screen::Onboarding => "Setup",
            Screen::Health => "Application health",
            Screen::LocalWorkspace => "Local workspace",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Screen::Overview => "◫",
            Screen::Projects => "▦",
            Screen::Repositories => "⌂",
            Screen::PullRequests => "⑂",
            Screen::Commits => "◎",
            Screen::Issues => "◇",
            Screen::People => "◉",
            Screen::Quality => "△",
            Screen::AiWorkspace => "✦",
            Screen::Activity => "≡",
            Screen::Automations => "↻",
            Screen::Settings => "⚙",
            Screen::Onboarding => "→",
            Screen::Health => "♡",
            Screen::LocalWorkspace => "⌘",
        }
    }
}

pub fn sidebar(
    ui: &mut Ui,
    tokens: &Tokens,
    active: Screen,
    collapsed: bool,
    unread: i64,
    repo_count: i64,
) -> Option<Screen> {
    let mut changed = None;
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        components::draw_logo(ui, tokens);
        if !collapsed {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("GitPilot")
                        .size(15.0)
                        .strong()
                        .color(tokens.text),
                );
                ui.label(
                    RichText::new("Project command centre")
                        .size(9.5)
                        .color(tokens.text_faint),
                );
            });
        }
    });
    ui.add_space(14.0);
    let groups: &[(&str, &[Screen])] = &[
        (
            "WORK",
            &[
                Screen::Overview,
                Screen::Projects,
                Screen::Repositories,
                Screen::PullRequests,
                Screen::Commits,
                Screen::Issues,
            ],
        ),
        (
            "INTELLIGENCE",
            &[
                Screen::People,
                Screen::Quality,
                Screen::AiWorkspace,
                Screen::Activity,
            ],
        ),
        (
            "CONTROL",
            &[
                Screen::Automations,
                Screen::LocalWorkspace,
                Screen::Settings,
            ],
        ),
    ];
    for (group, items) in groups {
        if !collapsed {
            ui.add_space(7.0);
            ui.label(
                RichText::new(*group)
                    .size(9.0)
                    .strong()
                    .color(tokens.text_faint),
            );
        }
        for item in *items {
            if nav_item(ui, tokens, *item, active, collapsed, unread, repo_count) {
                changed = Some(*item);
            }
        }
    }
    ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
        ui.add_space(8.0);
        if !collapsed {
            ui.horizontal(|ui| {
                components::status_dot(ui, tokens.success);
                ui.label(
                    RichText::new("Local database ready")
                        .size(10.5)
                        .color(tokens.text_faint),
                );
            });
        }
        if nav_item(
            ui,
            tokens,
            Screen::Health,
            active,
            collapsed,
            unread,
            repo_count,
        ) {
            changed = Some(Screen::Health);
        }
        if nav_item(
            ui,
            tokens,
            Screen::Onboarding,
            active,
            collapsed,
            unread,
            repo_count,
        ) {
            changed = Some(Screen::Onboarding);
        }
    });
    changed
}

fn nav_item(
    ui: &mut Ui,
    tokens: &Tokens,
    item: Screen,
    active: Screen,
    collapsed: bool,
    unread: i64,
    repo_count: i64,
) -> bool {
    let selected = item == active;
    let height = 29.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 5.0, tokens.accent_soft);
        ui.painter().rect_filled(
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + 2.0, rect.max.y)),
            1.0_f32,
            tokens.accent,
        );
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 5.0, tokens.raised);
    }
    let color = if selected {
        tokens.text
    } else {
        tokens.text_muted
    };
    let icon_x = if collapsed {
        rect.center().x
    } else {
        rect.min.x + 18.0
    };
    ui.painter().text(
        egui::pos2(icon_x, rect.center().y),
        egui::Align2::CENTER_CENTER,
        item.icon(),
        egui::FontId::proportional(13.0),
        if selected { tokens.accent } else { color },
    );
    if !collapsed {
        ui.painter().text(
            egui::pos2(rect.min.x + 32.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            item.label(),
            egui::FontId::proportional(11.8),
            color,
        );
        if item == Screen::PullRequests {
            ui.painter().text(
                egui::pos2(rect.max.x - 10.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                unread.to_string(),
                egui::FontId::proportional(9.5),
                tokens.text_faint,
            );
        }
        if item == Screen::Repositories {
            ui.painter().text(
                egui::pos2(rect.max.x - 10.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                repo_count.to_string(),
                egui::FontId::proportional(9.5),
                tokens.text_faint,
            );
        }
    }
    response.on_hover_text(item.label()).clicked()
}

pub fn top_bar(
    ui: &mut Ui,
    tokens: &Tokens,
    search: &mut String,
    model_status: &str,
    repo_scope: &str,
    background_jobs: usize,
    unread: i64,
) -> TopBarActions {
    let mut actions = TopBarActions::default();
    ui.horizontal(|ui| {
        ui.add_space(3.0);
        if components::badge_clickable(ui, tokens, "⌕  Search anything", "⌘K") {
            actions.open_search = true;
        }
        if !search.trim().is_empty() {
            if ui.small_button("×").on_hover_text("Clear search").clicked() {
                search.clear();
            }
            ui.label(
                RichText::new(format!("“{}”", search.trim()))
                    .size(10.0)
                    .color(tokens.text_faint),
            );
        }
        ui.add_space(8.0);
        if ui
            .add(egui::Button::new(
                RichText::new(format!("{repo_scope}  ▾")).size(11.0),
            ))
            .on_hover_text("Switch repository scope")
            .clicked()
        {
            actions.open_scope = true;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .button(RichText::new("◉").size(15.0))
                .on_hover_text("Account & data source")
                .clicked()
            {
                actions.open_account = true;
            }
            if components::count_badge(ui, tokens, "♢", unread) {
                actions.open_notifications = true;
            }
            let job_label = if background_jobs > 0 {
                format!("⟳ {background_jobs}")
            } else {
                "⟳ idle".into()
            };
            if ui
                .button(RichText::new(job_label).size(10.5).color(tokens.text_muted))
                .on_hover_text("Background task queue")
                .clicked()
            {
                actions.open_jobs = true;
            }
            components::status_dot(
                ui,
                if model_status.contains("not configured") {
                    tokens.text_faint
                } else {
                    tokens.success
                },
            );
            ui.label(
                RichText::new(model_status)
                    .size(10.5)
                    .color(tokens.text_muted),
            );
        });
    });
    actions
}

#[derive(Default)]
pub struct TopBarActions {
    pub open_search: bool,
    pub open_scope: bool,
    pub open_account: bool,
    pub open_notifications: bool,
    pub open_jobs: bool,
}

pub fn shortcut_reference(ui: &mut Ui, tokens: &Tokens) {
    let rows = [
        ("⌘ K", "Open command palette"),
        ("⌘ F", "Search current workspace"),
        ("⌘ ⇧ P", "Pull requests"),
        ("⌘ ⇧ R", "Repositories"),
        ("⌘ ,", "Settings"),
        ("⌘ R", "Refresh current data"),
        ("Esc", "Close modal / clear search"),
        ("↑ ↓", "Move through results"),
        ("Enter", "Open selection"),
        ("⌘ B", "Collapse / expand sidebar"),
    ];
    for (keys, description) in rows {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(70.0, 21.0_f32), Sense::hover());
            ui.painter().rect_filled(rect, 4.0, tokens.raised);
            ui.painter().rect_stroke(
                rect,
                4.0,
                Stroke::new(1.0_f32, tokens.border),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                keys,
                egui::FontId::monospace(10.5),
                tokens.text_muted,
            );
            ui.label(
                RichText::new(description)
                    .size(11.5)
                    .color(tokens.text_muted),
            );
        });
    }
}
