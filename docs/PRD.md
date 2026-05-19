# EchoNote MVP 需求文档

## 1. 文档信息

- 产品名称：EchoNote
- 产品形态：Obsidian 桌面端插件
- MVP 平台：macOS
- 当前版本：v0.1
- 文档状态：需求确认稿

## 2. 产品概述

EchoNote 是一个面向 Obsidian 用户的会议纪要插件。它可以在 macOS 上录制会议语音，使用本地 MLX ASR 模型进行准实时分段转录，将转录内容保存为 Obsidian Markdown 笔记，并支持通过 LLM 一键生成结构化会议总结。

MVP 版本聚焦个人知识管理场景，强调本地转录、隐私可控和 Obsidian 原生工作流，不做完整会议平台、团队协作系统或跨平台语音产品。

## 3. 产品定位

### 3.1 一句话描述

EchoNote 是一个为 Obsidian 打造的本地会议转录与 AI 纪要插件。

### 3.2 目标用户

- 使用 Obsidian 管理知识库、项目资料或工作笔记的用户。
- 经常需要记录会议、访谈、讨论、课程或播客素材的用户。
- 希望会议语音优先在本机转录、降低云端 ASR 依赖的用户。
- 希望会议记录以 Markdown 形式长期沉淀、检索、链接和复盘的用户。

### 3.3 核心价值

- 将会议语音直接转化为 Obsidian 中的 Markdown 会议笔记。
- 默认使用本地 ASR 模型，会议音频不上传到云端转录服务。
- 降低会议中手动记录和会后整理的成本。
- 一键生成会议摘要、决策事项、行动项、关键讨论点和开放问题。
- 原始转录与 AI 总结保存在同一篇笔记中，便于追溯。

## 4. MVP 目标

EchoNote MVP 需要验证以下关键假设：

- Obsidian 插件可以一键启动并管理本地 ASR 服务。
- macOS 上的本地 MLX ASR 模型可以提供可接受的准实时分段转录体验。
- 插件可以稳定创建会议笔记，并在录音过程中持续追加转录内容。
- 用户可以通过 OpenAI-compatible 或 Anthropic Provider 生成可用的结构化会议总结。
- 插件可以清晰展示录音权限、ASR 服务状态和模型加载状态，降低本地模型方案的使用门槛。

## 5. MVP 范围

### 5.1 本期包含

- 仅支持 macOS。
- Apple Silicon Mac 作为主要目标环境。
- Obsidian 插件内一键启动本地 ASR 服务。
- 使用本地 MLX ASR 模型完成语音转录。
- 默认 ASR 模型：`mlx-community/Qwen3-ASR-0.6B-4bit`。
- 支持用户选择 ASR 模型。
- 预置可选模型：`mlx-community/Qwen3-ASR-1.7B-4bit`。
- 预留自定义 MLX 模型 ID 配置。
- 准实时分段转录。
- 自动新建会议笔记。
- 转录内容按时间戳追加写入会议笔记。
- 默认不保存原始音频。
- 支持通过配置选择保存原始音频。
- 支持选择音频输入设备。
- 如需同时录制麦克风和会议软件输出，用户可选择 BlackHole、Loopback 等虚拟混音输入设备。
- 支持 LLM 一键总结会议内容。
- LLM Provider 支持：
  - OpenAI-compatible API。
  - Anthropic API。
- 提供专门状态面板，展示：
  - 录音权限状态。
  - ASR 服务状态。
  - 模型加载状态。
  - 录音状态。
  - 当前会议笔记。
  - 待处理音频分段数量。
- 提供基础设置页。

### 5.2 本期不包含

- Windows 支持。
- Linux 支持。
- Obsidian 移动端支持。
- `whisper.cpp` 实现。
- 云端 ASR Provider。
- 发言人分离。
- 声纹识别。
- Zoom、Google Meet、Teams、飞书会议等会议平台集成。
- 日历集成。
- 团队协作。
- 共享会议空间。
- 会议知识图谱。
- 自动链接已有 Obsidian 笔记。

## 6. 核心用户流程

1. 用户在 macOS 上打开 Obsidian。
2. 用户点击 EchoNote Ribbon 图标，或执行命令 `EchoNote: Start Meeting`。
3. EchoNote 打开或更新状态面板。
4. EchoNote 检查当前环境和状态：
   - 当前平台是否为 macOS。
   - 麦克风权限是否已授予。
   - 本地 ASR 服务是否正在运行。
   - 当前选择的 ASR 模型是否已加载。
