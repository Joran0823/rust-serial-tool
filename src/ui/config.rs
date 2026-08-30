//! 顶部配置区：串口配置（横向分布、填满窗口，含打开/关闭串口、语言、主题）。
//! 布局对应 docs/UI-Desing.svg（2026-08-01 版，800×700 单栏布局）。

use crate::app::SerialApp;
use crate::config::{
    DataBits, FlowControl, Parity, StopBits, ThemeSetting,
};
use crate::i18n::Language;
use crate::serial::Command;
use crate::ui::widgets;
use crate::ui::theme;
use eframe::egui;

impl SerialApp {
    /// 串口配置区：语言/主题/打开串口按钮 + 端口/波特率/数据位/停止位/校验位/流控，
    /// 以自动尺寸横向流式排布。
    pub fn config_panel(&mut self, ui: &mut egui::Ui) {
        let s = self.t();
        let enabled = !self.session_connected;
        const GAP: f32 = 8.0; // 控件之间的外部间距
        // 仅波特率文本框与下拉箭头需要固定尺寸，其余控件均为自动尺寸
        const BAUD_W: f32 = 88.0; // 波特率输入框（含边框与内部边距）总宽
        const BAUD_MARGIN_X: f32 = 8.0; // 输入框内部左右边距（4+4）
        const ARROW_W: f32 = 20.0;

        let flex = egui_flex::Flex::horizontal()
            .wrap(true)
            .gap(egui::vec2(GAP, 4.0));

        // 语言/主题/打开串口三个按钮固定为统一宽度：取“打开串口/Open Port/关闭串口/Close Port”
        // 中英文里最宽的文本，加上按钮水平内边距（style 中 button_padding.x=14 ×2）。
        // 这样无论中英文界面、是否已连接，三个按钮都等宽且文本完整显示。
        let btn_font = egui::TextStyle::Button.resolve(ui.style());
        let measure = |t: &str| {
            ui.ctx().fonts_mut(|f| {
                f.layout_no_wrap(
                    t.to_owned(),
                    btn_font.clone(),
                    egui::Color32::PLACEHOLDER,
                )
                .size()
                .x
            })
        };
        const BTN_PADDING_X: f32 = 28.0; // button_padding.x(14) × 2
        let wide = [
            Language::Chinese.strings().open_port,
            Language::English.strings().open_port,
            Language::Chinese.strings().close_port,
            Language::English.strings().close_port,
        ]
        .into_iter()
        .map(measure)
        .fold(0.0_f32, f32::max);
        let btn_w = wide + BTN_PADDING_X;
        let btn_item = || egui_flex::item().min_size(egui::vec2(btn_w, 28.0));

        flex.show(ui, |flex| {
            // 最左侧：语言按钮（固定宽度）
            let lang_btn = flex
                .add(btn_item(), theme::secondary_widget(s.language))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.language_tip);
            egui::Popup::menu(&lang_btn).show(|ui| {
                for lang in [Language::Chinese, Language::English] {
                    if ui
                        .selectable_label(self.config.language == lang, lang.label())
                        .clicked()
                    {
                        self.set_language(lang);
                        ui.close();
                    }
                }
            });

            // 主题按钮（固定宽度）
            let theme_btn = flex
                .add(btn_item(), theme::secondary_widget(s.theme))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.theme_tip);
            egui::Popup::menu(&theme_btn).show(|ui| {
                let system_dark = ui
                    .ctx()
                    .system_theme()
                    .is_none_or(|t| t == egui::Theme::Dark);
                for (label, setting) in [
                    (s.theme_system, ThemeSetting::System),
                    (s.theme_dark, ThemeSetting::Dark),
                    (s.theme_light, ThemeSetting::Light),
                ] {
                    if ui
                        .selectable_label(self.config.theme == setting, label)
                        .clicked()
                    {
                        self.config.theme = setting;
                        theme::apply_theme(ui.ctx(), setting.is_dark(system_dark));
                        ui.close();
                    }
                }
            });

            // 打开/关闭串口（固定宽度，与其他两个按钮等宽）
            let label = if self.session_connected {
                s.close_port
            } else {
                s.open_port
            };
            if flex
                .add(btn_item(), theme::primary_widget(label))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.open_close_tip)
                .clicked()
            {
                if self.session_connected {
                    self.session.send(Command::Close);
                } else {
                    self.open_port();
                }
            }

