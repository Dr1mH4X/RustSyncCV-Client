# RustSyncCV-Client

RustSyncCV-Client 是一个基于 Rust 的跨平台剪贴板同步客户端，用于与 RustSyncCV 服务端进行通信，实现多台设备之间的剪贴板内容实时同步。

## 功能

- 实时监听本地剪贴板变化
- 将剪贴板内容发送到远程服务端
- 从服务端获取其他设备的剪贴板内容并更新本地剪贴板
- 支持文本、图片等常见剪贴板数据类型

## 先决条件

- Rust 工具链（推荐使用稳定版）
- 配置文件 `config.toml`，用于指定服务端地址和认证信息

## 安装与运行

```powershell
# 克隆仓库
git clone https://github.com/Dr1mH4X/RustSyncCV-Client.git
cd RustSyncCV-Client

# 构建项目（调试模式）
cargo build

# 构建项目（发布模式）
cargo build --release

# 运行项目（使用默认配置）
cargo run
```

## 配置

项目根目录下的 `config.toml` 文件示例如下：

```toml
# 服务端地址
server_addr = "http://127.0.0.1:8080"

# 设备标识，用于区分不同客户端
device_id = "client-01"

# HTTP 请求超时时间（秒）
timeout_secs = 10
```

根据实际需求修改 `server_addr`、`device_id` 等字段。

## 项目结构

```
.
├── Cargo.toml        # 项目依赖和元数据定义
├── config.toml       # 默认配置文件示例
├── src/
│   ├── main.rs       # 程序入口
│   ├── clipboard.rs  # 剪贴板监听与操作模块
│   ├── config.rs     # 配置加载模块
│   └── messages.rs   # 网络消息定义与序列化
└── target/           # Cargo 构建输出目录（已被 .gitignore 忽略）
```

## 贡献

欢迎提交 issue 或 PR，参与项目改进。

## 许可证

本项目基于 MIT 许可证，详情见 [LICENSE](LICENSE)。