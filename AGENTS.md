# AGENTS.md — 串口调试助手 (Serial Tool)

> AI 编码助手快速上手指南。详细设计见 [docs/design.md](docs/design.md)。

## 项目概要

Rust + egui/eframe 跨平台串口调试助手。单二进制分发，内置中文字体，支持文本/HEX 收发、发送队列、文件发送、日志记录。

- **语言/版本**: Rust 2024 edition
- **GUI**: egui/eframe 0.35 (即时模式) + egui_dock 0.20 (停靠面板)
- **串口**: `serialport` 4.9
- **序列化**: `serde` + `toml` (配置持久化、队列导入导出)
- **编码**: `encoding_rs` (UTF-8/GBK/ASCII)

## 常用命令

```bash
cargo build              # 开发构建
cargo build --release    # 发布构建 (Windows 自动嵌入图标和版本信息)
cargo run                # 运行
cargo test               # 运行全部测试 (~60 个)
cargo clippy --all-targets  # Lint 检查
```

CI 会执行 `cargo clippy --all-targets` + `cargo test` + `cargo build --release`，跨 Windows/Linux/macOS。

## 架构

三层结构，详见 [docs/design.md §4](docs/design.md#4-总体架构)：

```
UI 层 (egui 主线程)  ←→  会话层 (SerialSession 后台线程)  ←→  硬件层 (serialport)
```

- **主线程**: eframe 事件循环，所有 UI 渲染，不做阻塞 IO
- **写线程**: 持有串口句柄，处理 Command 通道的写操作/队列/文件/周期发送
- **读线程**: 每次打开串口额外启动，50ms 超时聚合读取，通过有界 `sync_channel(1024)` 传给 UI（背压：数据不丢弃）

### 关键数据结构

- `SerialApp` (`src/app.rs`) — 约 50 字段的巨型状态结构体，`eframe::App` 实现
- `SerialSession` (`src/serial/session.rs`) — 持有 `cmd_tx`/`evt_rx` 通道和后台线程句柄
- `Command` / `Event` 枚举 — 跨线程通信协议
- `Config` (`src/config.rs`) — 可持久化配置 (TOML)，含 `PortSettings`、`SendQueue` 等

## 代码约定

### 国际化 (i18n)

所有 UI 文案通过 `Strings` 结构体 (`src/i18n.rs`) 获取，**禁止硬编码中文字符串**。用法：

```rust
let s = self.strings();  // 返回 &'static Strings
ui.label(s.some_label);
// 模板替换: s.fill("前缀 {key}", &[("key", "value")])
```

### 即时模式 UI

所有 UI 渲染在 `impl SerialApp` 的方法中，直接调用 `ui.xxx()`。不使用 retained-mode widget 状态。

自定义控件在 `src/ui/widgets.rs`：
- `combo()` — 统一样式下拉框（白底描边、手绘箭头、Popup 菜单）
- `text_edit_context_menu()` — 文本框右键菜单

### 主题

全局 `AtomicBool` (`IS_DARK`) 控制深色/亮色。所有颜色通过 `src/ui/theme.rs` 的 getter 函数获取（`text()`, `bg()`, `border()` 等），**不要硬编码颜色值**。

### 配置持久化

`Config` 通过 `serde` + `toml` 序列化，存储在 `directories::ProjectDirs` 配置目录。每 3 秒自动保存，`Drop` 和退出时也会保存。新增字段必须使用 `#[serde(default = "...")]` 保证向后兼容。

## 常见陷阱

- **egui_dock 借用冲突**: `dock_state` 用 `Option` 包裹，每帧 `take()` 取出渲染后再放回。`DockViewer` 通过快照和 `pending_restore` 实现面板恢复。**修改停靠相关代码时务必保持此模式**。

- **写操作可靠性**: `write_all_dyn()` 处理 `Ok(0)`/`WouldBlock`/`TimedOut`，最多重试 3000 次（10ms 间隔）。**不要简化此重试逻辑**。

- **serialport 库限制**: 不支持 1.5 停止位和 Mark/Space 校验。UI 保留选项但打开串口时会报错。**不要移除这些选项**，后续版本会自行实现。

- **HEX 解析**: 忽略空白和逗号，支持 `0x` 前缀，连续偶数个 hex 字符视为字节序列。奇数长度报错。

- **Rust 2024 edition**: 注意 edition 2024 的语法变化（如 `unsafe` 块、`impl Trait` 等）。

## 关键文件索引

| 文件 | 内容 |
|------|------|
| `src/main.rs` | 入口、内嵌字体/图标、eframe 启动 |
| `src/app.rs` | 核心状态、事件循环、停靠布局 |
| `src/config.rs` | 持久化配置结构体 |
| `src/i18n.rs` | 中英文案、语言切换 |
| `src/serial/session.rs` | 串口会话线程、命令/事件通道 |
| `src/codec/hex.rs` | HEX 解析/格式化 |
| `src/codec/text.rs` | 文本编解码 |
| `src/ui/config.rs` | 左栏配置面板 |
| `src/ui/data.rs` | 数据展示区、增量解码、ANSI |
| `src/ui/send.rs` | 发送区、文件发送、队列面板 |
| `src/ui/statusbar.rs` | 底部状态栏 |
| `src/ui/theme.rs` | 深色/亮色主题 |
| `src/ui/widgets.rs` | 自定义 combo 下拉框、右键菜单 |
| `build.rs` | 版本注入、Windows 资源嵌入 |