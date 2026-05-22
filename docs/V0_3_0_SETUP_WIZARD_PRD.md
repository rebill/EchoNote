# EchoNote v0.3.0 需求文档：新手向导式安装与配置

## 1. 文档信息

- 产品名称：EchoNote
- 目标版本：v0.3.0
- 版本主题：新手向导式 ASR 安装与配置
- 目标平台：macOS
- 主要客户端：EchoNote 桌面应用 + Obsidian Desktop 插件
- 技术路线：Tauri + 现有 Python ASR Service + 本地自动检测与修复流程
- 文档状态：需求草案

## 2. 背景

EchoNote v0.2.0 已将 ASR runtime 管理从 Obsidian 插件中抽离到独立 macOS 桌面应用。用户可以通过桌面应用启动、停止、诊断本地 ASR 服务，插件通过 discovery 文件发现当前可用的本地 ASR endpoint。

当前桌面应用的设置面板仍暴露了偏开发者的配置项：

- Python 可执行文件路径。
- ASR service 工程路径。
- 本地服务端口。
- Backend：`fake` 或 `mlx-audio`。
- 模型预设和自定义模型 ID。

这些配置对开发和调试有价值，但对新手用户过于复杂。用户需要理解 Python 虚拟环境、项目目录、端口占用、MLX backend 和模型 ID，才能完成首次启动。这与 EchoNote 的产品目标相冲突：本地优先，但不要求用户成为本地 ASR runtime 专家。

v0.3.0 需要将默认体验从“配置表单”改成“向导式安装与自动修复”。普通用户只需要打开 EchoNote，点击一个主按钮，应用负责检测、安装、修复、启动和验证本地 ASR runtime。原有配置项保留，但收纳到高级设置中。

## 3. 一句话描述

EchoNote v0.3.0 提供一个面向新手的 ASR 设置向导，让用户通过一次点击完成本地 ASR runtime 的检测、安装、修复、启动和验证。

## 4. 目标

### 4.1 产品目标

- 降低首次使用门槛，让非开发者用户不需要手动填写 Python 路径、ASR service 路径、端口和模型 ID。
- 将默认操作从“保存设置”改为“设置 EchoNote”或“修复 EchoNote”。
- 在桌面应用中清晰展示当前 ASR runtime 是否可用，以及下一步用户应该做什么。
- 保留高级配置能力，支持开发者、内测用户和故障排查。
- 保持 Obsidian 插件侧体验简单：用户只需要看到 Companion 是否 available，不需要理解底层安装细节。

### 4.2 工程目标

- 在桌面应用中新增 setup 状态模型，用于描述本地 runtime 的检测、安装、修复和验证结果。
- 新增 Tauri commands 支持自动检测和一键设置流程。
- 将当前设置面板改为高级设置折叠区，默认隐藏。
- 默认配置应能覆盖大多数本地开发安装场景和后续打包安装场景。
- 保持现有 ASR HTTP API、discovery 文件和 Obsidian 插件 runtime resolver 兼容。

## 5. 非目标

v0.3.0 不包含：

- 内置完整 Python runtime。
- 内置或预下载模型权重。
- macOS 签名、公证和自动更新。
- Windows 或 Linux 支持。
- 云端 ASR provider。
- 自动配置 BlackHole、Loopback 或系统音频路由。
- 完整安装器 `.pkg` 或 `.dmg` 体验。
- 重写 ASR service。
- 删除高级设置。

这些能力可在向导式设置稳定后继续规划。

## 6. 目标用户

- 第一次安装 EchoNote 的 Obsidian 用户。
- 不熟悉 Python、虚拟环境和本地端口的用户。
- 希望使用本地 ASR，但不想打开终端的用户。
- 已经安装过 EchoNote，但 ASR runtime 出错，需要一键修复的用户。
- 开发者和内测用户，他们仍需要高级设置和日志诊断能力。

## 7. 核心用户流程

### 7.1 首次设置

