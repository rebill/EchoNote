# EchoNote MVP 交付计划

## 1. 文档信息

- 产品名称：EchoNote
- 对应需求文档：[docs/PRD.md](./PRD.md)
- 对应技术设计：[docs/TECH_DESIGN.md](./TECH_DESIGN.md)
- 交付范围：MVP v0.1
- 文档目标：定义 Milestone 前后依赖、用户故事、子任务和严格验收标准

## 2. 交付原则

- 先冻结模块接口，再并行实现。
- 插件代码和 ASR 服务代码必须拆分为独立顶层目录。
- 先用假 ASR 跑通端到端链路，再接真实模型。
- 录音、转录、笔记写入、总结生成都必须可独立验证。
- 已写入会议笔记的内容不得因后续错误丢失。
- 每个 Milestone 必须有可演示、可测试、可验收的交付物。

## 3. Milestone 总览

| Milestone | 名称 | 目标 | 是否可并行 |
| --- | --- | --- | --- |
| M0 | 接口契约冻结 | 固定核心类型、API、状态机和目录结构 | 必须最先完成 |
| M1 | Obsidian 插件骨架 | 插件可加载，设置页、命令、状态面板可用 | M0 后可启动 |
| M2 | 本地 ASR 服务骨架 | Python 服务可启动，假转录 API 可联调 | M0 后可启动 |
| M3 | 真实 ASR Spike | 跑通 Qwen3 ASR 单段音频真实转录 | M0 后可与 M2 并行 |
| M4 | 录音、分段、转录队列和笔记写入 | 插件端完成会议记录主链路 | 依赖 M1 和 M2 |
| M5 | LLM 总结 | OpenAI-compatible 和 Anthropic 总结可写入笔记 | 依赖 M0，部分依赖 M1 |
| M6 | 端到端集成 | 真实 ASR、录音、笔记、总结完整闭环 | 依赖 M3、M4、M5 |
| M7 | 稳定性、打包和文档 | 错误处理、验收测试、使用文档 | 依赖 M6 |

## 4. 前后依赖

### 4.1 依赖图

```text
M0 接口契约冻结
  ├─ M1 Obsidian 插件骨架
  │    ├─ M4 录音、分段、转录队列和笔记写入
  │    ├─ M5 LLM 总结
  │    └─ M6 端到端集成
  │
  ├─ M2 本地 ASR 服务骨架
  │    ├─ M4 录音、分段、转录队列和笔记写入
  │    └─ M6 端到端集成
  │
  ├─ M3 真实 ASR Spike
  │    └─ M6 端到端集成
  │
  └─ M5 LLM 总结
       └─ M6 端到端集成

M6 端到端集成
  └─ M7 稳定性、打包和文档
```

### 4.2 强依赖

- M0 是所有开发任务的强依赖。
- M4 强依赖 M1 和 M2，因为它需要插件运行环境和可调用的 ASR API。
- M6 强依赖 M3、M4、M5，因为它需要真实 ASR、会议主链路和总结能力。
- M7 强依赖 M6，因为稳定性和打包必须基于完整链路验证。

### 4.3 弱依赖

- M3 不强依赖 M2 的完整服务实现，可以先用命令行脚本验证模型。
- M5 不强依赖真实 ASR，可以使用 fixture transcript 开发和测试。
- M1 的状态面板可以先用 mock 状态开发，不需要等待 ASR 服务完成。

## 5. 并行开发策略

M0 完成后，可以开启三条并行工作线：

### 5.1 插件主线

包含：

- M1 Obsidian 插件骨架。
- M4 插件端录音、分段、队列、笔记写入。
- M5 中的设置页集成和总结命令接入。

### 5.2 ASR 主线

包含：

- M2 本地 ASR 服务骨架。
- M3 真实 ASR Spike。
- M6 中真实 ASR 服务替换假转录。

### 5.3 LLM 主线

包含：

- M5 OpenAI-compatible Provider。
- M5 Anthropic Provider。
- Markdown summary section 替换。
- 长 transcript 分段总结。

## 6. M0：接口契约冻结

### 6.1 用户故事

作为开发者，我需要在正式开发前固定插件端、本地 ASR 服务和 LLM 总结模块之间的接口契约，以便多个模块可以并行开发而不互相阻塞。

### 6.2 子任务

