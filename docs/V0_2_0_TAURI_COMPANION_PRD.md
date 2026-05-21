# EchoNote v0.2.0 需求文档：Tauri ASR Companion

## 1. 文档信息

- 产品名称：EchoNote
- 目标版本：v0.2.0
- 版本主题：macOS ASR Companion 应用
- 目标平台：macOS
- 主要客户端：Obsidian Desktop 插件
- 技术路线：Tauri + 现有 Python ASR Service
- 文档状态：需求草案

## 2. 背景

EchoNote v0.1.0 已验证核心闭环：Obsidian 插件可以录制音频、切分音频分段、调用本地 ASR HTTP 服务、将转录写入 Markdown 会议笔记，并生成结构化会议总结。

当前 ASR 服务是一个独立 Python HTTP 进程。插件虽然可以自动启动它，但用户仍需要理解和配置：

- Python 可执行文件路径。
- ASR service 工程路径。
- 本地服务端口。
- ASR 模型 ID 和模型加载状态。
- ASR service 启动错误和日志。

这对非开发者用户门槛偏高。服务出错时，用户也很难从 Obsidian 内定位到底是 Python、模型、端口、音频输入还是插件调用出了问题。

v0.2.0 引入一个 macOS GUI Companion 应用，由 Companion 负责本地 ASR runtime 的启动、停止、状态监控、日志和诊断。Obsidian 插件继续负责会议笔记、录音、转录写入和总结。

## 3. 一句话描述

EchoNote ASR Companion 是一个 macOS Tauri 应用，用于启动、停止、监控和诊断本地 ASR 服务，让 Obsidian 用户无需手动管理 Python 路径和终端进程。

## 4. 目标

### 4.1 产品目标

- 降低本地 ASR 使用门槛。
- 让 ASR 服务状态在 Obsidian 外也清晰可见。
- 给用户一个统一入口启动、停止、重启和诊断 ASR 服务。
- 保留 v0.1.0 的 Obsidian 会议笔记工作流。
- 继续保持本地优先和隐私可控。

### 4.2 工程目标

- 新增 `companion/` Tauri 应用工程。
- 第一版不重写 ASR service，直接管理现有 Python FastAPI 服务子进程。
- 保持现有 ASR HTTP API 兼容。
- 增加本地 service discovery 文件，让插件能发现 Companion 管理的服务地址。
- 插件保留现有手动 ASR 配置作为 fallback。

## 5. 非目标

v0.2.0 不包含：

- 打包完整 Python runtime。
- 将 MLX 模型权重内置到 Companion。
- 模型自动下载 UI。
- macOS 签名、公证和自动更新。
- Windows 或 Linux 支持。
- 用 Rust、Swift 或其他语言重写 ASR 推理。
- 替换 Obsidian 插件 UI。
- 移除插件直接启动 ASR service 的现有 fallback。
- 发言人分离。
- token 级流式 ASR。
- 不依赖虚拟声卡的系统音频捕获。

这些能力可在 Companion MVP 验证后继续规划。

## 6. 目标用户

- 希望在 Obsidian 中使用本地会议转录，但不想打开终端的用户。
- 能安装 macOS 应用和 Obsidian 插件，但不想理解 Python 服务进程的用户。
- 已使用 v0.1.0，但需要更好 ASR 故障诊断能力的用户。

## 7. 核心用户流程

### 7.1 首次配置

1. 用户安装 EchoNote Obsidian 插件。
2. 用户安装 EchoNote ASR Companion。
3. 用户打开 Companion。
4. Companion 检查：
   - ASR service 路径。
   - Python 可执行文件路径。
   - 服务端口是否可用。
   - 当前 ASR 模型配置。
5. 用户点击 `Start Service`。
6. Companion 启动 ASR service，并等待 `/health` 返回正常。
7. Companion 加载或展示当前模型状态。
8. 用户打开 Obsidian 并开始会议。
9. 插件发现 Companion 管理的 ASR endpoint，并将音频分段发送给该服务。

### 7.2 日常使用

1. 用户启动 Companion。
2. Companion 显示 `Service: Running` 和 `Model: Ready`。
3. 用户在 Obsidian 中开始会议。
4. 插件使用当前本地 ASR endpoint。
5. 如果 ASR 出错，用户打开 Companion 查看日志，并复制诊断报告。

