use std::env;
use std::fs;
use std::path::PathBuf;

/// 版本号来源：
/// 1. 环境变量 `SERIAL_TOOL_VERSION`（由 release workflow 在打 tag 时注入，如 `v1.2.3`）；
/// 2. 回退到 `Cargo.toml` 的 package.version（本地构建，默认 1.0.0）。
fn app_version() -> String {
    env::var("SERIAL_TOOL_VERSION")
        .ok()
        .map(|v| v.trim().trim_start_matches(['v', 'V']).to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "1.0.0".into()))
}

/// 标题栏显示的版本，如 `V1.2.3`。
fn display_version(version: &str) -> String {
    format!("V{version}")
}

/// Windows 文件版本（`major,minor,build,revision`）。
fn numeric_version(version: &str) -> [u16; 4] {
    let parts: Vec<u16> = version
        .split(['.', '-', '+'])
        .filter_map(|p| p.parse::<u16>().ok())
        .collect();
    [parts.first().copied().unwrap_or(0),
     parts.get(1).copied().unwrap_or(0),
     parts.get(2).copied().unwrap_or(0),
     parts.get(3).copied().unwrap_or(0)]
}

fn main() {
    println!("cargo:rerun-if-env-changed=SERIAL_TOOL_VERSION");
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets/app.ico");

    let version = app_version();
    let display = display_version(&version);

    // 生成版本常量文件，供 src/main.rs 使用
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR 未设置"));
    let version_file = out_dir.join("app_version.rs");
    fs::write(
        &version_file,
        format!(
            "// 由 build.rs 自动生成，请勿手动修改\n\
             pub const APP_VERSION: &str = \"{display}\";\n"
        ),
    )
    .expect("写入版本常量文件失败");
    println!("cargo:rerun-if-changed={}", version_file.display());

    // Windows：嵌入应用图标 + 文件版本信息
    #[cfg(target_os = "windows")]
    {
        let [maj, min, build, rev] = numeric_version(&version);
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR 未设置");
        let icon = PathBuf::from(&manifest_dir).join("assets/app.ico");
        let icon_escaped = icon.to_string_lossy().replace('\\', "\\\\");

        let rc = format!(
            "// 由 build.rs 自动生成，请勿手动修改\n\
             #include <winres.h>\n\
             #pragma code_page(65001)\n\
             1 ICON \"{icon_escaped}\"\n\
             \n\
             1 VERSIONINFO\n\
             FILEVERSION {maj},{min},{build},{rev}\n\
             PRODUCTVERSION {maj},{min},{build},{rev}\n\
             FILEFLAGSMASK 0x3fL\n\
             #ifdef _DEBUG\n\
             FILEFLAGS 0x1L\n\
             #else\n\
             FILEFLAGS 0x0L\n\
             #endif\n\
             FILEOS 0x40004L\n\
             FILETYPE 0x1L\n\
             FILESUBTYPE 0x0L\n\
             BEGIN\n\
                 BLOCK \"StringFileInfo\"\n\
                 BEGIN\n\
                     BLOCK \"080404b0\"\n\
                     BEGIN\n\
                         VALUE \"CompanyName\", \"serial-tool\"\n\
                         VALUE \"FileDescription\", \"串口调试助手\"\n\
                         VALUE \"FileVersion\", \"{display}\"\n\
                         VALUE \"InternalName\", \"serial-tool\"\n\
                         VALUE \"OriginalFilename\", \"serial-tool.exe\"\n\
                         VALUE \"ProductName\", \"串口调试助手\"\n\
                         VALUE \"ProductVersion\", \"{display}\"\n\
                     END\n\
                 END\n\
                 BLOCK \"VarFileInfo\"\n\
                 BEGIN\n\
                     VALUE \"Translation\", 0x0804, 1200\n\
                 END\n\
             END\n"
        );

        let rc_path = out_dir.join("app.rc");
        fs::write(&rc_path, rc).expect("写入 app.rc 失败");
        embed_resource::compile(rc_path, embed_resource::NONE)
            .manifest_optional()
            .expect("嵌入应用图标资源失败");
    }
}