1. 用户安装并打开 EchoNote 桌面应用。
2. 应用自动执行环境检测。
3. 应用显示当前状态：
   - `Ready`
   - `Setup required`
   - `Repair required`
   - `Unsupported`
   - `Error`
4. 如果需要设置，用户点击 `Set Up EchoNote`。
5. 应用按步骤执行：
   - 检查 macOS 和 CPU 架构。
   - 检查 Python 3.11+。
   - 定位或创建 ASR service 虚拟环境。
   - 安装或验证 ASR service 依赖。
   - 检查默认端口是否可用。
   - 选择默认 backend 和模型预设。
   - 启动 ASR service。
   - 调用 `/health` 验证服务。
   - 调用 `/model/status` 验证模型状态。
   - 写入 discovery 文件。
6. 设置成功后，应用显示 `Ready`。
7. 用户打开 Obsidian，EchoNote 插件显示 Companion status 为 `available`。

### 7.2 日常启动

1. 用户打开 EchoNote 桌面应用。
2. 如果环境已配置，应用显示 `Ready` 或 `Service stopped`。
3. 用户点击 `Start Service`。
4. 应用启动 ASR service 并刷新 discovery 文件。
5. 用户在 Obsidian 中开始会议。

### 7.3 自动修复

如果检测到 runtime 配置不完整或失效：

1. 应用显示 `Repair required`。
2. 用户点击 `Repair EchoNote`。
3. 应用只修复失败项：
   - Python 路径失效时重新查找 Python。
   - 虚拟环境缺失时重新创建。
   - 依赖缺失时重新安装。
   - 端口占用时提示切换端口或停止占用进程。
   - ASR service 路径缺失时尝试使用默认内置路径或提示用户选择。
4. 修复完成后重新验证 `/health` 和 discovery。

### 7.4 高级配置

1. 用户点击 `Advanced Settings`。
2. 应用展开高级表单。
3. 用户可以手动修改：
   - Python path。
   - ASR service path。
   - Port。
   - Backend。
   - Model preset。
   - Custom model ID。
4. 用户保存后，应用重新执行 runtime 验证。

高级设置默认折叠，普通用户不需要进入。

## 8. MVP 范围

### 8.1 默认主界面

桌面应用默认首屏应以 runtime 状态和主操作为中心。

必须展示：

- 应用名：`EchoNote`。
- ASR runtime 当前状态。
- 当前模型名称或模型预设。
- 本地服务状态。
- Obsidian 连接提示。
- 最近一次检测或修复结果。

主操作按钮根据状态动态变化：

| 状态 | 主按钮 | 说明 |
| --- | --- | --- |
| `not_configured` | `Set Up EchoNote` | 首次安装或缺少关键配置。 |
| `ready` | `Start Service` | runtime 已可用但服务未启动。 |
| `running` | `Stop Service` | 服务正在运行。 |
| `repair_required` | `Repair EchoNote` | 发现可自动修复的问题。 |
| `unsupported` | 禁用 | 当前环境不支持。 |
| `error` | `Retry` | 上次检测或设置失败。 |

次要操作：

- `Copy Diagnostic Report`
- `Open Logs Folder`
- `Advanced Settings`

### 8.2 设置向导

设置向导可以是单页进度列表，也可以是分步骤流程。MVP 推荐单页进度列表，减少跳转复杂度。

必须包含以下步骤：

1. `Check System`
2. `Find Python`
3. `Prepare ASR Runtime`
4. `Install Dependencies`
5. `Check Port`
6. `Start Service`
7. `Verify Model`
8. `Connect Obsidian`

每一步状态：

- `pending`
- `running`
- `passed`
- `warning`
- `failed`
- `skipped`

每一步应有一句用户可理解的说明，不展示完整命令输出。详细日志保留在日志面板和诊断报告中。

### 8.3 自动检测

应用打开后应自动执行轻量检测，不应立即安装依赖或修改文件。

检测项：

