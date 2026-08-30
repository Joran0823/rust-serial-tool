//! 文件发送面板：选择文件、发送文件、进度条。
//! 布局对应 docs/UI-Desing.svg（2026-08-01 版）的文件发送区。

use crate::app::SerialApp;
use crate::config::FileSendMode;
use crate::serial::Command;
use crate::ui::theme;
use crate::ui::widgets;
use eframe::egui;

impl SerialApp {
    /// 文件发送面板。
    pub fn file_panel(&mut self, ui: &mut egui::Ui) {
        let s = self.t();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_sized([102.0, 33.0], theme::primary_widget(s.send_file))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.send_file_tip)
                .clicked()
            {
                self.start_file_send();
            }
            // 统一样式下拉框：宽度与发送文件按钮一致（102px），高度相同（33px）
            widgets::combo(
                ui,
                102.0,
                33.0,
                true,
                self.config.file_mode.label(self.config.language),
                s.file_mode_tip,
                |ui| {
                    ui.selectable_value(
                        &mut self.config.file_mode,
                        FileSendMode::WholeFile,
                        s.whole_file,
                    );
                    ui.selectable_value(
                        &mut self.config.file_mode,
                        FileSendMode::LineByLine,
                        s.line_by_line,
                    );
                },
            );
            // 逐行发送时：行间隔控件紧跟在模式下拉框后面
            if self.config.file_mode == FileSendMode::LineByLine {
                ui.label(egui::RichText::new(s.line_interval).color(theme::text_soft()));
                let resp = ui.add_sized(
                    [80.0, 24.0],
                    egui::DragValue::new(&mut self.config.file_line_interval_ms)
                        .range(0..=60_000)
                        .suffix(" ms"),
                );
                resp.on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(s.line_interval_tip);
            }
            // 选择文件按钮：固定较宽宽度，选中文件后直接显示文件名
            let name = self
                .pending_file
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|f| f.to_string_lossy().into_owned());
            let btn_text = match &name {
                Some(n) => shorten_file_name(n, 22),
                None => s.choose_file.to_string(),
            };
            if ui
                .add_sized([180.0, 33.0], theme::secondary_widget(&btn_text))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.choose_file_tip)
                .clicked()
                && let Some(path) = rfd::FileDialog::new().pick_file()
            {
                self.pending_file = Some(path);
            }
            // 进度条：仅在发送时显示，占满"选择文件"按钮后的剩余宽度
            if self.file_send_active {
                let spacing = ui.spacing().item_spacing.x;
                let cancel_w = 72.0 + spacing;
                let (sent, total) = self.file_progress.unwrap_or((0, 0));
                let frac = if total > 0 {
                    sent as f32 / total as f32
                } else {
                    0.0
                };
                ui.add(
                    egui::ProgressBar::new(frac)
                        .desired_width((ui.available_width() - cancel_w).max(60.0))
                        .text(s.fill(
                            s.progress_bytes,
                            &[("sent", sent.to_string()), ("total", total.to_string())],
                        )),
                );
                if ui
                    .add_sized([72.0, 33.0], theme::primary_widget(s.cancel))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(s.cancel_send_tip)
                    .clicked()
                {
                    self.session.send(Command::StopFile);
                }
            }
        });
    }

    pub(crate) fn start_file_send(&mut self) {
        let Some(path) = self.pending_file.clone() else {
            self.set_status(self.t().choose_file_first.to_string(), true);
            return;
        };
        let mode = self.config.file_mode;
        let line_ending = self.config.line_ending.bytes().to_vec();
        let interval = self.config.file_line_interval_ms;
        self.session.send(Command::StartFile {
            path,
            mode,
            line_ending,
            line_interval_ms: interval,
        });
    }
}

