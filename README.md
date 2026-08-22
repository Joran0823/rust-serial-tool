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

需要 Rust 工具链（stable）。

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
