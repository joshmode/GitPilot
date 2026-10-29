use crate::theme::Tokens;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, RichText, Sense,
    Stroke, Ui, Vec2,
};
use gitpilot_core::{CheckState, DataOrigin, EvidenceKind, MergeableState, ReviewState, Severity};

pub fn panel(tokens: &Tokens) -> Frame {
    Frame::new()
        .fill(tokens.surface)
        .stroke(Stroke::new(1.0_f32, tokens.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(14))
}

pub fn inset(tokens: &Tokens) -> Frame {
    Frame::new()
        .fill(tokens.raised)
        .stroke(Stroke::new(1.0_f32, tokens.border))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::same(10))
}

pub fn section_header(ui: &mut Ui, tokens: &Tokens, title: &str, subtitle: Option<&str>) {
    ui.label(RichText::new(title).size(16.0).strong().color(tokens.text));
    if let Some(subtitle) = subtitle {
        ui.label(RichText::new(subtitle).size(12.0).color(tokens.text_muted));
    }
    ui.add_space(8.0);
}

pub fn page_header(
    ui: &mut Ui,
    tokens: &Tokens,
    title: &str,
    subtitle: &str,
    right: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(title).size(24.0).strong().color(tokens.text));
            ui.label(RichText::new(subtitle).size(12.5).color(tokens.text_muted));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), right);
    });
    ui.add_space(12.0);
}

pub fn badge(ui: &mut Ui, _tokens: &Tokens, text: &str, color: Color32) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), FontId::proportional(10.5), color);
    let size = Vec2::new(galley.size().x + 13.0, 19.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(9),
        Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 24),
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
}

pub fn status_dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.2, color);
}

pub fn check_badge(ui: &mut Ui, tokens: &Tokens, state: CheckState) {
    let (label, color) = match state {
        CheckState::Success => ("checks pass", tokens.success),
        CheckState::Failure => ("checks fail", tokens.danger),
        CheckState::Pending => ("checks pending", tokens.warning),
        CheckState::Neutral => ("checks neutral", tokens.info),
        CheckState::Missing => ("checks missing", tokens.text_faint),
    };
    badge(ui, tokens, label, color);
}

pub fn review_badge(ui: &mut Ui, tokens: &Tokens, state: ReviewState) {
    let (label, color) = match state {
        ReviewState::Approved => ("approved", tokens.success),
        ReviewState::ChangesRequested => ("changes requested", tokens.danger),
        ReviewState::Pending => ("review pending", tokens.warning),
        ReviewState::None => ("no review", tokens.text_muted),
    };
    badge(ui, tokens, label, color);
}

pub fn merge_badge(ui: &mut Ui, tokens: &Tokens, state: MergeableState) {
    let (label, color) = match state {
        MergeableState::Mergeable => ("mergeable", tokens.success),
        MergeableState::Conflicting => ("conflict", tokens.danger),
        MergeableState::Blocked => ("blocked", tokens.warning),
        MergeableState::Unknown => ("mergeability unknown", tokens.text_muted),
    };
    badge(ui, tokens, label, color);
}

pub fn evidence_chip(ui: &mut Ui, tokens: &Tokens, kind: EvidenceKind, label: Option<&str>) {
    let (default_label, color) = match kind {
        EvidenceKind::Observed => ("observed", tokens.success),
        EvidenceKind::Derived => ("derived", tokens.info),
        EvidenceKind::Inference => ("inference", tokens.warning),
        EvidenceKind::Recommendation => ("recommendation", tokens.accent),
        EvidenceKind::Unavailable => ("unavailable", tokens.text_faint),
    };
    badge(ui, tokens, label.unwrap_or(default_label), color);
}

pub fn severity_badge(ui: &mut Ui, tokens: &Tokens, severity: Severity) {
    badge(ui, tokens, severity.label(), tokens.severity(severity));
}

pub fn origin_badge(ui: &mut Ui, tokens: &Tokens, origin: DataOrigin) {
    match origin {
        DataOrigin::Demo => badge(ui, tokens, "DEMO DATA", tokens.warning),
        DataOrigin::GitHub => badge(ui, tokens, "LIVE GITHUB", tokens.success),
        DataOrigin::LocalGit => badge(ui, tokens, "LOCAL GIT", tokens.info),
    }
}

pub fn metric_card(
    ui: &mut Ui,
    tokens: &Tokens,
    label: &str,
    value: &str,
    hint: &str,
    color: Color32,
) {
    panel(tokens).show(ui, |ui| {
        ui.set_min_width(100.0);
        ui.horizontal(|ui| {
            status_dot(ui, color);
            ui.label(RichText::new(label).size(11.5).color(tokens.text_muted));
        });
        ui.label(RichText::new(value).size(25.0).strong().color(tokens.text));
        ui.label(RichText::new(hint).size(10.5).color(tokens.text_faint));
    });
}

pub fn empty_state(
    ui: &mut Ui,
    tokens: &Tokens,
    title: &str,
    message: &str,
    action: Option<(&str, &mut bool)>,
) {
    panel(tokens).show(ui, |ui| {
        ui.set_min_height(130.0);
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            ui.add_space(20.0);
            ui.label(RichText::new("◇").size(28.0).color(tokens.text_faint));
            ui.label(RichText::new(title).strong().color(tokens.text));
            ui.label(RichText::new(message).size(12.0).color(tokens.text_muted));
            if let Some((label, clicked)) = action {
                ui.add_space(8.0);
                if ui.button(label).clicked() {
                    *clicked = true;
                }
            }
        });
    });
}