### 7.3 故障恢复

如果 ASR 子进程异常退出：

1. Companion 状态变为 `Service: Error`。
2. Companion 显示最近退出码和 stderr 日志摘要。
3. 用户点击 `Restart Service`。
4. 插件端尽可能保持当前会议 session，不因单次 ASR 失败直接丢失会议笔记。
5. 用户可复制诊断报告提交 GitHub issue。

## 8. Companion MVP 范围

### 8.1 主窗口

Companion 主窗口必须展示：

- Service 状态：`Stopped`、`Starting`、`Running`、`Error`。
- Model 状态：`Not Loaded`、`Loading`、`Ready`、`Error`。
- 当前 ASR 模型 ID。
- 本地 API URL。
- Python 可执行文件路径。
- ASR service 路径。
- 最近错误摘要。

主操作：

- `Start Service`
- `Stop Service`
- `Restart Service`
- `Load Model`
- `Copy Diagnostic Report`
- `Open Logs Folder`

### 8.2 设置项

Companion 必须支持：

- Python 可执行文件路径。
- ASR service 目录。
- 首选服务端口，默认 `8765`。
- ASR 模型预设：
  - `mlx-community/Qwen3-ASR-0.6B-4bit`
  - `mlx-community/Qwen3-ASR-1.7B-4bit`
  - 自定义模型 ID
- Backend：
  - `fake`
  - `mlx-audio`
- 打开 Companion 时自动启动服务。

设置文件建议保存到：

```text
~/Library/Application Support/EchoNote/companion-settings.json
```

### 8.3 服务发现

Companion 必须在服务状态变化时写入 discovery 文件：

```text
~/Library/Application Support/EchoNote/companion.json
```

建议 schema：

```json
{
  "version": 1,
  "app": "EchoNote ASR Companion",
  "service": "echonote-asr",
  "status": "running",
  "baseUrl": "http://127.0.0.1:8765",
  "host": "127.0.0.1",
  "port": 8765,
  "backend": "mlx-audio",
  "modelId": "mlx-community/Qwen3-ASR-0.6B-4bit",
  "modelStatus": "ready",
  "pid": 12345,
  "updatedAt": "2026-05-20T10:00:00.000Z"
}
```

插件行为：

- discovery 文件存在且 `status` 为 `running` 时，优先使用 Companion 提供的 `baseUrl`。
- discovery 缺失或不可用时，回退到插件现有手动配置。
- 如果 `updatedAt` 超过 30 秒且 `/health` 失败，则视为 stale。

### 8.4 日志

Companion 必须捕获 ASR service 的 stdout 和 stderr，并写入日志文件：

```text
~/Library/Logs/EchoNote/companion.log
~/Library/Logs/EchoNote/asr-service.log
```

主窗口应提供只读最近日志面板。

### 8.5 诊断报告

`Copy Diagnostic Report` 复制 Markdown 格式诊断信息，至少包含：

- Companion 版本。
- macOS 版本。
- CPU 架构。
- Service 状态。
- Model 状态。
- Backend。
- Model ID。
- Base URL。
- Python 可执行文件路径。
- ASR service 路径。
- 最近一次 service 退出码。
- 最近 30 行日志。

诊断报告必须脱敏，不得包含 API key、LLM token 或其他密钥。

## 9. Obsidian 插件改动

插件必须在没有 Companion 的情况下继续可用。

### 9.1 设置项

新增 ASR runtime mode：

- `Auto`
- `Companion`
- `Manual`

默认值：`Auto`。

模式行为：

- `Auto`：优先使用 Companion discovery；不可用时回退到现有手动配置。
- `Companion`：必须使用 Companion；不可用时给出明确错误。
- `Manual`：忽略 Companion，继续使用现有 Python path、ASR service path 和端口配置。

### 9.2 状态面板

状态面板新增 Companion 相关信息：

- `ASR Runtime`
- `Companion Status`
- `Companion API`
- `Discovery File`

新增操作：

- `Refresh Companion Status`

### 9.3 服务启动策略

在 `Auto` 和 `Companion` 模式下，如果检测到 Companion 管理的服务正在运行，插件不再直接启动 Python ASR 进程。

在 `Manual` 模式下，保留 v0.1.0 的行为。

## 10. API 兼容性