/// 文件名过长时保留末尾（含扩展名），避免撑破固定宽度的"选择文件"按钮。
fn shorten_file_name(name: &str, max_chars: usize) -> String {
    let count = name.chars().count();
    if count <= max_chars {
        name.to_string()
    } else {
        let keep = max_chars.saturating_sub(1);
        let tail: String = name.chars().skip(count - keep).collect();
        format!("\u{2026}{tail}")
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{SerialApp, UiPanel};
    use crate::config::Config;
    use crate::i18n::Language;
    use crate::serial::SerialSession;
    use egui_dock::DockState;
    use std::sync::Arc;
    use std::time::Instant;

    fn make_app() -> SerialApp {
        SerialApp {
            session: SerialSession::spawn(Language::Chinese),
            config: Config::default(),
            last_save: Instant::now(),
            port_list: Vec::new(),
            auto_refresh_last: Instant::now(),
            session_connected: false,
            connected_port: String::new(),
            baud_input: String::new(),
            data_display: Vec::new(),
            display_segments: Vec::new(),
            display_cache: String::new(),
            display_dirty: false,
            display_segments_decoded: 0,
            display_cache_invalid: false,
            display_decoder: None,
            display_decoder_enc: None,
            display_dropped: 0,
            terminal_buffer: Vec::new(),
            terminal_text: String::new(),
            terminal_decoder: None,
            terminal_decoder_enc: None,
            terminal_cache_invalid: false,
            terminal_decoded_len: 0,
            terminal_input: String::new(),
            terminal_cursor: 0,
            rx_total: 0,
            tx_total: 0,
            paused: false,
            log_on: false,
            log_path: None,
            log_file: None,
            send_input: String::new(),
            send_history: Vec::new(),
            periodic_enabled: false,
            pending_file: None,
            file_send_active: false,
            file_progress: None,
            queue_sending: false,
            queue_progress: None,
            status: String::new(),
            status_error: false,
            alert: None,
            viewport_clamped: false,
            dock_state: Some(DockState::new(UiPanel::ALL.to_vec())),
        }
    }

    /// 运行一帧 UI。egui 0.36 起 `FullOutput` 携带纹理增量，测试不渲染到屏幕，
    /// 必须先 `clear()`，否则丢弃时触发 epaint 的 panic 检查。
    fn run_ui(
        ctx: &eframe::egui::Context,
        input: eframe::egui::RawInput,
        f: impl FnMut(&mut eframe::egui::Ui),
    ) -> eframe::egui::FullOutput {
        let mut out = ctx.run_ui(input, f);
        out.textures_delta.clear();
        out
    }

    fn cjk_ctx() -> eframe::egui::Context {
        let ctx = eframe::egui::Context::default();
        let mut fonts = eframe::egui::FontDefinitions::default();
        fonts.font_data.insert(
            "cjk".to_owned(),
            Arc::new(eframe::egui::FontData::from_static(include_bytes!(
                "../../assets/fonts/NotoSansCJKsc-Regular.otf"
            ))),
        );
        if let Some(list) = fonts.families.get_mut(&eframe::egui::FontFamily::Proportional) {
            list.insert(0, "cjk".to_owned());
        }
        ctx.set_fonts(fonts);
        ctx
    }

    fn text_positions(output: &eframe::egui::FullOutput) -> Vec<(String, f32, f32)> {
        let mut out = Vec::new();
        for clipped in &output.shapes {
            if let eframe::egui::epaint::Shape::Text(ts) = &clipped.shape {
                out.push((ts.galley.job.text.clone(), ts.pos.x, ts.galley.rect.width()));
            }
        }
        out
    }

    #[test]
    fn file_panel_layout_keeps_all_controls() {
        let ctx = cjk_ctx();
        let mut app = make_app();
        app.config.language = Language::Chinese;
        let output = run_ui(&ctx, Default::default(), |ui| {
            ui.allocate_ui_with_layout(
                eframe::egui::vec2(1084.0, 40.0),
                eframe::egui::Layout::left_to_right(eframe::egui::Align::Center),
                |ui| app.file_panel(ui),
            );
        });
        let s = Language::Chinese.strings();
        let positions = text_positions(&output);
        let x_of = |label: &str| {
            positions
                .iter()
                .find(|(t, _, _)| t == label)
                .unwrap_or_else(|| panic!("缺少控件文本: {label}"))
                .1
        };
        let order = [
            x_of(s.send_file),
            x_of(s.whole_file),
            x_of(s.choose_file),
        ];
        for w in order.windows(2) {
            assert!(w[0] < w[1], "文件行控件顺序异常: x={} 不在 x={} 左侧", w[0], w[1]);
        }
        assert!(!positions.iter().any(|(t, _, _)| t == s.cancel));
        assert!(!positions.iter().any(|(t, _, _)| t.contains('/')));
    }

    #[test]
    fn file_panel_button_shows_selected_file_name() {
        let ctx = cjk_ctx();
        let mut app = make_app();
        app.config.language = Language::Chinese;
        app.pending_file = Some(std::path::Path::new("data").join("test.bin"));
        let output = run_ui(&ctx, Default::default(), |ui| {
            ui.allocate_ui_with_layout(
                eframe::egui::vec2(1084.0, 40.0),
                eframe::egui::Layout::left_to_right(eframe::egui::Align::Center),
                |ui| app.file_panel(ui),
            );
        });
        let positions = text_positions(&output);
        assert!(
            positions.iter().any(|(t, _, _)| t == "test.bin"),
            "选择文件按钮应显示文件名"
        );
    }

    #[test]
    fn file_panel_shows_progress_bar_while_sending() {
        let ctx = cjk_ctx();
        let mut app = make_app();
        app.config.language = Language::Chinese;
        app.file_send_active = true;
        app.file_progress = Some((50, 100));
        let output = run_ui(&ctx, Default::default(), |ui| {
            ui.allocate_ui_with_layout(
                eframe::egui::vec2(1084.0, 40.0),
                eframe::egui::Layout::left_to_right(eframe::egui::Align::Center),
                |ui| app.file_panel(ui),
            );
        });
        let s = Language::Chinese.strings();
        let positions = text_positions(&output);
        assert!(
            positions.iter().any(|(t, _, _)| t == "50/100 字节"),
            "发送时应显示进度条文本"
        );
        assert!(positions.iter().any(|(t, _, _)| t == s.cancel));
    }
}