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
# 服务端 WebSocket 地址 (ws:// 或 wss://)
server_url = "ws://localhost:8067/ws"

# 可选：Token 验证，与 username/password 二选一
# token = "YOUR_TOKEN"

# 可选：用户名/密码认证，与 token 二选一
# username = "test"
# password = "ilovcv"

# 可选：同步间隔（秒），默认 5 秒
# sync_interval = 5

# 可选：最大允许发送的图片大小（KB），默认 512，超过则跳过广播
# max_image_kb = 512

# 可选：跳过 TLS 证书校验 (仅调试自签名/过期证书环境; 强烈不建议生产启用)
# trust_insecure_cert = false
```

根据实际需求修改各字段。

### 图片与大小限制说明

客户端会：
- 轮询剪贴板，优先检测文本，其次检测图片。
- 图片数据将转为 PNG 后再 base64 编码发送（content_type = `image/png`）。
- 对图片使用内容哈希去重，避免重复广播同一图片。
- 若编码后 PNG 大小超过 `max_image_kb`（默认 512KB）将跳过发送，并输出日志：`[clip:image] skip oversized ...`。
- 接收到来自服务端的 `image/png` 消息时，会 base64 解码并写入本地剪贴板（RGBA）。

注意：某些平台/应用复制的超大图片（例如截图工具的高分辨率原始数据）可能会被过滤；如需放宽请调高 `max_image_kb`。 

## 安全与 TLS 证书验证

默认情况下，若 `server_url` 使用 `wss://`，客户端会执行标准 TLS 证书验证。

在调试使用自签名、过期或尚未正确部署证书的测试服务器时，可以临时关闭验证：

方式一（配置文件）：
```toml
trust_insecure_cert = true
```

方式二（命令行覆盖 / 临时使用）：
```bash
cargo run -- --insecure
```
或已构建可执行文件：
```powershell
./RustSyncCV-Client.exe --insecure
```

判定逻辑：只要配置项为 `true` 或命令行包含 `--insecure`，即进入“不安全模式”并完全跳过证书校验。

⚠️ 警告：启用后易受到中间人攻击 (MITM)。请勿在生产或不可信网络环境使用。务必在调试结束后恢复安全模式。

## 运行示例

调试（普通非 TLS）：
```bash
cargo run
```

调试自签名 TLS（跳过验证，风险自担）：
```bash
cargo run -- --insecure
```

生产（推荐，使用有效证书）：
```bash
cargo build --release
./target/release/RustSyncCV-Client
```

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
