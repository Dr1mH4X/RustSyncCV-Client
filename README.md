# RustSyncCV-Client

RustSyncCV-Client 是一个基于 Rust 的跨平台剪贴板同步客户端，用于与 RustSyncCV 服务端进行通信，实现多台设备之间的剪贴板内容实时同步。

## 功能

- 实时监听本地剪贴板变化
- 将剪贴板内容发送到远程服务端
- 从[服务端](https://github.com/Dr1mH4X/RustSyncCV-Server)获取其他设备的剪贴板内容并更新本地剪贴板
- 支持文本、图片等常见剪贴板数据类型

## 先决条件

- Rust 工具链（推荐使用稳定版）
- 配置文件 `config.toml`，用于指定服务端地址和认证信息

## 配置

项目根目录下的 `config.toml` 文件示例如下：

```toml
# 服务端 WebSocket 地址，必填
server_url = "ws://localhost:8067/ws"

# 可选：Token 验证，与 username/password 二选一
# token = "YOUR_TOKEN"

# 可选：用户名/密码认证，与 token 二选一
# username = "test"
# password = "ilovcv"

# 可选：同步间隔（秒），默认 5 秒
# sync_interval = 5
```

根据实际需求修改各字段。

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

## 许可证

本项目基于 MIT 许可证，详情见 [LICENSE](LICENSE)。

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=Dr1mH4X/RustSyncCV-Client,Dr1mH4X/RustSyncCV-Server&type=Date)](https://www.star-history.com/#Dr1mH4X/RustSyncCV-Client&Dr1mH4X/RustSyncCV-Server&Date)
