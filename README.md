# RustSyncCV 客户端工作区

本仓库现已采用 Cargo Workspace，将命令行版本与图形界面版本拆分为两个独立 crate，便于统一依赖管理和发布流程。

## 目录结构

```
.
├── Cargo.toml        # 工作区清单
├── Cargo.lock        # 工作区锁文件
├── CLI/              # 命令行客户端 crate
└── GUI/              # 图形界面客户端 crate
```

## 快速开始

在运行以下命令前，请确保已经根据各子项目中的 `config.toml` 模板完成配置。

### 运行命令行客户端

```powershell
cargo run -p RustSyncCV-Client
```

发布版本：

```powershell
cargo build -p RustSyncCV-Client --release
```

### 运行图形界面客户端

```powershell
cargo run -p RustSyncCV-Client-GUI
```

发布版本：

```powershell
cargo build -p RustSyncCV-Client-GUI --release
```

## 常见问题

- 如果之前通过进入 `CLI/` 或 `GUI/` 子目录运行 Cargo 命令，现在推荐在工作区根目录直接使用 `-p` 选项指定目标 crate。
- 工作区会在根目录生成统一的 `Cargo.lock` 和 `target/`，如需清理可在根目录执行 `cargo clean`。

更多功能与配置细节请参考 `CLI/README.md` 以及 GUI 目录中的相关注释。
