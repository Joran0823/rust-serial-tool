// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Joran

//! 生成应用图标资产（在 scripts/ 目录下以 cargo run 运行）：
//!   assets/app.ico - 多尺寸（256/64/48/32/16），用于 Windows 可执行文件资源
//!
//! 图标源为 assets/icon.png（egui 默认黑色图标，512x512）。
//! 运行：cargo run --release --manifest-path scripts/Cargo.toml

use image::imageops::FilterType;
use image::DynamicImage;
use std::fs;
use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let asset_dir = root.join("assets");

    let source = DynamicImage::ImageRgba8(
        image::open(asset_dir.join("icon.png"))
            .expect("读取 assets/icon.png 失败")
            .to_rgba8(),
    );

    // 多尺寸 ICO
    let sizes: [u32; 5] = [256, 64, 48, 32, 16];
    let mut entries: Vec<ico::IconDirEntry> = Vec::new();
    for &s in &sizes {
        let resized = source.resize(s, s, FilterType::Lanczos3);
        let icon_image = ico::IconImage::from_rgba_data(s, s, resized.to_rgba8().into_raw());
        entries.push(
            ico::IconDirEntry::encode_as_png(&icon_image).expect("ICO 条目编码失败"),
        );
    }

    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
    for e in entries {
        icon_dir.add_entry(e);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    icon_dir
        .write(&mut out)
        .expect("ICO 编码失败");
    fs::write(asset_dir.join("app.ico"), out.into_inner()).expect("写入 assets/app.ico 失败");

    println!("生成完成: assets/app.ico");
}
