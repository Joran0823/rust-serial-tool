//! 通用控件：统一样式下拉框（“整块发送”样式）、悬停光标等。

use crate::ui::theme;
use eframe::egui;

/// 给文本框追加右键菜单：全选、复制（始终显示）+ 粘贴（仅可读写时显示）。
///
/// egui 的剪贴板命令（`ViewportCommand::RequestCopy` / `RequestPaste`）作用于
/// 当前持有焦点的文本框，因此右键打开菜单时会先让该文本框获得焦点；点击菜单项
/// 会让文本框失焦，所以执行动作后再次请求焦点（egui 只在聚焦时绘制选区并处理
/// 剪贴板事件）。egui 的右键会把光标移到点击处并清空选区，因此右键时用上一帧
/// 的选区快照恢复，保证选中文本后右键不清空。`text` 为文本框当前内容，用于
/// 计算全选的字符范围。`strings` 为当前语言的界面文案。
pub fn text_edit_context_menu(
    ui: &mut egui::Ui,
    response: &egui::Response,
    editable: bool,
    text: &str,
    strings: &crate::i18n::Strings,
) {
    // 上一帧的选区快照（用于恢复右键清掉的选区）
    let stash_id = egui::Id::new(("text_edit_sel_stash", response.id));
    let prev_state =
        ui.data(|d| d.get_temp::<egui::widgets::text_edit::TextEditState>(stash_id));

    if response.secondary_clicked() {
        // egui 的右键会在文本框内放置光标并清空选区，恢复右键前的选区状态
        if let Some(prev) = prev_state {
            prev.store(ui.ctx(), response.id);
        }
        ui.memory_mut(|mem| mem.request_focus(response.id));
    }

    // 记录本帧的选区状态，供下一次右键恢复
    if let Some(state) = egui::widgets::text_edit::TextEditState::load(ui.ctx(), response.id) {
        ui.data_mut(|d| d.insert_temp(stash_id, state));
    }

    response.context_menu(|ui| {
        if ui.selectable_label(false, strings.select_all).clicked() {
            select_all_text(ui.ctx(), response.id, text);
            ui.memory_mut(|mem| mem.request_focus(response.id));
            ui.close();
        }
        if ui.selectable_label(false, strings.copy).clicked() {
            ui.memory_mut(|mem| mem.request_focus(response.id));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestCopy);
            ui.close();
        }
        if editable && ui.selectable_label(false, strings.paste).clicked() {
            ui.memory_mut(|mem| mem.request_focus(response.id));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            ui.close();
        }
    });
}

/// 将某个文本框的选区设置为全选（直接写入其持久化状态，与 `TextEdit` 共用同一状态）。
fn select_all_text(ctx: &egui::Context, id: egui::Id, text: &str) {
    let mut state =
        egui::widgets::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    let range = egui::text::CCursorRange::two(
        egui::text::CCursor::new(0),
        egui::text::CCursor::new(text.chars().count()),
    );
    state.cursor.set_char_range(Some(range));
    state.store(ctx, id);
}

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
    // 中英文文案宽度差异大：固定宽度会裁掉英文文本（如 “Line by line”）。
    // 按当前文字实际宽度自适应加宽，预留 8px 左边距 + 26px 箭头区 + 4px 右边距；
    // 宽度传 available_width() 的控件（如端口下拉框）不受影响。
    let text_width = ui.ctx().fonts_mut(|f| {
        f.layout_no_wrap(
            text.clone(),
            egui::FontId::proportional(14.0),
            egui::Color32::PLACEHOLDER,
        )
        .size()
        .x
    });
    let needed = text_width + 8.0 + 26.0 + 4.0;
    let width = width.max(needed.min(ui.available_width()));
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let resp = if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tip)
    } else {
        resp.on_hover_text(tip)
    };

    if ui.is_rect_visible(resp.rect) {
        let fill = if enabled {
            theme::input_bg()
        } else {
            egui::Color32::from_rgb(0x18, 0x1D, 0x24)
        };
        // 背景 + 边框
        ui.painter()
            .rect(rect, 4.0, fill, theme::border(), egui::StrokeKind::Inside);

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
                theme::text(),
            );
        }

        // 右侧绘制下拉箭头（不依赖字体字形）
        let tri = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 13.0, rect.center().y),
            egui::vec2(7.0, 4.5),
        );
        ui.painter().add(egui::Shape::convex_polygon(
            vec![tri.left_top(), tri.right_top(), tri.center_bottom()],
            theme::text(),
            egui::Stroke::NONE,
        ));
    }

    if enabled {
        egui::Popup::menu(&resp).show(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            // 弹出菜单至少与下拉框同宽，避免选项文字看起来被裁切
            ui.set_min_width(width);
            options(ui);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_all_state_survives_next_frame() {
        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty()); // 测试无需加载字体
        let mut text = String::from("hello");

        for frame in 0..3 {
            let _ = ctx.run_ui(Default::default(), |ui| {
                let out = egui::TextEdit::singleline(&mut text).show(ui);
                if frame == 1 {
                    // 模拟右键菜单点击“全选”
                    select_all_text(ui.ctx(), out.response.id, &text);
                }
                if frame == 2 {
                    // 下一帧重新渲染后，选区应仍是全选范围
                    let state =
                        egui::widgets::text_edit::TextEditState::load(ui.ctx(), out.response.id)
                            .unwrap_or_default();
                    let expected: std::ops::Range<egui::text::CharIndex> =
                        egui::text::CharIndex(0)..egui::text::CharIndex(text.chars().count());
                    assert_eq!(
                        state.cursor.char_range().map(|r| r.as_sorted_char_range()),
                        Some(expected)
                    );
                }
            });
        }
    }

}
