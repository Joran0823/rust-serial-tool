//! 设计稿（docs/UI-Desing.svg）对应的浅色主题与常用控件配色。

use eframe::egui;

/// 窗口/侧栏背景（画板底色）
pub const BG: egui::Color32 = egui::Color32::from_rgb(0xCC, 0xCC, 0xCC);
/// 面板底色（左栏分区、右侧三栏）
pub const PANEL: egui::Color32 = egui::Color32::from_rgb(0xE0, 0xE0, 0xE0);
/// 状态栏底色
pub const STATUS_BG: egui::Color32 = egui::Color32::from_rgb(0xD6, 0xD6, 0xD6);
/// 次要按钮底色
pub const BTN_GRAY: egui::Color32 = egui::Color32::from_rgb(0xE7, 0xE7, 0xE7);
/// 主按钮 / 强调色
pub const BLUE: egui::Color32 = egui::Color32::from_rgb(0x2A, 0x82, 0xE4);
/// 勾选 / 选中蓝
pub const BLUE_CHECK: egui::Color32 = egui::Color32::from_rgb(0x18, 0x90, 0xFF);
/// 普通正文
pub const TEXT: egui::Color32 = egui::Color32::from_rgb(0x33, 0x33, 0x33);
/// 弱化文字（占位、说明）
pub const TEXT_SOFT: egui::Color32 = egui::Color32::from_rgb(0x7A, 0x7A, 0x7A);
/// 控件边框（rgba(0,0,0,0.15)）
pub const BORDER: egui::Stroke = egui::Stroke {
    width: 1.0,
    color: egui::Color32::from_black_alpha(38),
};
/// 错误红
pub const ERROR_RED: egui::Color32 = egui::Color32::from_rgb(0xDC, 0x3C, 0x3C);
/// 成功绿
pub const OK_GREEN: egui::Color32 = egui::Color32::from_rgb(0x3C, 0xA0, 0x3C);

/// 应用到全局样式。
pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::light();
    v.panel_fill = PANEL;
    v.window_fill = egui::Color32::from_rgb(0xF5, 0xF5, 0xF5);
    v.extreme_bg_color = BG;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = BLUE_CHECK;
    // egui 渲染选中文字时用 selection.stroke.color 着色、selection.bg_fill 画高亮底，
    // 两者不能相同（此前均为蓝色导致选中文字不可见）。
    v.selection.stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    v.hyperlink_color = BLUE;

    v.widgets.noninteractive.bg_fill = egui::Color32::WHITE;
    v.widgets.noninteractive.weak_bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = BORDER;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT);

    v.widgets.inactive.bg_fill = egui::Color32::WHITE;
    // 下拉框/输入类控件底色（设计稿为白色）
    v.widgets.inactive.weak_bg_fill = egui::Color32::WHITE;
    v.widgets.inactive.bg_stroke = BORDER;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT);

    v.widgets.hovered.bg_fill = egui::Color32::WHITE;
    v.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(0xF6, 0xF6, 0xF6);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, BLUE_CHECK);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, TEXT);

    v.widgets.active.bg_fill = egui::Color32::from_rgb(0xDD, 0xEA, 0xF8);
    v.widgets.active.weak_bg_fill = egui::Color32::from_rgb(0xDD, 0xEA, 0xF8);
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0, BLUE);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, TEXT);

    v.widgets.open.bg_fill = egui::Color32::WHITE;
    v.widgets.open.weak_bg_fill = egui::Color32::WHITE;
    v.widgets.open.bg_stroke = BORDER;
    v.widgets.open.fg_stroke = egui::Stroke::new(1.0, TEXT);

    // 应用默认主题可能是 Dark，这里强制 Light 并同时写入两套样式，
    // 保证下拉框等控件使用浅色主题。
    ctx.set_theme(egui::Theme::Light);
    ctx.set_visuals_of(egui::Theme::Light, v.clone());
    ctx.set_visuals_of(egui::Theme::Dark, v);
}

/// 主按钮控件（配合 `ui.add_sized` 使用）。
pub fn primary_widget(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(egui::Color32::WHITE).strong())
        .fill(BLUE)
        .stroke(egui::Stroke::new(0.0, BLUE))
        .corner_radius(4)
}

/// 次要按钮控件（配合 `ui.add_sized` 使用）。
pub fn secondary_widget(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(TEXT))
        .fill(BTN_GRAY)
        .stroke(BORDER)
        .corner_radius(4)
}

/// 白底蓝描边按钮控件（配合 `ui.add_sized` 使用）。
pub fn outline_widget(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(BLUE))
        .fill(egui::Color32::WHITE)
        .stroke(egui::Stroke::new(1.0, BLUE))
        .corner_radius(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_text_color_differs_from_highlight() {
        // egui 用 selection.stroke.color 给选中文字着色、selection.bg_fill 画高亮底；
        // 两者相同会导致选中文字不可见（此前 bug：均为 BLUE_CHECK）。
        let ctx = egui::Context::default();
        apply(&ctx);
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            let visuals = &ctx.style_of(theme).visuals;
            assert_ne!(
                visuals.selection.bg_fill,
                visuals.selection.stroke.color,
                "选中文字颜色不能与高亮底色相同（theme={theme:?}）"
            );
        }
    }
}
