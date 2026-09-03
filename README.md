<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 Joran -->

# 串口调试助手（Serial Tool）

> ⚠️ **本程序由 AI 编写**（代码由 AI 生成，人工进行需求整理、验证与使用）。

一款基于 Rust + egui/eframe 的跨平台串口调试助手，用于串口设备的调试与开发。轻量、启动快，支持单可执行文件分发（不依赖外部运行时）。

## 主要功能

- 串口自动枚举与刷新（Windows 的 COM 口、Linux/macOS 的 tty 设备）
- 连接参数配置：波特率、数据位、停止位、校验位、流控
- 数据收发：文本 / HEX 双模式、时间戳、自动滚动、暂停显示
- 发送区：文本 / HEX 输入、行尾选择、发送历史、定时发送
- 发送队列：多条命名队列、按序发送、条目级延迟、队列保存 / 导入（TOML / TXT）
- 发送文件（后台线程分块发送，带进度与取消）
- 日志保存、收发字节统计
- 设置持久化（记住上次的端口参数与 UI 偏好）
- 内置中文字体支持

## 构建

### 1. 安装 Rust 工具链

需要 stable 版 Rust，推荐通过 [rustup](https://rustup.rs) 安装（默认安装的就是 stable 工具链）：

- **Windows**：下载并运行 [rustup-init.exe](https://rustup.rs)，按提示安装；也可用 winget：`winget install Rustlang.Rustup`
- **Linux**：`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

### 2. 安装系统构建依赖

#### Windows

Windows 上不需要额外安装系统库，但 Rust 的 MSVC 工具链在编译时需要 Visual Studio 的 C++ 构建工具：

1. 安装 [Visual Studio Build Tools 2022](https://visualstudio.microsoft.com/zh-hans/downloads/#build-tools-for-visual-studio-2022)（或完整版 Visual Studio）；
2. 勾选“**使用 C++ 的桌面开发**”工作负载（包含 MSVC 编译器与 Windows SDK）。

也可用 winget 命令行安装（等价于勾选上述工作负载）：

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

> 建议使用默认的 MSVC 工具链。Windows 下 `build.rs` 会通过 MSVC 资源编译器嵌入应用图标与版本信息。

#### Linux

Ubuntu / Debian：

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libudev-dev \
    libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-dev libx11-dev libxrandr-dev libxi-dev libwayland-dev \
    libvulkan-dev libssl-dev
```

Fedora / RHEL 系：

```bash
sudo dnf install -y gcc gcc-c++ make pkgconf-pkg-config systemd-devel \
    libxcb-devel libxkbcommon-devel libX11-devel libXrandr-devel libXi-devel \
    wayland-devel vulkan-loader-devel openssl-devel
```

> 其他发行版请安装对应的开发包：pkg-config、libudev（udev 头文件）、X11/Wayland、Vulkan 与 OpenSSL。这些依赖来自 eframe/winit（GUI 窗口）与 serialport（串口枚举）。

> 提示：Linux 下访问串口通常需要把当前用户加入 `dialout` 组并重新登录后生效：`sudo usermod -aG dialout $USER`

### 3. 构建

```bash
# 开发版
cargo build

# 发布版（Windows 下自动嵌入应用图标）
cargo build --release
```

生成的可执行文件位于 `target/release/`（Windows 为 `serial-tool.exe`）。

## 版本号

- 程序在标题栏显示版本号（如 `串口调试助手 V1.0.0`），同时写入可执行文件的版本信息。
- 本地构建默认版本为 `V1.0.0`。
- 构建时可通过环境变量 `SERIAL_TOOL_VERSION` 指定版本号，例如：

  ```bash
  SERIAL_TOOL_VERSION=v1.2.3 cargo build --release
  ```

## 发布（GitHub Actions）

推送 `v*` 格式的 tag（如 `v1.2.3`）后，[`release.yml`](.github/workflows/release.yml) 会自动：

1. 在 Windows / Linux / macOS 上构建 release 版本；
2. 将 tag 名作为版本号注入程序（标题栏显示 `串口调试助手 V1.2.3`）；
3. 把各平台产物上传到对应的 GitHub Release。

示例：

```bash
git tag v1.2.3
git push origin v1.2.3
```

## 运行

```bash
cargo run
```

## 项目结构

- `src/` — 主程序源码（入口 `src/main.rs`）
- `assets/` — 图标与字体资源
- `docs/` — 设计文档与设计稿
- `scripts/` — 图标生成工具

## 说明

- 设计文档见 [`docs/design.md`](docs/design.md)。
- 本程序由 AI 编写，请在使用前自行验证其行为是否符合预期。