5. 如果 ASR 服务未启动，EchoNote 自动启动本地 ASR 服务。
6. ASR 服务加载用户选择的 MLX ASR 模型。
7. EchoNote 在配置的会议目录下自动新建会议笔记。
8. EchoNote 开始录音。
9. EchoNote 按固定时长切分音频。
10. EchoNote 将音频分段发送到本地 ASR 服务。
11. ASR 服务返回当前分段的转录文本。
12. EchoNote 将转录文本追加写入会议笔记的 `## Transcript` 区域。
13. 用户停止会议。
14. 用户点击 `Summarize Meeting`。
15. EchoNote 将会议转录内容发送给配置的 LLM Provider。
16. EchoNote 将生成的会议总结写入会议笔记的固定章节。

## 7. Obsidian 插件入口

### 7.1 Ribbon 图标

EchoNote 需要提供 Obsidian Ribbon 图标。

点击 Ribbon 图标后，默认打开 EchoNote 状态面板。状态面板中应提供开始会议、暂停录音、继续录音、停止会议、生成总结等主要操作。

### 7.2 命令面板

EchoNote 需要提供以下 Obsidian 命令：

- `EchoNote: Start Meeting`
- `EchoNote: Pause Recording`
- `EchoNote: Resume Recording`
- `EchoNote: Stop Meeting`
- `EchoNote: Summarize Current Meeting`
- `EchoNote: Open Status Panel`
- `EchoNote: Restart ASR Service`

## 8. 会议笔记需求

### 8.1 创建规则

MVP 中，EchoNote 在每次开始会议时自动新建一篇会议笔记。

MVP 不支持写入当前已打开笔记。

### 8.2 默认保存目录

默认会议笔记目录：

```text
Meetings/
```

该目录需要支持用户配置。

### 8.3 默认标题格式

默认会议标题格式：

```text
YYYY-MM-DD HH-mm Meeting
```

标题格式需要支持用户配置。

### 8.4 默认会议笔记模板

```markdown
# {{meeting_title}}

- Date: {{date}}
- Time: {{start_time}} - {{end_time}}
- Platform: EchoNote
- ASR Model: {{asr_model}}
- LLM Provider: {{llm_provider}}
- Tags: #meeting #echonote

## Summary

_Pending._

## Decisions

_Pending._

## Action Items

_Pending._

## Key Points

_Pending._

## Open Questions

_Pending._

## Transcript
```

### 8.5 转录格式

转录内容追加到 `## Transcript` 章节下。

默认每段转录包含时间戳：

```markdown
[10:03:12] 我们今天主要讨论 EchoNote 第一版的范围。
[10:03:28] 第一版只针对 macOS，使用本地 MLX ASR 模型。
```

时间戳默认开启，并可在设置中关闭。

## 9. ASR 转录需求

### 9.1 架构原则

Obsidian 插件不直接在插件运行时中执行 ASR 模型推理。

EchoNote 使用本地 ASR 服务完成模型加载和音频转录：

```text
Obsidian Plugin
  -> 启动本地 ASR 进程
  -> 检查服务健康状态
  -> 发送音频分段
  -> 接收转录文本
  -> 写入 Markdown 笔记

Local ASR Service
  -> Python 服务
  -> MLX Runtime
  -> 用户选择的 Qwen3 ASR 模型
  -> 音频分段转录接口
```

### 9.2 服务启动

ASR 服务必须支持由 Obsidian 插件一键启动。

MVP 需要支持以下配置：

- Python 路径。
- ASR 服务目录。
- ASR 服务端口。
- 是否在开始会议时自动启动 ASR 服务。

### 9.3 ASR 模型策略

默认模型：

```text
mlx-community/Qwen3-ASR-0.6B-4bit
```

默认使用 0.6B 4bit 模型的原因：

- 资源消耗更低。
- 启动和加载压力更小。
- 更适合普通 Mac 用户作为默认体验。
- 有助于降低本地模型方案的首次使用门槛。

预置可选模型：

```text
mlx-community/Qwen3-ASR-1.7B-4bit
```

设置页需要预留自定义 MLX 模型 ID 字段。

用户切换模型后，需要提示重启 ASR 服务。

### 9.4 转录模式

EchoNote MVP 使用准实时分段转录，而不是严格流式实时转录。

默认音频分段长度：

```text
15 秒
```

可配置选项：

```text
10 秒
15 秒
30 秒
```

### 9.5 本地 ASR 服务 API

本地 ASR 服务至少需要提供以下接口：

```text
GET  /health
GET  /model/status
POST /transcribe
POST /shutdown
```

接口职责：

