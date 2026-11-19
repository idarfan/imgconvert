//! 介面配色、字級與共用元件（卡片、分段按鈕、主按鈕、狀態標籤）。
//! 淺色／深色各一套，跟隨 Windows 系統設定切換。

use eframe::egui::{self, Color32, CornerRadius, FontId, Margin, Response, RichText, Stroke, TextStyle, Theme, Vec2};

const ACCENT_LIGHT: Color32 = Color32::from_rgb(37, 99, 235);
const ACCENT_DARK: Color32 = Color32::from_rgb(59, 130, 246);
pub const OK: Color32 = Color32::from_rgb(34, 160, 90);
pub const ERR: Color32 = Color32::from_rgb(220, 70, 70);
const RADIUS: u8 = 6;
const CARD_RADIUS: u8 = 10;

pub fn install(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::System);
    for theme in [Theme::Light, Theme::Dark] {
        ctx.style_mut_of(theme, |s| apply(s, theme == Theme::Dark));
    }
}

fn apply(s: &mut egui::Style, dark: bool) {
    s.text_styles = [
        (TextStyle::Small, FontId::proportional(12.0)),
        (TextStyle::Body, FontId::proportional(15.0)),
        (TextStyle::Button, FontId::proportional(15.0)),
        (TextStyle::Monospace, FontId::monospace(14.0)),
        (TextStyle::Heading, FontId::proportional(19.0)),
    ]
    .into();
    s.spacing.item_spacing = Vec2::new(8.0, 8.0);
    s.spacing.button_padding = Vec2::new(12.0, 5.0);
    s.spacing.interact_size.y = 28.0;

    let pick = |light: Color32, dark_c: Color32| if dark { dark_c } else { light };
    let v = &mut s.visuals;
    v.selection.bg_fill = pick(ACCENT_LIGHT, ACCENT_DARK);
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = pick(ACCENT_LIGHT, ACCENT_DARK);
    v.panel_fill = pick(Color32::from_rgb(244, 245, 248), Color32::from_rgb(24, 26, 30));
    v.faint_bg_color = pick(Color32::from_rgb(238, 240, 244), Color32::from_rgb(34, 37, 43));
    v.extreme_bg_color = pick(Color32::WHITE, Color32::from_rgb(31, 33, 38));
    v.window_corner_radius = CornerRadius::same(CARD_RADIUS);

    let base = pick(Color32::from_rgb(226, 229, 235), Color32::from_rgb(50, 54, 62));
    let hover = pick(Color32::from_rgb(213, 218, 227), Color32::from_rgb(64, 69, 79));
    let w = &mut v.widgets;
    w.inactive.weak_bg_fill = base;
    w.inactive.bg_fill = base;
    w.inactive.bg_stroke = Stroke::NONE;
    w.hovered.weak_bg_fill = hover;
    w.hovered.bg_fill = hover;
    for wv in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        wv.corner_radius = CornerRadius::same(RADIUS);
    }
}

pub fn accent(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode { ACCENT_DARK } else { ACCENT_LIGHT }
}

/// 白底圓角卡片（深色模式為略亮的灰）
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let v = ui.visuals();
    egui::Frame::NONE
        .fill(v.extreme_bg_color)
        .stroke(Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// 分段按鈕：取代一排圓點選項
pub fn segmented<T: PartialEq + Copy>(ui: &mut egui::Ui, value: &mut T, options: &[(T, &str)]) {
    egui::Frame::NONE
        .fill(ui.visuals().widgets.inactive.weak_bg_fill)
        .corner_radius(CornerRadius::same(RADIUS + 2))
        .inner_margin(Margin::same(2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for (val, label) in options {
                    ui.selectable_value(value, *val, *label);
                }
            });
        });
}

fn big_button<'a>(icon: Option<&egui::TextureHandle>, text: RichText) -> egui::Button<'a> {
    let btn = match icon {
        Some(tex) => egui::Button::image_and_text(crate::icons::img(tex, 22.0), text),
        None => egui::Button::new(text),
    };
    btn.min_size(Vec2::new(170.0, 40.0)).corner_radius(CornerRadius::same(8))
}