- 定义 `EchoNoteSettings` 默认字段和默认值。
- 定义 `EchoNoteStatus` 状态字段。
- 定义 `EchoNoteError` 错误码。
- 定义 ASR HTTP API：
  - `GET /health`
  - `GET /model/status`
  - `POST /model/load`
  - `POST /transcribe`
  - `POST /shutdown`
- 定义 `TranscriptSegment` 数据结构。
- 定义 `AudioChunk` 数据结构。
- 定义 `MeetingSummary` 数据结构。
- 定义 `LlmProvider` 接口。
- 确认项目目录结构：
  - `plugin/`：Obsidian 插件代码目录。
  - `asr-service/`：本地 ASR 服务代码目录。

### 6.3 验收标准

- 仓库中存在基础类型定义或接口文档，且字段名、类型和默认值明确。
- 插件代码目录和 ASR 服务代码目录明确拆分，不在仓库根目录直接放业务源码。
- Obsidian 插件相关源码、构建配置和 `manifest.json` 位于 `plugin/`。
- Python ASR 服务源码和 `pyproject.toml` 位于 `asr-service/`。
- ASR API 的请求和响应结构明确到字段级别。
- `TranscriptSegment` 包含 `chunk_id`、`text`、`started_at_ms`、`ended_at_ms`、`model_id`。
- `MeetingSummary` 至少包含 `summary`、`decisions`、`actionItems`、`keyPoints`、`openQuestions`。
- 后续 Milestone 不允许随意改动已冻结接口；如需改动，必须同步更新 PRD、技术设计和交付计划。

## 7. M1：Obsidian 插件骨架

### 7.1 用户故事

作为 Obsidian 用户，我需要能够安装并启用 EchoNote 插件，看到 EchoNote 的入口、设置页和状态面板，以确认插件已经正常加载。

### 7.2 子任务

- 初始化 Obsidian 插件工程。
- 在 `plugin/` 下创建 `manifest.json`、`package.json`、`tsconfig.json` 和构建配置。
- 实现 `plugin/src/main.ts` 插件生命周期。
- 注册 Ribbon 图标。
- 注册命令：
  - `EchoNote: Start Meeting`
  - `EchoNote: Pause Recording`
  - `EchoNote: Resume Recording`
  - `EchoNote: Stop Meeting`
  - `EchoNote: Summarize Current Meeting`
  - `EchoNote: Open Status Panel`
  - `EchoNote: Restart ASR Service`
- 实现设置读取、保存和默认值合并。
- 实现基础设置页。
- 实现状态面板 View 空壳。
- 实现 `StatusStore`，支持状态订阅和更新。

### 7.3 验收标准

- 插件可以在 Obsidian Desktop macOS 中加载，无启动报错。
- 插件工程位于 `plugin/`，不依赖仓库根目录作为插件代码目录。
- Ribbon 中显示 EchoNote 图标。
- 命令面板中可以搜索到所有 EchoNote 命令。
- 点击 Ribbon 或执行 `EchoNote: Open Status Panel` 可以打开状态面板。
- 设置页可以显示并保存以下配置：
  - 会议目录。
  - ASR 模型选择。
  - Python 路径。
  - ASR 服务目录。
  - ASR 服务端口。
  - 音频分段长度。
  - 音频输入设备。
  - 是否保存原始音频。
  - LLM Provider。
- 修改设置后，重启 Obsidian 仍能读取已保存配置。
- 状态面板至少显示麦克风权限、ASR 服务、模型、录音状态四类状态。

## 8. M2：本地 ASR 服务骨架

### 8.1 用户故事

作为开发者，我需要一个可由插件启动的本地 ASR 服务，即使真实模型尚未接入，也能通过假转录响应完成插件端联调。

### 8.2 子任务

- 初始化 `asr-service/` Python 项目。
- 创建 FastAPI 服务。
- 实现 CLI 参数：
  - `--host`
  - `--port`
  - `--model`
  - `--log-level`
- 实现 `GET /health`。
- 实现 `GET /model/status`。
- 实现 `POST /model/load`。
- 实现 `POST /transcribe` 假转录。
- 实现 `POST /shutdown`。
- 实现模型状态机：
  - `not_loaded`
  - `loading`
  - `ready`
  - `error`
- 实现 WAV 文件基础校验。
- 输出结构化日志。

### 8.3 验收标准