pub fn error_banner(ui: &mut Ui, tokens: &Tokens, message: &str, retry: &mut bool) {
    Frame::new()
        .fill(Color32::from_rgba_premultiplied(
            tokens.danger.r(),
            tokens.danger.g(),
            tokens.danger.b(),
            20,
        ))
        .stroke(Stroke::new(1.0_f32, tokens.danger))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("!").strong().color(tokens.danger));
                ui.label(RichText::new(message).size(12.0).color(tokens.text));
                if ui.small_button("Retry").clicked() {
                    *retry = true;
                }
            });
        });
    ui.add_space(8.0);
}

pub fn loading(ui: &mut Ui, tokens: &Tokens, label: &str) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(RichText::new(label).size(12.0).color(tokens.text_muted));
    });
}

pub fn kv(ui: &mut Ui, tokens: &Tokens, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).size(11.0_f32).color(tokens.text_faint));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value).size(11.5).color(tokens.text_muted));
        });
    });
}

pub fn divider(ui: &mut Ui, tokens: &Tokens) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0_f32), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, tokens.border);
    ui.add_space(6.0);
}

pub fn tag(ui: &mut Ui, tokens: &Tokens, text: &str) {
    badge(ui, tokens, text, tokens.text_muted);
}

pub fn short_time(time: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let diff = now - time;
    if diff.num_minutes() < 1 {
        "just now".into()
    } else if diff.num_minutes() < 60 {
        format!("{}m ago", diff.num_minutes())
    } else if diff.num_hours() < 24 {
        format!("{}h ago", diff.num_hours())
    } else if diff.num_days() < 14 {
        format!("{}d ago", diff.num_days())
    } else {
        time.format("%b %d").to_string()
    }
}

pub fn modal_frame(tokens: &Tokens) -> Frame {
    Frame::new()
        .fill(tokens.surface)
        .stroke(Stroke::new(1.0_f32, tokens.border_strong))
        .corner_radius(CornerRadius::same(10))
        .shadow(egui::epaint::Shadow {
            offset: [0, 12],
            blur: 30,
            spread: 0,
            color: Color32::from_black_alpha(120),
        })
        .inner_margin(Margin::same(18))
}

pub fn draw_logo(ui: &mut Ui, tokens: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
    let painter = ui.painter();
    painter.circle_filled(rect.center(), 13.0, tokens.accent);
    painter.circle_stroke(
        rect.center(),
        6.0,
        Stroke::new(
            2.0_f32,
            if tokens.dark {
                tokens.bg
            } else {
                Color32::WHITE
            },
        ),
    );
    painter.line_segment(
        [
            egui::pos2(rect.center().x, rect.center().y + 5.0),
            egui::pos2(rect.center().x, rect.center().y + 11.0_f32),
        ],
        Stroke::new(
            2.0_f32,
            if tokens.dark {
                tokens.bg
            } else {
                Color32::WHITE
            },
        ),
    );
}

pub fn badge_clickable(ui: &mut Ui, tokens: &Tokens, label: &str, shortcut: &str) -> bool {
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        FontId::proportional(11.5),
        tokens.text_muted,
    );
    let shortcut_galley = ui.painter().layout_no_wrap(
        shortcut.to_string(),
        FontId::monospace(9.5),
        tokens.text_faint,
    );
    let width = galley.size().x + shortcut_galley.size().x + 36.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width.max(230.0), 26.0), Sense::click());
    ui.painter().rect_filled(
        rect,
        6.0,
        if response.hovered() {
            tokens.raised
        } else {
            tokens.surface
        },
    );
    ui.painter().rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0_f32, tokens.border),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        egui::pos2(rect.min.x + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(11.5),
        tokens.text_muted,
    );
    ui.painter().text(
        egui::pos2(rect.max.x - 10.0, rect.center().y),
        Align2::RIGHT_CENTER,
        shortcut,
        FontId::monospace(9.5),
        tokens.text_faint,
    );
    response.clicked()
}

pub fn count_badge(ui: &mut Ui, tokens: &Tokens, icon: &str, count: i64) -> bool {
    let label = if count > 0 {
        format!("{icon} {count}")
    } else {
        icon.to_string()
    };
    ui.button(RichText::new(label).size(12.0).color(tokens.text_muted))
        .clicked()
}

pub fn centered_modal<R>(
    ctx: &egui::Context,
    tokens: &Tokens,
    width: f32,
    content: impl FnOnce(&mut Ui) -> R,
) {
    egui::Area::new(egui::Id::new("modal_backdrop"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let screen = ctx.screen_rect();
            ui.painter().rect_filled(
                screen,
                CornerRadius::ZERO,
                Color32::from_black_alpha(if tokens.dark { 150 } else { 80 }),
            );
        });
    egui::Window::new(format!("modal_{width}"))
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([width, 0.0])
        .frame(modal_frame(tokens))
        .show(ctx, |ui| content(ui));
}