/// 強調色的主要動作按鈕
pub fn primary_button(ui: &mut egui::Ui, icon: Option<&egui::TextureHandle>, text: &str, enabled: bool) -> Response {
    let btn = big_button(icon, RichText::new(text).color(Color32::WHITE).strong().size(16.0)).fill(accent(ui));
    ui.add_enabled(enabled, btn)
}

/// 次要的大按鈕（與主按鈕同尺寸，例如「取消」）
pub fn secondary_button(ui: &mut egui::Ui, icon: Option<&egui::TextureHandle>, text: &str) -> Response {
    ui.add(big_button(icon, RichText::new(text).size(16.0)))
}

/// 區段標題（Windows 11 設定頁的 BodyStrong 小標）
pub fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).strong().size(14.0));
    ui.add_space(2.0);
}

/// Windows 11 設定頁風格的卡片列：左邊 3D 圖示，右邊標題、說明，下方放控制項
pub fn settings_card(
    ui: &mut egui::Ui,
    icon: &egui::TextureHandle,
    title: &str,
    desc: &str,
    add_control: impl FnOnce(&mut egui::Ui),
) {
    card(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.add(crate::icons::img(icon, 32.0));
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(title).strong());
                ui.add(egui::Label::new(RichText::new(desc).weak().small()).wrap());
                ui.add_space(6.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                add_control(ui);
            });
        });
    });
}

/// 大圖示磚塊（圖示在上、文字在下）；sense 用 click() 當按鈕，用 hover() 純展示
pub fn tile(ui: &mut egui::Ui, icon: &egui::TextureHandle, text: &str, width: f32, sense: egui::Sense) -> Response {
    let clickable = sense.senses_click();
    let fill = ui.visuals().extreme_bg_color;
    let stroke = Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 84.0), sense);
    let hovered = clickable && resp.hovered() && ui.is_enabled();
    let bg = if hovered { ui.visuals().widgets.hovered.weak_bg_fill } else { fill };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(CARD_RADIUS), bg);
    p.rect_stroke(rect, CornerRadius::same(CARD_RADIUS), stroke, egui::StrokeKind::Inside);
    let icon_rect = egui::Rect::from_center_size(rect.center() - Vec2::new(0.0, 12.0), Vec2::splat(40.0));
    crate::icons::img(icon, 40.0).paint_at(ui, icon_rect);
    let color = if ui.is_enabled() { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
    p.text(rect.center_bottom() - Vec2::new(0.0, 14.0), egui::Align2::CENTER_CENTER, text, FontId::proportional(14.0), color);
    if clickable { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// 帶 3D 圖示的一般按鈕
pub fn icon_button(ui: &mut egui::Ui, icon: &egui::TextureHandle, text: &str) -> Response {
    ui.add(egui::Button::image_and_text(crate::icons::img(icon, 18.0), text))
}

/// Windows 11 風格開關
pub fn toggle(ui: &mut egui::Ui, on: &mut bool, label: &str) -> Response {
    ui.horizontal(|ui| {
        let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(40.0, 20.0), egui::Sense::click());
        if resp.clicked() {
            *on = !*on;
            resp.mark_changed();
        }
        let t = ui.ctx().animate_bool_responsive(resp.id, *on);
        let off_fill = ui.visuals().widgets.inactive.bg_fill;
        let fill = if *on { accent(ui) } else { off_fill };
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(10), fill);
        let x = egui::lerp((rect.left() + 10.0)..=(rect.right() - 10.0), t);
        let knob = if *on { Color32::WHITE } else { ui.visuals().text_color() };
        p.circle_filled(egui::pos2(x, rect.center().y), 6.0, knob);
        let text = ui.label(label).interact(egui::Sense::click());
        if text.clicked() {
            *on = !*on;
        }
        resp | text
    })
    .inner
}

/// 圓角狀態標籤，例如「完成」「失敗」
pub fn chip(ui: &mut egui::Ui, text: &str, color: Color32) -> Response {
    egui::Frame::NONE
        .fill(color.gamma_multiply(0.15))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(Margin::symmetric(8, 1))
        .show(ui, |ui| ui.label(RichText::new(text).color(color).small()))
        .inner
}