- macOS 版本。
- CPU 架构。
- Python 是否存在。
- Python 版本是否满足 `>=3.11`。
- ASR service 路径是否存在。
- 虚拟环境是否存在。
- ASR service 是否可 import 或启动。
- 默认端口是否可用。
- 当前 discovery 文件是否存在且新鲜。
- 当前 ASR service 是否已经运行。

检测结果必须可序列化并显示在 UI 中。

### 8.4 自动安装与修复

用户点击 `Set Up EchoNote` 或 `Repair EchoNote` 后，应用才允许执行会修改本机环境的操作。

允许执行：

- 创建虚拟环境。
- 安装或更新 ASR service 依赖。
- 写入 EchoNote 设置文件。
- 写入 discovery 文件。
- 启动 ASR service。

不允许静默执行：

- 删除用户自定义目录。
- 覆盖用户指定的 Python。
- 更换用户选择的模型 ID。
- 终止非 EchoNote 管理的进程。
- 修改系统音频设置。

如果需要执行风险较高的操作，必须展示明确确认。

### 8.5 高级设置

当前设置表单保留，但移动到 `Advanced Settings` 折叠区。

高级设置包括：

- Python path。
- ASR service path。
- Port。
- Backend。
- Model preset。
- Custom model ID。

高级设置下方保留：

- Settings file path。
- `Save Settings`。
- `Reset to Defaults`。

默认情况下，用户只看到高级设置的折叠入口，不看到这些字段。

### 8.6 错误恢复

错误信息必须分层展示：

- 主界面展示用户可理解的错误摘要。
- 每一步展示失败原因。
- 诊断报告包含完整技术细节。

常见错误和建议：

| 错误 | 用户提示 | 建议操作 |
| --- | --- | --- |
| Python 不存在 | EchoNote could not find Python 3.11+. | 安装 Python 或在高级设置中选择 Python。 |
| Python 版本过低 | Python 3.11 or newer is required. | 升级 Python。 |
| 虚拟环境创建失败 | EchoNote could not prepare the ASR runtime. | 复制诊断报告。 |
| 依赖安装失败 | EchoNote could not install ASR dependencies. | 检查网络或复制诊断报告。 |
| 端口占用 | Port 8765 is already in use. | 自动选择可用端口或在高级设置中修改。 |
| 服务启动失败 | EchoNote could not start the ASR service. | 查看日志或修复 runtime。 |
| 模型加载失败 | EchoNote could not load the selected ASR model. | 切换模型或复制诊断报告。 |

## 9. 状态模型

### 9.1 Setup status

建议新增 `SetupStatus`：

```ts
type SetupStatus =
  | "unknown"
  | "checking"
  | "not_configured"
  | "ready"
  | "running"
  | "repair_required"
  | "installing"
  | "unsupported"
  | "error";
```

### 9.2 Setup step

建议新增 `SetupStep`：

```ts
type SetupStep = {
  id:
    | "system"
    | "python"
    | "runtime"
    | "dependencies"
    | "port"
    | "service"
    | "model"
    | "obsidian";
  label: string;
  status: "pending" | "running" | "passed" | "warning" | "failed" | "skipped";
  summary: string;
  detail?: string;
  recoverable: boolean;
};
```

### 9.3 Setup response

建议 Tauri commands 返回统一结构：

```ts
type SetupResponse = {
  status: SetupStatus;
  steps: SetupStep[];
  settings: CompanionSettings;
  state: CompanionAppState;
  primaryAction: "setup" | "repair" | "start" | "stop" | "retry" | "none";
  message: string;
};
```

## 10. Tauri Command 需求

### 10.1 `detect_setup`

用途：轻量检测当前环境，不修改文件。

返回：`SetupResponse`。

触发：

- 应用启动。
- 用户点击 `Refresh`。
- 保存高级设置后。

### 10.2 `install_or_repair_runtime`

用途：执行首次设置或自动修复。

行为：

- 可创建虚拟环境。
- 可安装依赖。
- 可更新设置。
- 可启动服务并验证。

返回：`SetupResponse`。

### 10.3 `start_service_with_defaults`

用途：在 runtime ready 后按默认配置启动服务。