- 可以用命令启动 ASR 服务，并指定端口和模型 ID。
- ASR 服务工程位于 `asr-service/`，不与 Obsidian 插件源码混放。
- `GET /health` 返回 HTTP 200 和 `status: ok`。
- `GET /model/status` 返回当前模型 ID 和合法状态值。
- `POST /model/load` 可以将模型状态从 `not_loaded` 推进到 `ready`，假实现允许在 1 秒内完成。
- `POST /transcribe` 可以接收 `multipart/form-data` WAV 文件并返回合法 `TranscriptSegment`。
- 假转录返回文本必须包含 `chunk_id`，便于插件端确认顺序。
- `POST /shutdown` 可以关闭服务进程。
- 端口被占用时，服务启动失败并输出可读错误。

## 9. M3：真实 ASR Spike

### 9.1 用户故事

作为开发者，我需要确认 `mlx-community/Qwen3-ASR-0.6B-4bit` 能在目标 Mac 上完成单段音频转录，并明确模型输入格式、加载方式和性能边界。

### 9.2 子任务

- 确认 MLX Qwen3 ASR 的安装依赖。
- 编写本地命令行 Spike 脚本。
- 下载或加载 `mlx-community/Qwen3-ASR-0.6B-4bit`。
- 准备 16kHz mono WAV 测试音频。
- 跑通单段音频转录。
- 记录模型首次加载时间。
- 记录 15 秒音频分段的转录耗时。
- 记录内存占用观察值。
- 验证 `mlx-community/Qwen3-ASR-1.7B-4bit` 是否可用，作为可选模型。
- 将真实转录逻辑封装到 `Transcriber` Adapter。

### 9.3 验收标准

- 可以在命令行对一个本地 WAV 文件生成非空转录文本。
- Spike 文档或日志中记录：
  - Python 版本。
  - 关键依赖版本。
  - 模型 ID。
  - 输入音频格式。
  - 首次加载时间。
  - 单段转录耗时。
  - 主要错误和解决方式。
- `Transcriber` 提供稳定方法：
  - `load(model_id)`
  - `transcribe_wav(wav_path)`
- 真实 ASR 失败时不会影响 M2 假转录服务继续用于插件联调。

## 10. M4：录音、分段、转录队列和笔记写入

### 10.1 用户故事

作为 Obsidian 用户，我需要点击开始会议后自动创建会议笔记，并在录音过程中持续看到转录内容追加到 `## Transcript` 章节。

### 10.2 子任务

- 实现麦克风权限检查。
- 实现麦克风权限请求。
- 实现音频输入设备枚举。
- 实现音频输入设备选择。
- 实现 `AudioRecorder`。
- 实现 PCM 采集。
- 实现 16kHz mono 重采样。
- 实现 WAV 编码。
- 实现 `AudioChunker`。
- 实现分段队列。
- 实现 `AsrProcessManager`。
- 实现 `AsrServiceClient`。
- 实现 ASR 服务启动、健康检查和重启。
- 实现会议笔记自动创建。
- 实现会议模板渲染。
- 实现 transcript 追加写入。
- 实现暂停、继续、停止录音。
- 实现停止会议时 flush 当前音频缓冲。
- 实现过短音频分段和静音分段过滤，避免真实 ASR 尾段污染 Transcript。
- 实现默认不保存原始音频。
- 实现开启配置后保存原始音频。

### 10.3 验收标准

- 点击 `EchoNote: Start Meeting` 后，插件会创建一篇新的会议笔记。
- 新会议笔记路径位于配置的会议目录下。
- 新会议笔记包含 `## Summary`、`## Decisions`、`## Action Items`、`## Key Points`、`## Open Questions`、`## Transcript`。
- 录音开始后，每个音频分段会进入转录队列。
- 可以在设置页选择音频输入设备。
- 默认音频输入设备可用。
- 选择 BlackHole、Loopback 等虚拟输入设备后，录音使用该设备作为输入源。
- 使用 M2 假 ASR 服务时，转录结果会持续追加到 `## Transcript`。
- 追加的 transcript 默认包含形如 `[10:03:12]` 的时间戳。
- 暂停录音后，不再产生新的音频分段。
- 继续录音后，可以继续产生音频分段。
- 停止会议后，录音状态回到 `idle`，已写入 transcript 不丢失。
- 单个音频分段转录失败时，后续分段仍继续处理。
- 过短或近似静音的尾段不会写入 Transcript。
- 默认配置下，Vault 中不会生成原始音频文件。
- 开启保存原始音频后，每场会议只生成一个完整 WAV 文件，并位于配置的音频目录下。

## 11. M5：LLM 总结

### 11.1 用户故事