- `GET /health`：返回 ASR 服务是否运行。
- `GET /model/status`：返回当前模型名、加载状态、是否可用、错误信息。
- `POST /transcribe`：接收一个音频分段，返回转录文本。
- `POST /shutdown`：按插件请求关闭本地 ASR 服务。

### 9.6 ASR 错误处理

EchoNote 需要处理以下错误：

- ASR 服务启动失败。
- ASR 服务异常退出。
- 模型下载失败。
- 模型加载失败。
- 音频分段转录失败。
- ASR 服务响应超时。

错误处理原则：

- 已写入会议笔记的转录内容不得被删除或覆盖。
- 单个音频分段转录失败，不应影响后续分段继续处理。
- 关键错误需要展示在状态面板中。
- 用户应能从插件中重启 ASR 服务。

## 10. 录音与音频需求

### 10.1 录音输入

EchoNote 需要支持从用户默认麦克风录音。

EchoNote MVP 采用虚拟音频设备方案支持录制会议软件输出。插件不直接捕获系统输出音频，而是允许用户选择 macOS 音频输入设备。

插件需要支持：

- 枚举可用音频输入设备。
- 在设置页选择音频输入设备。
- 默认使用系统默认输入设备。
- 使用所选输入设备进行录音。

如果用户需要同时录制麦克风和会议软件输出，需要在 macOS 中配置 BlackHole、Loopback 等虚拟音频设备，将麦克风和会议软件输出混合成一个输入源，再在 EchoNote 中选择该虚拟输入设备。

MVP 不实现系统级音频捕获、ScreenCaptureKit 或 native helper。

### 10.2 原始音频保存策略

默认行为：

- 不保存原始音频。
- 音频分段仅用于转录处理。
- 在技术可行的情况下，转录完成后清理临时音频分段。

可选行为：

- 用户可以在设置中开启保存原始音频。
- 用户可以配置音频保存目录。

默认音频保存目录：

```text
Meetings/audio/
```

### 10.3 音频保存模式

MVP 只需要提供一个简单配置：

```text
Save raw audio: on / off
```

如果开启保存原始音频，具体保存为完整会议音频或分段音频，可由实现方案决定，但需要在界面或文档中明确告知用户。

## 11. LLM 总结需求

### 11.1 支持的 Provider

EchoNote MVP 支持两类 LLM Provider：

- OpenAI-compatible API。
- Anthropic API。

### 11.2 Provider 配置

OpenAI-compatible 配置：

- API Key。
- Base URL。
- Model。

Anthropic 配置：

- API Key。
- Model。

通用配置：

- 默认总结语言。
- 自定义总结 Prompt。
- 长转录文本处理策略。

### 11.3 总结写入规则

LLM 总结只允许更新以下章节：

- `## Summary`
- `## Decisions`
- `## Action Items`
- `## Key Points`
- `## Open Questions`

`## Transcript` 章节不得被总结功能覆盖或删除。

### 11.4 默认总结结构

```markdown
## Summary

{{summary}}

## Decisions

{{decisions}}

## Action Items

- [ ] {{task}} @{{owner}} due {{date}}

## Key Points

{{key_points}}

## Open Questions

{{open_questions}}
```

### 11.5 长会议处理

当会议转录文本超过所选 LLM 上下文窗口时，EchoNote 需要使用分段总结策略：

1. 将 Transcript 按长度拆分。
2. 对每段生成局部总结。
3. 将局部总结合并为最终会议总结。

该能力用于保障长会议总结的稳定性。

## 12. 状态面板需求

EchoNote 需要提供专门的状态面板，作为本地转录工作流的主要控制中心。

### 12.1 状态字段

状态面板需要显示：

```text
Microphone Permission: Granted / Denied / Unknown
ASR Service: Not Started / Starting / Running / Error
Model: Not Loaded / Loading / Ready / Error
Selected Model: mlx-community/Qwen3-ASR-0.6B-4bit
Recording: Idle / Recording / Paused
Current Meeting: {{meeting_note_title}}
Chunk Queue: {{pending_chunk_count}} pending
Last Transcript: {{relative_time}}
```

### 12.2 状态面板操作

状态面板需要提供以下操作：

- 请求麦克风权限。
- 启动 ASR 服务。
- 重启 ASR 服务。
- 查看模型加载错误。
- 开始会议。
- 暂停录音。
- 继续录音。
- 停止会议。
- 生成会议总结。

## 13. 设置页需求

EchoNote 设置页需要包含以下配置项。

### 13.1 会议设置

