## 开发调试

### 安装依赖

**Node.js** (>= 18)

```bash
# macOS (Homebrew)
brew install node
brew install pnpm

# Windows (Scoop)
scoop install nodejs
scoop install -g pnpm
```

**Rust**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

## 开发

**项目依赖**

```bash
pnpm install
```

### 开发调试

```bash
pnpm tauri dev
```

启动前端开发服务器和 Tauri 桌面应用，支持热重载。

### 生产构建

```bash
pnpm tauri build
```

## 技术栈

| 类别     | 技术                                                |
| -------- | -------------------------------------------------- |
| 桌面框架 | [Tauri](https://tauri.app/) v2                      |
| 后端语言 | [Rust](https://www.rust-lang.org/) 1.70+            |
| 前端框架 | [React](https://react.dev/) 19                      |
| 类型系统 | [TypeScript](https://www.typescriptlang.org/) 5.8   |
| 样式方案 | [Tailwind CSS](https://tailwindcss.com/) 4          |
| 国际化   | [i18next](https://www.i18next.com/) i18next         |
| 构建工具 | [Vite](https://vitejs.dev/) 7                       |

## 架构

-   **前端**: `src/` (React, Tailwind CSS, TypeScript)
    -   `components/`: UI 组件 (MainWindow, SettingsWindow)。
    -   `i18n/`: 国际化文件。
-   **后端**: `src-tauri/src/` (Rust)
    -   `main.rs`: 入口点，Tauri 设置，指令 (commands)。
    -   `runtime/`: 连接和剪贴板同步的核心逻辑。