作为 Obsidian 用户，我需要在会议结束后点击总结按钮，让 EchoNote 根据 Transcript 生成会议摘要、决策、行动项、关键点和开放问题，并写回同一篇会议笔记。

### 11.2 子任务

- 实现 `LlmProvider` 接口。
- 实现 OpenAI-compatible Provider。
- 实现 Anthropic Provider。
- 实现 Summary Prompt。
- 实现 LLM JSON 响应解析。
- 实现解析失败 fallback。
- 实现长 transcript 分段总结。
- 实现 summary section parser。
- 实现总结章节替换。
- 实现 `EchoNote: Summarize Current Meeting` 命令。
- 在状态面板中接入生成总结按钮。
- 实现 LLM 配置缺失错误提示。

### 11.3 验收标准

- 使用 fixture transcript 可以在不依赖 ASR 的情况下生成总结。
- OpenAI-compatible Provider 可以向配置的 Base URL 发起请求。
- Anthropic Provider 可以向 Anthropic Messages API 发起请求。
- 缺少 API Key 或 Model 时，不发起请求，并显示可读错误。
- LLM 返回合法 JSON 时，可以解析为 `MeetingSummary`。
- LLM 返回非 JSON 文本时，不写坏会议笔记，并提示解析失败。
- 总结结果只写入以下章节：
  - `## Summary`
  - `## Decisions`
  - `## Action Items`
  - `## Key Points`
  - `## Open Questions`
- `## Transcript` 内容在总结前后完全保留。
- 当 transcript 超过配置阈值时，会进入分段总结流程。

## 12. M6：端到端集成

### 12.1 用户故事

作为 EchoNote MVP 用户，我需要从 Obsidian 内完成一次完整会议流程：启动服务、加载模型、录音、转录、写入笔记、停止会议并生成总结。

### 12.2 子任务

- 将 M3 真实 ASR Adapter 接入 M2 ASR 服务。
- 将 M4 插件端转录队列连接到真实 ASR 服务。
- 验证默认模型 `mlx-community/Qwen3-ASR-0.6B-4bit`。
- 验证可选模型 `mlx-community/Qwen3-ASR-1.7B-4bit`。
- 验证状态面板中的服务状态、模型状态、录音状态和队列状态。
- 验证一次 5 分钟会议流程。
- 验证一次至少 30 分钟长会议流程。
- 验证会议结束后总结生成。
- 验证 ASR 服务重启。
- 验证 Obsidian 关闭或插件卸载时资源释放。

### 12.3 验收标准

- 用户只需在 Obsidian 中执行一次开始会议操作，即可触发 ASR 服务检查和启动。
- ASR 服务启动后，状态面板显示 `ASR Service: Running`。
- 模型加载完成后，状态面板显示 `Model: Ready` 和实际模型 ID。
- 默认模型为 `mlx-community/Qwen3-ASR-0.6B-4bit`。
- 切换到 `mlx-community/Qwen3-ASR-1.7B-4bit` 后，重启 ASR 服务并显示新模型 ID。
- 真实麦克风录音可以产生非空 transcript。
- Transcript 持续写入会议笔记，不只保存在内存。
- 5 分钟会议流程可以完成，无未捕获异常。
- 30 分钟会议流程可以完成，过程中已写入内容不丢失。
- 停止会议后可以生成总结。
- 总结生成后，Transcript 没有被覆盖。
- 重启 ASR 服务后，可以继续开始新的会议。

## 13. M7：稳定性、打包和文档

### 13.1 用户故事

作为最终用户，我需要 EchoNote 在常见错误情况下给出清晰反馈，并能按照文档完成安装、配置和一次 MVP 流程。

### 13.2 子任务

- 完善错误码和错误提示。
- 完善状态面板错误详情。
- 捕获 ASR 服务 stdout/stderr 最近日志。
- 编写 macOS 安装说明。
- 编写 Python/MLX 依赖说明。
- 编写模型首次下载说明。
- 编写 OpenAI-compatible 配置说明。
- 编写 Anthropic 配置说明。
- 编写隐私说明。
- 编写 MVP 手动验收清单。
- 准备 Obsidian 插件打包产物。

### 13.3 验收标准