- 默认会议目录。
- 默认会议标题格式。
- 自定义会议笔记模板。
- 是否启用时间戳。

### 13.2 ASR 设置

- ASR 模型选择。
- 自定义 MLX 模型 ID。
- Python 路径。
- ASR 服务目录。
- ASR 服务端口。
- 音频分段长度。
- 是否在开始会议时自动启动 ASR 服务。

### 13.3 音频设置

- 音频输入设备。
- 刷新音频输入设备列表。
- 是否保存原始音频。
- 音频保存目录。

### 13.4 LLM 设置

- LLM Provider。
- OpenAI-compatible API Key。
- OpenAI-compatible Base URL。
- OpenAI-compatible Model。
- Anthropic API Key。
- Anthropic Model。
- 默认总结语言。
- 自定义总结 Prompt。

## 14. 隐私需求

EchoNote 需要明确向用户说明以下隐私行为：

- ASR 转录默认在本地运行。
- MVP 不会将会议音频发送给云端 ASR 服务。
- 默认不保存原始音频。
- 会议笔记保存在用户自己的 Obsidian Vault 中。
- 如果用户使用云端 LLM Provider 生成总结，Transcript 会发送给对应 Provider。
- 如果用户希望总结也保留在本地，可以配置本地 OpenAI-compatible Endpoint。

## 15. 平台需求

### 15.1 MVP 平台

- macOS only。
- Apple Silicon Mac 优先。

### 15.2 后续平台扩展

未来可以扩展以下 ASR Provider：

- `whisper.cpp`。
- 云端 ASR Provider。
- 其他本地模型 Runtime。

MVP 实现时需要保持 ASR Provider 边界清晰，但本期只实现 MLX Qwen3 ASR Provider。

## 16. 非功能需求

### 16.1 稳定性

- 已写入会议笔记的转录内容不得因后续错误丢失。
- 停止会议时，应尽可能处理或落盘已有待处理内容。
- ASR 服务异常需要在状态面板中展示。
- 用户应能从插件内重启 ASR 服务。

### 16.2 性能

- 默认模型应优先降低资源消耗。
- 默认分段长度应在延迟和识别稳定性之间取得平衡。
- 长会议应通过增量写入保存转录内容，不应只依赖内存保存。

### 16.3 易用性

- 开始会议应尽量只需要一个主要操作。
- 麦克风权限、Python 配置、ASR 服务、模型加载等问题需要清晰可见。
- 插件不应在关键状态异常时静默失败。

### 16.4 兼容性

- MVP 仅支持 Obsidian 桌面端 macOS。
- MVP 不支持 Obsidian 移动端。

## 17. 验收标准

EchoNote MVP 满足以下条件时视为完成：

- 用户可以在 macOS Obsidian 中启动一次会议记录。
- EchoNote 可以从插件内启动本地 ASR 服务。
- 状态面板可以显示麦克风权限、ASR 服务状态、模型状态和录音状态。
- 默认 ASR 模型为 `mlx-community/Qwen3-ASR-0.6B-4bit`。
- 用户可以选择 `mlx-community/Qwen3-ASR-1.7B-4bit`。
- EchoNote 可以自动创建新的会议笔记。
- 录音过程中，转录内容可以持续追加到 `## Transcript` 章节。
- 转录内容默认包含时间戳。
- 默认不保存原始音频。
- 用户可以在设置中开启保存原始音频。
- 用户可以选择音频输入设备。
- 通过虚拟音频设备配置后，EchoNote 可以录制会议软件输出和麦克风混音。
- 用户可以使用 OpenAI-compatible Provider 生成会议总结。
- 用户可以使用 Anthropic Provider 生成会议总结。
- 总结功能只更新总结相关章节。
- 总结功能不会覆盖原始 Transcript。
- ASR 服务启动失败或模型加载失败时，用户可以在状态面板看到错误。

## 18. 后续版本方向

### 18.1 v0.2 候选能力

- 更完善的模型安装和依赖检查引导。
- 一键检查 Python、MLX、模型文件和端口占用。
- 更稳健的长会议总结。
- 自定义总结模板。
- 本地 LLM Endpoint 预设。
- 更完整的音频设备选择。

### 18.2 v0.3 候选能力

- 发言人分离。
- `whisper.cpp` Provider。
- 日历事件导入。
- 自动生成会议标题。
- 自动建议链接到已有 Obsidian 笔记。

### 18.3 v1.0 候选能力

- 跨平台 ASR Provider 支持。
- 会议平台集成。
- 团队协作工作流。
- 会议复盘和后续跟进自动化。