            // 端口：自动刷新复选框（含 label）+ 下拉框按文本自适应宽度，
            // 两者合为一个 flex 实体：空间不足时整体换行，不拆开。
            flex.add_ui(egui_flex::item(), |ui| {
                ui.checkbox(&mut self.config.auto_refresh_ports, s.auto_refresh)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(s.auto_refresh_tip);
                let port = if self.config.port.port_name.is_empty() {
                    s.port_placeholder.to_string()
                } else {
                    self.port_list
                        .iter()
                        .find(|(_, n)| n == &self.config.port.port_name)
                        .map(|(d, _)| d.clone())
                        .unwrap_or_else(|| self.config.port.port_name.clone())
                };
                widgets::combo(
                    ui,
                    12.0,
                    28.0,
                    enabled,
                    port,
                    s.port_tip,
                    |ui| {
                        for (display, name) in &self.port_list {
                            ui.selectable_value(
                                &mut self.config.port.port_name,
                                name.clone(),
                                display,
                            );
                        }
                    },
                );
            });

            // 波特率：label 自动宽度；文本框自适应 + 固定宽下拉按钮。
            // 整个组合（label+文本框+箭头）作为一个 flex 实体，不拆开。
            flex.add_ui(egui_flex::item(), |ui| {
                ui.label(egui::RichText::new(s.baud_rate).color(theme::text_soft()))
                    .on_hover_text(s.baud_rate_tip);
                {
                    let mut arrow_resp = None;
                egui::Frame::new()
                    .fill(theme::input_bg())
                    .stroke(theme::border())
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(4, 1))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = FIELD_INNER;
                        let text_w =
                            (BAUD_W - BAUD_MARGIN_X - ARROW_W - FIELD_INNER).max(40.0);
                        let mut baud = self.baud_input.clone();
                        let edit = ui.add_sized([text_w, 26.0],
                            egui::TextEdit::singleline(&mut baud)
                                .font(egui::TextStyle::Monospace)
                                .frame(egui::Frame::NONE)
                                .background_color(egui::Color32::TRANSPARENT)
                                .margin(egui::vec2(4.0, 0.0)),
                        );
                        if edit.changed() {
                            self.baud_input = baud;
                            if let Ok(v) = self.baud_input.trim().parse::<u32>() {
                                self.config.port.baud_rate = v;
                            }
                        }
                        widgets::text_edit_context_menu(
                            ui,
                            &edit,
                            true,
                            &self.baud_input,
                            self.t(),
                        );
                        edit.on_hover_text(s.baud_rate);
                        let arrow = ui
                            .add_sized(
                                [ARROW_W, 26.0],
                                egui::Button::new("")
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE),
                            )
                            .on_hover_text(s.common_bauds);
                        if ui.is_rect_visible(arrow.rect) {
                            let tri = egui::Rect::from_center_size(
                                arrow.rect.center(),
                                egui::vec2(6.0, 4.0),
                            );
                            ui.painter().add(egui::Shape::convex_polygon(
                                vec![tri.left_top(), tri.right_top(), tri.center_bottom()],
                                theme::text_soft(),
                                egui::Stroke::NONE,
                            ));
                        }
                        arrow_resp = Some(arrow);
                    });
                if let Some(arrow) = arrow_resp {
                    egui::Popup::menu(&arrow).show(|ui| {
                    for rate in [
                        9600u32, 19200, 38400, 57600, 115200, 230400, 460800, 921600,
                        1_000_000, 2_000_000,
                    ] {
                        if ui
                            .selectable_label(
                                self.config.port.baud_rate == rate,
                                rate.to_string(),
                            )
                            .clicked()
                        {
                            self.baud_input = rate.to_string();
                            self.config.port.baud_rate = rate;
                            ui.close();
                        }
                    }
                    });
                }
            }
            });

            // 数据位 / 停止位 / 校验位 / 流控（label 与下拉框每个组合是一个 flex 实体）
            flex.add_ui(egui_flex::item(), |ui| {
                field_ui(
                    ui,
                    s.data_bits,
                    enabled,
                    self.config.port.data_bits.label().to_string(),
                    s.data_bits_tip,
                    |ui| {
                        ui.selectable_value(&mut self.config.port.data_bits, DataBits::Five, "5");
                        ui.selectable_value(&mut self.config.port.data_bits, DataBits::Six, "6");
                        ui.selectable_value(&mut self.config.port.data_bits, DataBits::Seven, "7");
                        ui.selectable_value(&mut self.config.port.data_bits, DataBits::Eight, "8");
                    },
                );
            });
            flex.add_ui(egui_flex::item(), |ui| {
                field_ui(
                    ui,
                    s.stop_bits,
                    enabled,
                    self.config.port.stop_bits.label().to_string(),
                    s.stop_bits_tip,
                    |ui| {
                        ui.selectable_value(&mut self.config.port.stop_bits, StopBits::One, "1");
                        ui.selectable_value(
                            &mut self.config.port.stop_bits,
                            StopBits::OnePointFive,
                            "1.5",
                        );
                        ui.selectable_value(&mut self.config.port.stop_bits, StopBits::Two, "2");
                    },
                );
            });
            flex.add_ui(egui_flex::item(), |ui| {
                field_ui(
                    ui,
                    s.parity,
                    enabled,
                    self.config.port.parity.label().to_string(),
                    s.parity_tip,
                    |ui| {
                        ui.selectable_value(&mut self.config.port.parity, Parity::None, "None");
                        ui.selectable_value(&mut self.config.port.parity, Parity::Even, "Even");
                        ui.selectable_value(&mut self.config.port.parity, Parity::Odd, "Odd");
                        ui.selectable_value(&mut self.config.port.parity, Parity::Mark, "Mark");
                        ui.selectable_value(&mut self.config.port.parity, Parity::Space, "Space");
                    },
                );
            });
            let flow = match self.config.port.flow_control {
                FlowControl::None => s.flow_none,
                FlowControl::Software => s.flow_software,
                FlowControl::Hardware => s.flow_hardware,
            };
            flex.add_ui(egui_flex::item(), |ui| {
                field_ui(
                    ui,
                    s.flow_control,
                    enabled,
                    flow.to_string(),
                    s.flow_tip,
                    |ui| {
                        ui.selectable_value(
                            &mut self.config.port.flow_control,
                            FlowControl::None,
                            s.flow_none,
                        );
                        ui.selectable_value(
                            &mut self.config.port.flow_control,
                            FlowControl::Software,
                            s.flow_software,
                        );
                        ui.selectable_value(
                            &mut self.config.port.flow_control,
                            FlowControl::Hardware,
                            s.flow_hardware,
                        );
                    },
                );
            });
        });
    }

    pub fn refresh_ports(&mut self) {
        match serialport::available_ports() {
            Ok(ports) => {
                let mut list: Vec<(String, String)> = ports
                    .into_iter()
                    .map(|info| (port_display(&info), info.port_name))
                    .collect();
                list.sort_by(|a, b| a.1.cmp(&b.1));
                let prev = self.config.port.port_name.clone();
                self.port_list = list;
                if self.port_list.is_empty() {
                    self.config.port.port_name.clear();
                } else if !self.port_list.iter().any(|(_, n)| n == &prev) {
                    self.config.port.port_name = self.port_list[0].1.clone();
                }
            }
            Err(e) => {
                self.set_status(
                    self.t()
                        .fill(self.t().enum_ports_failed, &[("e", e.to_string())]),
                    true,
                );
            }
        }
    }

    fn open_port(&mut self) {
        let port_name = self.config.port.port_name.trim().to_string();
        if port_name.is_empty() {
            self.set_status(self.t().select_port_first.to_string(), true);
            return;
        }
        let baud: u32 = match self.baud_input.trim().parse() {
            Ok(v) if v > 0 => v,
            _ => {
                self.set_status(self.t().invalid_baud.to_string(), true);
                return;
            }
        };
        self.config.port.baud_rate = baud;
        self.config.port.port_name = port_name;
        let settings = self.config.port.clone();
        self.session.send(Command::Open(settings));
        self.set_status(self.t().opening_port.to_string(), false);
    }
}

