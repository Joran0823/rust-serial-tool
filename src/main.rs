//! 串口调试助手 - 入口

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod codec;
mod config;
mod i18n;
mod queue_file;
mod serial;
mod ui;
mod util;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let lang = config::Config::load().language;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(
                lang.strings()
                    .fill(lang.strings().app_title, &[("version", app_version::APP_VERSION.to_string())]),
            )
            .with_inner_size([1100.0, 740.0])
            .with_min_inner_size([1100.0, 560.0])
            .with_icon(load_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "serial-tool",
        options,
        Box::new(|cc| Ok(Box::new(app::SerialApp::new(cc)))),
    )
}

// build.rs 生成的版本常量（V1.0.0；打 tag 构建时取 tag 名，如 V1.2.3）
mod app_version {
    include!(concat!(env!("OUT_DIR"), "/app_version.rs"));
}

/// 加载内置图标（assets/icon.png）作为窗口标题栏 / 任务栏图标。
fn load_icon() -> egui::IconData {
    let bytes = include_bytes!("../assets/icon.png");
    let image = image::load_from_memory(bytes).expect("内置图标 assets/icon.png 解码失败");
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    egui::IconData {
        rgba: rgba.into_raw(),
        width,
        height,
    }
}
