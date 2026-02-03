# RustSyncCV Client 开发手册

## 前置要求

确保已安装以下环境：

1.  **Rust**: [安装 Rust](https://www.rust-lang.org/tools/install)
2.  **Node.js**: [安装 Node.js>25.0.0](https://nodejs.org/)
3.  **pnpm**: `npm install -g pnpm`
4.  **Tauri 环境依赖**: 根据你的操作系统，遵循 [Tauri 系统要求](https://v2.tauri.app/start/prerequisites/) 进行配置。

## 项目设置

克隆仓库并安装依赖：

```bash
git clone https://github.com/your-username/RustSyncCV-Client.git
cd RustSyncCV-Client
pnpm install
```

## 开发

以热重载模式运行应用：

```bash
pnpm tauri dev
```

## 架构概览

-   **前端**: `src/` (React, Tailwind CSS, TypeScript)
    -   `components/`: UI 组件 (MainWindow, SettingsWindow)。
    -   `i18n/`: 国际化文件。
-   **后端**: `src-tauri/src/` (Rust)
    -   `main.rs`: 入口点，Tauri 设置，指令 (commands)。
    -   `runtime/`: 连接和剪贴板同步的核心逻辑。