- 麦克风权限拒绝时，状态面板显示明确错误和处理建议。
- Python 路径错误时，状态面板显示明确错误和当前 Python 路径。
- ASR 服务端口被占用时，状态面板显示明确错误。
- 模型加载失败时，状态面板显示模型 ID 和错误摘要。
- LLM 配置缺失时，状态面板或 Notice 显示需要补充的配置项。
- README 或文档中包含从零开始运行 EchoNote MVP 的步骤。
- 文档中明确说明默认不保存原始音频。
- 文档中明确说明云端 LLM 总结会发送 Transcript。
- 打包产物可以被 Obsidian 加载。
- 按手动验收清单执行，所有 MVP 验收项通过。

## 14. 用户故事清单

| ID | 用户故事 | Milestone | 优先级 |
| --- | --- | --- | --- |
| US-001 | 作为开发者，我需要冻结接口契约，以便并行开发 | M0 | P0 |
| US-002 | 作为用户，我需要启用插件并看到 EchoNote 入口 | M1 | P0 |
| US-003 | 作为用户，我需要能配置会议目录、ASR 模型和 LLM Provider | M1 | P0 |
| US-004 | 作为用户，我需要看到录音、ASR 和模型状态 | M1 | P0 |
| US-005 | 作为开发者，我需要一个可联调的本地 ASR 服务 | M2 | P0 |
| US-006 | 作为开发者，我需要验证真实 Qwen3 ASR 模型可用 | M3 | P0 |
| US-007 | 作为用户，我需要开始会议后自动新建会议笔记 | M4 | P0 |
| US-008 | 作为用户，我需要会议录音被准实时转录并写入笔记 | M4 | P0 |
| US-009 | 作为用户，我需要暂停、继续和停止会议录音 | M4 | P0 |
| US-010 | 作为用户，我需要默认不保存原始音频 | M4 | P0 |
| US-011 | 作为用户，我需要可选择保存原始音频 | M4 | P1 |
| US-012 | 作为用户，我需要用 OpenAI-compatible Provider 生成总结 | M5 | P0 |
| US-013 | 作为用户，我需要用 Anthropic Provider 生成总结 | M5 | P0 |
| US-014 | 作为用户，我需要总结不覆盖原始 Transcript | M5 | P0 |
| US-015 | 作为用户，我需要完整完成一次会议记录和总结流程 | M6 | P0 |
| US-016 | 作为用户，我需要在常见错误时看到清晰提示 | M7 | P0 |
| US-017 | 作为用户，我需要安装和使用文档 | M7 | P1 |

## 15. 严格验收总清单

MVP 最终交付必须全部满足：

- 插件可以在 macOS Obsidian Desktop 中加载。
- Ribbon 图标和命令面板入口可用。
- 设置页可以保存并恢复配置。
- 设置页可以选择音频输入设备。
- 状态面板显示麦克风权限、ASR 服务状态、模型状态、录音状态、当前会议、队列数量。
- 插件可以启动本地 ASR 服务。
- ASR 服务只监听 `127.0.0.1`。
- 默认 ASR 模型为 `mlx-community/Qwen3-ASR-0.6B-4bit`。
- 用户可以选择 `mlx-community/Qwen3-ASR-1.7B-4bit`。
- ASR 模型加载状态可见。
- 开始会议会自动创建新的会议笔记。
- 会议笔记包含 PRD 中定义的默认章节。
- 录音会按配置长度分段。
- 分段会按顺序进入转录队列。
- 转录结果会追加到 `## Transcript`。
- Transcript 默认包含时间戳。
- 停止会议后已写入内容不丢失。
- 默认不保存原始音频。
- 开启保存原始音频后，每场会议只保存一个完整 WAV 文件。
- OpenAI-compatible 总结可用。
- Anthropic 总结可用。
- 总结只更新 Summary、Decisions、Action Items、Key Points、Open Questions。
- Transcript 不被总结流程覆盖。
- 麦克风权限、ASR 服务启动、模型加载、LLM 配置缺失等错误有明确提示。
- README 或文档包含安装、配置、运行和隐私说明。

## 16. 建议执行顺序

推荐执行顺序：

```text
第 1 步：M0 接口契约冻结
第 2 步：并行启动 M1、M2、M3、M5
第 3 步：M1 + M2 完成后启动 M4
第 4 步：M3 + M4 + M5 完成后启动 M6
第 5 步：M6 完成后启动 M7
```

最小可演示闭环：

```text
M0 -> M1 -> M2 -> M4
```

该闭环使用假 ASR 服务即可演示：点击开始会议、创建会议笔记、录音分段、返回假转录、写入 Transcript。

真实 MVP 闭环：

```text
M0 -> M1 -> M2 -> M3 -> M4 -> M5 -> M6 -> M7
```