/// 字段内部 label 与控件之间的间距。
const FIELD_INNER: f32 = 6.0;

/// 字段子布局：自动宽 label + 下拉框（基础宽度 12，内部按文本自适应扩展）。
///
/// egui_flex 的 flex 容器使用 `Align::Min`（顶部对齐），`add_ui` 里的子 Ui 会继承，
/// 导致矮的 label 与高的下拉框顶部对齐、视觉偏上。这里强制 `Align::Center`
/// 让 label 与下拉框在同一行内垂直居中。
#[allow(clippy::too_many_arguments)]
fn field_ui(
    ui: &mut egui::Ui,
    label: &str,
    enabled: bool,
    selected: String,
    tip: &str,
    options: impl FnOnce(&mut egui::Ui),
) {
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.label(egui::RichText::new(label).color(theme::text_soft()))
            .on_hover_text(tip);
        widgets::combo(ui, 12.0, 28.0, enabled, selected, tip, options);
    });
}

/// 端口显示名：优先 USB 产品名/制造商；无名称时直接显示端口名（不在尾部追加 COMx）。
fn port_display(info: &serialport::SerialPortInfo) -> String {
    let friendly = match &info.port_type {
        serialport::SerialPortType::UsbPort(usb) => usb
            .product
            .as_deref()
            .or(usb.manufacturer.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_default(),
        _ => String::new(),
    };
    if friendly.is_empty() {
        info.port_name.clone()
    } else {
        friendly
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

    /// 与应用启动时一致的 Context：默认字体 + CJK 字体优先。
    /// 渲染测试必须使用真实字体，否则中文文本在无 CJK 字体的环境中
    /// （如 CI 的 macOS/Windows runner）渲染结果会不一致。
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

    /// 收集渲染输出中所有文本及其 x 位置与绘制宽度。
    ///
    /// 在换行布局（`horizontal_wrapped`）下，egui 0.35 的 `Label` 会把
    /// “距行首的缩进”放进 `LayoutSection::leading_space`，最终体现在首个
    /// glyph 的 `pos.x` 偏移上，而 `Shape::Text.pos.x` 恒为行首 0。
    /// 因此真实绘制位置 = shape.pos + row.pos + 首个 glyph.pos，
    /// 宽度取所有 glyph advance_width 之和（galley.rect.width 含缩进不可用）。
    fn text_positions(output: &eframe::egui::FullOutput) -> Vec<(String, f32, f32)> {
        let mut out = Vec::new();
        for clipped in &output.shapes {
            if let eframe::egui::epaint::Shape::Text(ts) = &clipped.shape {
                let galley = &ts.galley;
                if let Some(row) = galley.rows.first() {
                    let glyph_x = row.glyphs.first().map_or(0.0, |g| g.pos.x);
                    let x = ts.pos.x + row.pos.x + glyph_x;
                    let w = row.glyphs.iter().map(|g| g.advance_width).sum::<f32>();
                    out.push((galley.job.text.clone(), x, w));
                }
            }
        }
        out
    }

    #[test]
    fn config_panel_wraps_to_second_row_narrow_window() {
        let ctx = cjk_ctx();
        let mut app = make_app();
        app.config.language = Language::Chinese;
        // 模拟 500px 窗口：控件会换到第二行
        let output = run_ui(&ctx, Default::default(), |ui| {
            ui.allocate_ui_with_layout(
                eframe::egui::vec2(500.0, 96.0),
                eframe::egui::Layout::left_to_right(eframe::egui::Align::Center),
                |ui| app.config_panel(ui),
            );
        });
        let s = Language::Chinese.strings();
        let positions = text_positions(&output);
        let find = |label: &str| {
            positions
                .iter()
                .find(|(t, _, _)| t == label)
                .unwrap_or_else(|| panic!("缺少控件文本: {label}"))
        };
        // 所有控件必须完整落在行内（右缘不超过面板宽度）
        for (t, x, w) in &positions {
            assert!(
                x + w <= 500.0,
                "控件被挤出: {t} x={x} w={w} (行宽 500)"
            );
        }
        // 语言按钮仍应在行首：其按钮矩形左缘从行首开始（文本因按钮内边距略偏移）
        let first_rect_left = output
            .shapes
            .iter()
            .filter_map(|clipped| {
                if let eframe::egui::epaint::Shape::Rect(rs) = &clipped.shape {
                    Some(rs.rect.left())
                } else {
                    None
                }
            })
            .fold(f32::MAX, f32::min);
        assert!(
            first_rect_left < 1.0,
            "语言按钮未从行首开始: rect_left={first_rect_left}",
        );
        // 自动刷新复选框应落到第一行或换行后第二行，且完整落在行内
        let (_, port_x, port_w) = *find(s.auto_refresh);
        assert!(port_x + port_w <= 500.0, "自动刷新被挤出: x={port_x} w={port_w}");
    }

    #[test]
    fn config_panel_layout_keeps_all_controls() {
        let ctx = cjk_ctx();

        for lang in [Language::Chinese, Language::English] {
            let mut app = make_app();
            app.config.language = lang;
            let output = run_ui(&ctx, Default::default(), |ui| {
                // 模拟 1100px 窗口内的顶部面板（1100 - 左右边距 16）
                ui.allocate_ui_with_layout(
                    eframe::egui::vec2(1200.0, 48.0),
                    eframe::egui::Layout::left_to_right(eframe::egui::Align::Center),
                    |ui| app.config_panel(ui),
                );
            });
            let s = lang.strings();
            let positions = text_positions(&output);
            let find = |label: &str| {
                positions
                    .iter()
                    .find(|(t, _, _)| t == label)
                    .unwrap_or_else(|| panic!("缺少控件文本: {label}"))
            };
            // 从左到右的顺序：语言 < 主题 < 打开串口 < 自动刷新 < 波特率 < 数据位 < 停止位 < 校验位 < 流控
            let order = [
                find(s.language).1,
                find(s.theme).1,
                find(s.open_port).1,
                find(s.auto_refresh).1,
                find(s.baud_rate).1,
                find(s.data_bits).1,
                find(s.stop_bits).1,
                find(s.parity).1,
                find(s.flow_control).1,
            ];
            for w in order.windows(2) {
                assert!(
                    w[0] < w[1],
                    "[{lang:?}] 控件顺序/间距异常: x={} 不在 x={} 左侧",
                    w[0],
                    w[1]
                );
            }
            // 所有控件必须完整落在行内（右缘不超过面板宽度）
            for (t, x, w) in &positions {
                assert!(
                    x + w <= 1200.0,
                    "[{lang:?}] 控件被挤出: {t} x={x} w={w} (行宽 1200)"
                );
            }
            // 语言按钮位于行首：其按钮矩形左缘从行首开始（文本因按钮内边距略偏移）
            let first_rect_left = output
                .shapes
                .iter()
                .filter_map(|clipped| {
                    if let eframe::egui::epaint::Shape::Rect(rs) = &clipped.shape {
                        Some(rs.rect.left())
                    } else {
                        None
                    }
                })
                .fold(f32::MAX, f32::min);
            assert!(
                first_rect_left < 1.0,
                "[{lang:?}] 语言按钮未从行首开始: rect_left={first_rect_left}",
            );
        }
    }

    #[test]
    fn config_panel_language_button_visible_in_real_panel() {
        let ctx = cjk_ctx();

        for lang in [Language::Chinese, Language::English] {
            let mut app = make_app();
            app.config.language = lang;
            let input = eframe::egui::RawInput {
                screen_rect: Some(eframe::egui::Rect::from_min_size(
                    eframe::egui::Pos2::ZERO,
                    eframe::egui::vec2(1100.0, 740.0),
                )),
                ..Default::default()
            };
            let output = run_ui(&ctx, input, |ui| {
                eframe::egui::Panel::top("serial_config")
                    .exact_size(48.0)
                    .frame(
                        eframe::egui::Frame::new()
                            .inner_margin(eframe::egui::Margin::same(8)),
                    )
                    .show(ui, |ui| {
                        app.config_panel(ui);
                    });
            });
            let s = lang.strings();
            let positions = text_positions(&output);
            let (_, x, w) = *positions
                .iter()
                .find(|(t, _, _)| t == s.language)
                .unwrap_or_else(|| panic!("[{lang:?}] 缺少语言按钮文本"));
            assert!(
                x + w <= 1100.0 - 4.0,
                "[{lang:?}] 语言按钮被挤出: 右缘={} (窗口 1100)",
                x + w
            );
        }
    }

    #[test]
    fn file_panel_layout_keeps_all_controls() {
        let ctx = cjk_ctx();
        let mut app = make_app();
        // Config::default() 按系统语言初始化，CI（en-US）下会渲染英文，
        // 必须显式指定语言，断言才与渲染内容一致。
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
        // 顺序：发送文件 < 整块发送 < 选择文件
        let order = [
            x_of(s.send_file),
            x_of(s.whole_file),
            x_of(s.choose_file),
        ];
        for w in order.windows(2) {
            assert!(w[0] < w[1], "文件行控件顺序异常: x={} 不在 x={} 左侧", w[0], w[1]);
        }
        // 未发送时：不显示进度条文本与取消按钮
        assert!(!positions.iter().any(|(t, _, _)| t == s.cancel));
        assert!(!positions.iter().any(|(t, _, _)| t.contains('/')));
    }

    #[test]
    fn file_panel_button_shows_selected_file_name() {
        let ctx = cjk_ctx();
        let mut app = make_app();
        app.config.language = Language::Chinese;
        // 使用平台无关的相对路径：Windows 上为 data\test.bin，
        // macOS/Linux 上为 data/test.bin，两者 file_name() 都返回 test.bin。
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
        // 进度条文本与取消按钮在发送时出现
        assert!(
            positions.iter().any(|(t, _, _)| t == "50/100 字节"),
            "发送时应显示进度条文本"
        );
        assert!(positions.iter().any(|(t, _, _)| t == s.cancel));
    }

}