返回：`CompanionAppState` 或 `SetupResponse`。

### 10.4 `reset_setup`

用途：重置 EchoNote 设置为默认值。

限制：

- 不删除虚拟环境。
- 不删除日志。
- 不删除用户选择的 ASR service 源码目录。

## 11. UI 文案

### 11.1 主状态文案

- `Ready`: EchoNote is ready.
- `Setup required`: Set up EchoNote to use local transcription.
- `Repair required`: EchoNote found an issue it can repair.
- `Running`: Local transcription is running.
- `Unsupported`: This Mac is not supported.
- `Error`: EchoNote could not complete setup.

### 11.2 主按钮文案

- `Set Up EchoNote`
- `Repair EchoNote`
- `Start Service`
- `Stop Service`
- `Retry`

### 11.3 高级设置入口

- `Advanced Settings`
- 说明：`For custom Python, ports, backend, and model settings. Most users do not need this.`

## 12. Obsidian 插件影响

Obsidian 插件不需要承担安装和修复职责。

插件侧需要调整的内容：

- 错误提示从“打开 Companion”更新为“打开 EchoNote”。
- 当 discovery 不可用时，提示用户在 EchoNote 桌面应用中点击 `Set Up EchoNote` 或 `Start Service`。
- 状态面板继续展示 Companion status，但用户文案应称为 EchoNote desktop app。

插件不需要知道 setup 每一步细节。

## 13. 数据与文件

继续使用现有路径：

```text
~/Library/Application Support/EchoNote/companion-settings.json
~/Library/Application Support/EchoNote/companion.json
~/Library/Logs/EchoNote/companion.log
~/Library/Logs/EchoNote/asr-service.log
```

设置文件中可新增字段：

```ts
type CompanionSettings = {
  setupCompletedAt?: string;
  setupVersion?: string;
  autoRepairEnabled?: boolean;
};
```

MVP 不要求迁移旧路径。旧用户升级后，应用应读取现有设置并显示 `Ready` 或 `Repair required`。

## 14. 验收标准

### 14.1 新手首次使用

- 用户首次打开 EchoNote 时，不会看到 Python path、ASR service path、port、backend、custom model ID 等表单字段。
- 用户可以点击 `Set Up EchoNote` 完成默认 runtime 设置。
- 设置成功后主状态显示 `Ready`。
- 用户点击 `Start Service` 后服务启动，discovery 文件写入成功。
- Obsidian 插件能发现服务并开始会议。

### 14.2 高级设置

- 用户点击 `Advanced Settings` 后可以看到并修改原有设置项。
- 保存高级设置后，应用会重新检测 runtime。
- 高级设置可以折叠回默认视图。

### 14.3 错误恢复

- Python 缺失、端口占用、依赖安装失败、服务启动失败均有明确用户提示。
- 用户可以复制诊断报告。
- 错误状态下不会静默覆盖用户配置。

### 14.4 回归

- `fake` backend 仍可用于 smoke test。
- `mlx-audio` backend 仍可作为默认真实 ASR backend。
- 现有 discovery schema 兼容插件 runtime resolver。
- 现有启动、停止、重启、加载模型能力不退化。

## 15. 发布说明要求

v0.3.0 release notes 需要说明：

- EchoNote 桌面应用新增新手向导式设置。
- 普通用户不再需要手动配置 Python path、ASR service path、port、backend 和 model ID。
- 原高级配置仍然保留。
- 如果自动设置失败，用户可以复制诊断报告提交 issue。

## 16. 待确认问题

- v0.3.0 是否允许自动执行 `pip install -e '.[mlx]'`，还是只生成命令并让用户确认？
- 默认是否优先使用仓库内 `asr-service/.venv/bin/python`，还是优先查找系统 Python？
- 端口占用时是否自动选择新端口，还是必须让用户确认？
- 后续正式打包时，ASR service 源码目录是否随 app bundle 提供，还是继续要求用户从源码仓库运行？
- 是否需要在首屏区分“开发者源码模式”和“用户安装模式”？
