//! 底部状态栏：状态信息、连接状态、收发统计。

use crate::app::SerialApp;
use crate::ui::theme;
use eframe::egui;

impl SerialApp {
    pub fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let color = if self.status_error {
                theme::ERROR_RED
            } else {
                theme::OK_GREEN
            };
            ui.label(egui::RichText::new(&self.status).color(color));
            ui.separator();
            let conn = if self.session_connected {
                format!("连接 {}", self.connected_port)
            } else {
                "未连接".to_string()
            };
            ui.label(conn);
            ui.separator();
            ui.label(format!("RX {} B", self.rx_total));
            ui.label(format!("TX {} B", self.tx_total));
            if self.periodic_enabled {
                ui.label(
                    egui::RichText::new("周期发送中")
                        .color(egui::Color32::from_rgb(0, 160, 80)),
                );
            }
        });
    }
}