现有 ASR service API 继续作为插件调用的标准接口：

- `GET /health`
- `GET /model/status`
- `POST /model/load`
- `POST /transcribe`
- `POST /shutdown`

Companion 后续可以暴露自己的本地控制 API，但 v0.2.0 MVP 不要求插件依赖新的 Companion-only HTTP API。discovery 文件加现有 ASR API 足够支撑第一版。

## 11. 进程模型

```text
EchoNote Companion.app
  Tauri shell
  Rust process manager
  Web UI
  Child process:
    python -m echonote_asr --host 127.0.0.1 --port 8765 --model ... --backend ...
```

Process manager 职责：

- 校验配置路径。
- 启动 ASR service 子进程。
- 停止 ASR service 子进程。
- 重启 ASR service 子进程。
- 轮询 `/health`。
- 轮询 `/model/status`。
- 持久化 discovery 状态。
- 捕获 stdout/stderr 日志。
- 检测子进程异常退出。

## 12. 隐私与安全

- ASR service 必须只监听 `127.0.0.1`。
- Companion 不上传音频、转录文本、日志或诊断报告。
- 诊断报告必须脱敏。
- Companion 不暴露非 localhost 的未认证网络端口。
- 插件继续说明：如果用户使用云端 LLM Provider 总结，Transcript 会发送给对应 LLM 服务。

## 13. 验收标准

### 13.1 Companion

- 用户可以在 macOS 上启动 Tauri Companion。
- 用户可以在 Companion 中启动现有 ASR service，无需打开终端。
- Companion 展示 service 状态和 model 状态。
- Companion 写入 `companion.json`，包含当前可用 ASR API URL。
- Companion 捕获 ASR service 日志。
- 用户可以复制诊断报告。
- 用户可以停止和重启 ASR service。

### 13.2 插件集成

- `Auto` 模式下，插件优先使用 Companion discovery。
- `Auto` 模式下，Companion 不可用时插件回退到手动配置。
- `Companion` 模式下，Companion 不可用时插件展示明确错误。
- `Manual` 模式下，v0.1.0 的 ASR 启动和调用流程仍可用。
- 会议录制、Transcript 写入和总结流程不回退。

### 13.3 验证

插件：

```bash
cd plugin
npm run typecheck
npm run build
```

ASR service：

```bash
cd asr-service
.venv/bin/python -m unittest discover -s tests
```

Companion：

```bash
cd companion
npm run tauri build
```

端到端 smoke test：

1. Companion 使用 `fake` backend 启动 ASR service。
2. Obsidian 插件选择 `Auto` runtime mode。
3. 开始一次会议。
4. 确认 Transcript 收到 fake ASR 分段。
5. 切换到 `Manual` runtime mode。
6. 确认插件仍可使用直接 ASR service 配置。

## 14. 建议里程碑

### M1：Companion 工程骨架

- 新增 `companion/` Tauri 工程。
- 构建静态主窗口。
- 设置文件写入 Application Support。

### M2：服务进程管理

- 启动、停止、重启 ASR service。
- 捕获 stdout/stderr。
- 轮询 health 和 model status。

### M3：Discovery 与插件 runtime mode

- Companion 写入 `companion.json`。
- 插件新增 ASR runtime mode。
- 插件读取 discovery 文件。
- 保留 manual fallback。

### M4：诊断能力

- Companion 增加最近日志 UI。
- Companion 增加 `Copy Diagnostic Report`。
- 插件状态面板增加 Companion 状态行。

### M5：发布准备

- 文档补充 Companion 安装步骤。
- 可行时将 Companion build 加入 CI。
- 更新 `CHANGELOG.md`、`versions.json` 和 release assets。

## 15. 开放问题

- v0.2.0 是否发布 Companion 的二进制安装包，还是先只发布源码和手动构建说明？
- 模型加载应由 Companion 主动负责，还是仍由插件在开始会议前调用 `/model/load`？
- 菜单栏常驻模式是否进入 v0.2.0，还是延后到 v0.3.0？
- `companion.json` 是每次状态轮询都写入，还是只在状态变化时写入？
- Tauri app 的最低 macOS 版本应定为多少？
- 应用正式名称使用 `EchoNote Companion`、`EchoNote ASR` 还是 `EchoNote Local ASR`？
