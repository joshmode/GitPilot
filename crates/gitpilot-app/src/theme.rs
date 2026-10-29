use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
}

#[derive(Debug, Clone)]
pub struct Tokens {
    pub dark: bool,
    pub bg: Color32,
    pub sidebar: Color32,
    pub surface: Color32,
    pub raised: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    pub info: Color32,
    pub selection: Color32,
}

impl Tokens {
    pub fn dark() -> Self {
        Self {
            dark: true,
            bg: Color32::from_rgb(20, 22, 26),
            sidebar: Color32::from_rgb(16, 18, 21),
            surface: Color32::from_rgb(27, 30, 35),
            raised: Color32::from_rgb(33, 37, 43),
            border: Color32::from_rgb(45, 50, 58),
            border_strong: Color32::from_rgb(64, 71, 81),
            text: Color32::from_rgb(231, 234, 239),
            text_muted: Color32::from_rgb(153, 162, 174),
            text_faint: Color32::from_rgb(103, 112, 124),
            accent: Color32::from_rgb(124, 138, 255),
            accent_soft: Color32::from_rgba_premultiplied(124, 138, 255, 28),
            success: Color32::from_rgb(87, 195, 138),
            warning: Color32::from_rgb(228, 169, 77),
            danger: Color32::from_rgb(232, 99, 109),
            info: Color32::from_rgb(109, 172, 230),
            selection: Color32::from_rgba_premultiplied(124, 138, 255, 40),
        }
    }

    pub fn light() -> Self {
        Self {
            dark: false,
            bg: Color32::from_rgb(247, 248, 250),
            sidebar: Color32::from_rgb(240, 242, 245),
            surface: Color32::from_rgb(255, 255, 255),
            raised: Color32::from_rgb(250, 251, 252),
            border: Color32::from_rgb(218, 222, 229),
            border_strong: Color32::from_rgb(190, 196, 206),
            text: Color32::from_rgb(31, 36, 43),
            text_muted: Color32::from_rgb(96, 105, 119),
            text_faint: Color32::from_rgb(139, 148, 162),
            accent: Color32::from_rgb(78, 91, 214),
            accent_soft: Color32::from_rgba_premultiplied(78, 91, 214, 22),
            success: Color32::from_rgb(34, 137, 82),
            warning: Color32::from_rgb(169, 106, 21),
            danger: Color32::from_rgb(194, 48, 62),
            info: Color32::from_rgb(38, 112, 177),
            selection: Color32::from_rgba_premultiplied(78, 91, 214, 30),
        }
    }

    pub fn severity(&self, severity: gitpilot_core::Severity) -> Color32 {
        match severity {
            gitpilot_core::Severity::Info => self.info,
            gitpilot_core::Severity::Low => self.text_muted,
            gitpilot_core::Severity::Medium => self.warning,
            gitpilot_core::Severity::High | gitpilot_core::Severity::Critical => self.danger,
        }
    }

    pub fn check(&self, state: gitpilot_core::CheckState) -> Color32 {
        match state {
            gitpilot_core::CheckState::Success => self.success,
            gitpilot_core::CheckState::Failure => self.danger,
            gitpilot_core::CheckState::Pending => self.warning,
            gitpilot_core::CheckState::Neutral => self.info,
            gitpilot_core::CheckState::Missing => self.text_faint,
        }
    }
}

pub fn install(ctx: &egui::Context, theme: Theme, scale: f32, density: &str) {
    let tokens = match theme {
        Theme::Dark => Tokens::dark(),
        Theme::Light => Tokens::light(),
    };
    let mut style = (*ctx.style()).clone();
    style.visuals = match theme {
        Theme::Dark => Visuals::dark(),
        Theme::Light => Visuals::light(),
    };
    style.visuals.override_text_color = Some(tokens.text);
    style.visuals.panel_fill = tokens.bg;
    style.visuals.window_fill = tokens.surface;
    style.visuals.extreme_bg_color = tokens.sidebar;
    style.visuals.faint_bg_color = tokens.raised;
    style.visuals.widgets.noninteractive.bg_fill = tokens.surface;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, tokens.border);
    style.visuals.widgets.inactive.bg_fill = tokens.raised;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, tokens.border);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, tokens.text_muted);
    style.visuals.widgets.hovered.bg_fill = tokens.raised;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, tokens.border_strong);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, tokens.text);
    style.visuals.widgets.active.bg_fill = tokens.selection;
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, tokens.accent);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, tokens.text);
    style.visuals.selection.bg_fill = tokens.selection;
    style.visuals.selection.stroke = Stroke::new(1.0_f32, tokens.accent);
    style.visuals.window_stroke = Stroke::new(1.0_f32, tokens.border);
    style.visuals.window_corner_radius = CornerRadius::same(10);
    style.visuals.menu_corner_radius = CornerRadius::same(8);
    let size = 13.5 * scale;
    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(size * 1.55, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(size, FontFamily::Proportional)),
        (
            TextStyle::Monospace,
            FontId::new(size * 0.92, FontFamily::Monospace),
        ),
        (
            TextStyle::Button,
            FontId::new(size, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(size * 0.82, FontFamily::Proportional),
        ),
    ]
    .into();
    let gap = if density == "compact" { 4.0 } else { 7.0 };
    style.spacing.item_spacing = egui::vec2(8.0, gap);
    style.spacing.button_padding = egui::vec2(9.0, if density == "compact" { 4.0 } else { 6.0 });
    style.spacing.interact_size.y = if density == "compact" { 24.0 } else { 30.0 };
    style.spacing.scroll.bar_width = 7.0;
    ctx.set_style(style);
}
