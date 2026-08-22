//! 通用控件：统一样式下拉框（“整块发送”样式）、悬停光标等。

use crate::ui::theme;
use eframe::egui;

/// 统一样式下拉框：白底描边按钮 + 右侧绘制箭头 + Popup 菜单 + 悬停手型光标。
///
/// `text` 为当前选中文本；`options` 在弹出菜单中渲染选项（如 `selectable_value`）。
pub fn combo(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    enabled: bool,
    text: impl Into<String>,
    tip: &str,
    options: impl FnOnce(&mut egui::Ui),
) {
    let text: String = text.into();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let resp = if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tip)
    } else {
        resp.on_hover_text(tip)
    };

    if ui.is_rect_visible(resp.rect) {
        let fill = if enabled {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_gray(235)
        };
        // 背景 + 边框
        ui.painter()
            .rect(rect, 4.0, fill, theme::BORDER, egui::StrokeKind::Inside);

        // 选中文本：左对齐、垂直居中，预留右侧箭头空间（超长自动裁剪）
        let text_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 8.0, rect.top()),
            egui::pos2(rect.right() - 26.0, rect.bottom()),
        );
        if text_rect.width() > 4.0 {
            let painter = ui.painter().with_clip_rect(text_rect);
            painter.text(
                egui::pos2(text_rect.left(), text_rect.center().y),
                egui::Align2::LEFT_CENTER,
                text,
                egui::FontId::proportional(14.0),
                theme::TEXT,
            );
        }

        // 右侧绘制下拉箭头（不依赖字体字形）
        let tri = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 13.0, rect.center().y),
            egui::vec2(7.0, 4.5),
        );
        ui.painter().add(egui::Shape::convex_polygon(
            vec![tri.left_top(), tri.right_top(), tri.center_bottom()],
            theme::TEXT,
            egui::Stroke::NONE,
        ));
    }

    if enabled {
        egui::Popup::menu(&resp).show(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            options(ui);
        });
    }
}
