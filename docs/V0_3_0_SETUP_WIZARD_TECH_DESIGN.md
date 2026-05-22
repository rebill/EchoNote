# EchoNote v0.3.0 技术设计文档：新手向导式安装与配置

## 1. 文档信息

- 产品名称：EchoNote
- 对应 PRD：[docs/V0_3_0_SETUP_WIZARD_PRD.md](./V0_3_0_SETUP_WIZARD_PRD.md)
- 技术方案版本：v0.3.0
- 目标平台：macOS
- 主要客户端：EchoNote 桌面应用 + Obsidian Desktop 插件
- 桌面应用技术栈：Tauri 2 + TypeScript Web UI + Rust backend
- ASR 方案：继续复用现有 Python FastAPI ASR service
- 文档状态：技术设计草案

## 2. 设计目标

v0.3.0 的技术目标是把 EchoNote 桌面应用的默认体验从“手动填写 runtime 配置”升级为“自动检测 + 一键设置 + 一键修复”，同时保留 v0.2.0 已经稳定的本地 ASR 进程管理、discovery 文件和 Obsidian 插件集成。

具体目标：

- 新增 setup 状态模型，统一表达检测、安装、修复、启动和验证状态。
- 新增 setup detector，应用启动时只做轻量检测，不修改用户环境。
- 新增 setup installer/repairer，用户点击主按钮后才执行会修改本机环境的操作。
- 将当前设置表单移动到 `Advanced Settings` 折叠区。
- 默认视图只展示 runtime 状态、主按钮、进度步骤、日志/诊断入口。
- 保持现有 `companion.json` discovery 文件兼容。
- 保持 Obsidian 插件只依赖 discovery + localhost ASR HTTP API，不引入插件侧安装逻辑。

## 3. 非目标

本设计不解决：

- 内置 Python runtime。
- 内置 MLX 模型权重。
- 模型下载管理器。
- 自动配置 BlackHole、Loopback 或系统音频路由。
- macOS 签名、公证、自动更新。
- `.pkg` 或 `.dmg` 安装器。
- Windows、Linux 或 Obsidian Mobile 支持。
- 重写 Python ASR service。
- 删除高级设置。

## 4. 总体架构

v0.3.0 后，EchoNote 仍由三个本地部分协作：

```text
┌──────────────────────────────────────────────────────────────┐
│                      Obsidian Desktop                        │
│                                                              │
│  EchoNote Plugin                                             │
│  - Audio Recorder + Chunker                                  │
│  - Meeting Session Controller                                │
│  - Runtime Resolver                                          │
│  - ASR Service Client                                        │
│  - Status Panel                                              │
└───────────────────────────────┬──────────────────────────────┘
                                │
                                │ read companion.json
                                │ HTTP localhost
                                │
┌───────────────────────────────▼──────────────────────────────┐
│                         EchoNote.app                         │
│                                                              │
│  TypeScript Web UI                                           │
│  - Setup status hero                                         │
│  - Setup progress steps                                      │
│  - Primary action button                                     │
│  - Advanced settings disclosure                              │
│  - Logs and diagnostics actions                              │
│                                                              │
│  Rust Backend                                                │
│  - Setup detector                                            │
│  - Runtime installer / repairer                              │
│  - Settings store                                            │
│  - Process manager                                           │
│  - Health/model polling                                      │
│  - Discovery writer                                          │
│  - Logs and diagnostics                                      │
└───────────────────────────────┬──────────────────────────────┘
                                │
                                │ child process
                                │
┌───────────────────────────────▼──────────────────────────────┐
│                    Python ASR Service                        │
│                                                              │
│  FastAPI + Uvicorn                                           │
│  - GET /health                                               │
│  - GET /model/status                                         │
│  - POST /model/load                                          │
│  - POST /transcribe                                          │
│  - POST /shutdown                                            │
└──────────────────────────────────────────────────────────────┘
```

关键原则：

- EchoNote 桌面应用负责 runtime 安装、修复、运行和诊断。
- Obsidian 插件只负责录音、笔记和调用已发现的 ASR endpoint。
- 自动检测不能修改本机环境。
- 自动安装/修复必须由用户点击触发。
- 高级设置保留，但不再是默认入口。

## 5. 项目结构

在现有目录基础上新增模块：

```text
companion/
  src/
    components/
      SetupDashboard.ts
      SetupProgress.ts
      AdvancedSettings.ts
      StatusDashboard.ts
      SettingsPanel.ts
    lib/
      setup.ts
      companion-api.ts
      settings.ts
      state.ts
  src-tauri/
    src/
      setup.rs
      setup_detector.rs
      setup_installer.rs
      setup_types.rs
      commands.rs
      settings.rs
      process.rs
      discovery.rs
      logs.rs
      state.rs
```

说明：

- `setup_types.rs` 定义 Rust 侧可序列化 setup 数据结构。
- `setup_detector.rs` 只负责检测，不写文件，不创建 venv，不安装依赖。
- `setup_installer.rs` 负责用户触发后的创建、安装、修复和验证。
- `setup.rs` 作为 orchestration 层组合 detector、installer、settings、process、discovery。
- `SetupDashboard.ts` 作为新的默认主界面。
- `SettingsPanel.ts` 保留，但被 `AdvancedSettings.ts` 折叠包裹。

## 6. Setup 状态模型

### 6.1 SetupStatus

TypeScript/Rust 共享语义：

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

语义：

| Status | 含义 | 主操作 |
| --- | --- | --- |
| `unknown` | 尚未检测。 | `Retry` |
| `checking` | 正在轻量检测。 | 禁用 |
| `not_configured` | 缺少首次配置。 | `Set Up EchoNote` |
| `ready` | runtime 可用，服务未运行。 | `Start Service` |
| `running` | ASR service 正在运行。 | `Stop Service` |
| `repair_required` | 发现可自动修复问题。 | `Repair EchoNote` |
| `installing` | 正在安装或修复。 | 禁用 |
| `unsupported` | 当前环境不支持。 | 禁用 |
| `error` | 检测或设置失败。 | `Retry` |

### 6.2 SetupStep

```ts
type SetupStepId =
  | "system"
  | "python"
  | "runtime"
  | "dependencies"
  | "port"
  | "service"
  | "model"
  | "obsidian";

type SetupStepStatus =
  | "pending"
  | "running"
  | "passed"
  | "warning"
  | "failed"
  | "skipped";

type SetupStep = {
  id: SetupStepId;
  label: string;
  status: SetupStepStatus;
  summary: string;
  detail?: string;
  recoverable: boolean;
};
```

每一步只展示用户可理解的 `summary`。技术细节写入日志和诊断报告。

### 6.3 SetupResponse

```ts
type SetupPrimaryAction =
  | "setup"
  | "repair"
  | "start"
  | "stop"
  | "retry"
  | "none";

type SetupResponse = {
  status: SetupStatus;
  steps: SetupStep[];
  settings: CompanionSettings;
  state: CompanionAppState;
  primaryAction: SetupPrimaryAction;
  message: string;
};
```

Rust 侧使用 `serde(rename_all = "camelCase")`，前端保持 camelCase。

## 7. Rust Backend 设计

### 7.1 Commands

新增 Tauri commands：

```rust
#[tauri::command]
async fn detect_setup(state: State<'_, AppState>) -> Result<SetupResponse, String>;

#[tauri::command]
async fn install_or_repair_runtime(state: State<'_, AppState>) -> Result<SetupResponse, String>;

#[tauri::command]
async fn start_service_with_defaults(state: State<'_, AppState>) -> Result<SetupResponse, String>;

#[tauri::command]
async fn reset_setup(state: State<'_, AppState>) -> Result<SetupResponse, String>;
```

现有 commands 保留：

- `get_app_state`
- `get_settings`
- `save_settings`
- `start_service`
- `stop_service`
- `restart_service`
- `load_model`
- `copy_diagnostic_report`
- `open_logs_folder`

新 commands 与旧 commands 的关系：

- `detect_setup` 可以调用 settings store、process state、health check，但不能写入设置。
- `install_or_repair_runtime` 可以写入设置、创建 venv、安装依赖、启动服务。
- `start_service_with_defaults` 可复用现有 `start_service`，但先确保 settings 为默认可用状态。
- `reset_setup` 重置设置，不删除用户目录、venv 或日志。

### 7.2 SetupDetector

职责：

- 检查 OS 和 CPU 架构。
- 查找 Python。
- 检查 Python 版本。
- 检查 ASR service 路径。
- 检查虚拟环境。
- 检查依赖是否可 import。
- 检查端口。
- 检查当前 ASR service 是否运行。
- 检查 discovery 文件是否存在且新鲜。

Detector 约束：

- 只能读取文件和执行轻量命令。
- 不创建目录。
- 不安装依赖。
- 不启动或停止服务。
- 不写 settings/discovery/log 之外的临时探测结果。

推荐检测实现：

```text
system:
  - std::env::consts::OS == "macos"
  - std::env::consts::ARCH in ["aarch64", "x86_64"]

python:
  - settings.pythonPath
  - asr-service/.venv/bin/python
  - python3
  - python
  - run: <python> --version

runtime:
  - settings.asrServicePath exists
  - pyproject.toml exists
  - echonote_asr package directory exists

dependencies:
  - run: <python> -c "import fastapi, uvicorn, echonote_asr"
  - if mlx-audio backend: optionally probe mlx_audio import

port:
  - if current EchoNote ASR service already healthy: passed
  - else test TCP bind availability

service:
  - GET /health if process/baseUrl exists

model:
  - GET /model/status when service is running

obsidian:
  - discovery path exists or can be written by app state later
```

### 7.3 SetupInstaller

职责：

- 根据 detector 结果执行最小必要修复。
- 创建虚拟环境。
- 安装 ASR service 依赖。
- 更新 settings。
- 启动服务并验证。
- 写入 discovery。

安装/修复流程：

```text
1. Run detect_setup.
2. If unsupported, return unsupported.
3. Resolve ASR service path.
4. Resolve Python executable.
5. Create venv if missing.
6. Install dependencies if missing.
7. Select backend and model defaults.
8. Resolve or choose port.
9. Save settings.
10. Start service.
11. Wait for /health.
12. Optionally load model or check /model/status.
13. Write discovery.
14. Return ready/running response.
```

### 7.4 Python Resolution

Resolution order:

1. User-specified `settings.pythonPath`, if valid.
2. Existing venv Python under `settings.asrServicePath/.venv/bin/python`.
3. Existing repository venv Python under `asr-service/.venv/bin/python`.
4. `python3` on PATH.
5. `python` on PATH.

Validation:

```text
<python> --version
```

Minimum version: Python 3.11.

If no valid Python is found, return `repair_required` or `error` with user-facing message:

```text
EchoNote could not find Python 3.11 or newer.
```

### 7.5 ASR Service Path Resolution

Resolution order:

1. User-specified `settings.asrServicePath`, if it contains `pyproject.toml` and `echonote_asr/`.
2. Repository-relative path from current workspace: `../../asr-service` relative to `companion/src-tauri`.
3. App-bundled resource path, reserved for future packaged builds.
4. Prompt user through Advanced Settings if not found.

MVP can support source-tree mode first. Packaged app resource mode can be stubbed with a clear error until packaging exists.

### 7.6 Dependency Installation

MVP install command:

```text
<venv-python> -m pip install --upgrade pip
<venv-python> -m pip install -e <asr-service-path>[mlx]
```

For fake backend only, `.[mlx]` is not strictly required. Recommended v0.3.0 behavior:

- Default real-user backend: `mlx-audio`.
- If MLX install/import fails, setup returns warning and suggests switching to `fake` only for smoke testing.
- Do not silently downgrade real users to `fake`.

Security/safety:

- Run commands only inside the resolved ASR service directory.
- Capture stdout/stderr into logs.
- Redact paths only in shareable diagnostic report if needed; raw local logs can keep local paths.
- Do not run arbitrary user-provided shell strings.
- Use `Command` with args, not shell interpolation.

### 7.7 Port Handling

Default port: `8765`.

Behavior:

- If port is free, use it.
- If port is occupied by a healthy EchoNote ASR service, reuse it.
- If port is occupied by another process, return `repair_required`.
- MVP should not kill the process.
- Optional v0.3.0 enhancement: offer `Use another port`.

Automatic alternative port selection is allowed only if UI clearly tells the user which port was selected and settings are updated.

### 7.8 Process Manager Integration

Existing process manager remains source of truth for:

- starting service
- stopping service
- restarting service
- health polling
- model status polling
- stdout/stderr capture
- process state

Setup installer should call process manager APIs rather than duplicating process launch code.

Required integration point:

```text
setup_installer -> settings_store -> process_manager.start(settings)
```

### 7.9 Settings Changes

Existing `CompanionSettings` can be extended:

```ts
type CompanionSettings = {
  pythonPath: string;
  asrServicePath: string;
  preferredPort: number;
  backend: "fake" | "mlx-audio";
  modelPreset: "qwen3-0.6b-4bit" | "qwen3-1.7b-4bit" | "custom";
  customModelId: string;
  autoStartService: boolean;
  setupCompletedAt?: string;
  setupVersion?: string;
  autoRepairEnabled?: boolean;
};
```

Migration:

- Missing new fields must default safely.
- Existing v0.2.0 settings remain valid.
- If existing settings point to valid runtime, setup status should be `ready` or `running`, not `not_configured`.

### 7.10 Logs And Diagnostics

Diagnostic report should add:

- Setup status.
- Setup step results.
- Python candidate list with pass/fail summary.
- Resolved Python path.
- Resolved ASR service path.
- Dependency check result.
- Port check result.
- Last install/repair command names and exit codes.

Do not include:

- API keys.
- Full environment dump.
- Shell history.

## 8. Frontend Design

### 8.1 Default Layout

The first screen should be a functional app dashboard, not a settings form.

Recommended structure:

```text
┌──────────────────────────────────────────────┐
│ EchoNote                                      │
│ Local transcription is ready.                 │
│ [Start Service] [Copy Diagnostic Report]      │
├──────────────────────────────────────────────┤
│ Setup                                         │
│ ✓ Check System                                │
│ ✓ Find Python                                 │
│ ✓ Prepare ASR Runtime                         │
│ ✓ Install Dependencies                        │
│ ✓ Check Port                                  │
│ ○ Start Service                               │
│ ○ Verify Model                                │
│ ○ Connect Obsidian                            │
├──────────────────────────────────────────────┤
│ Runtime                                       │
│ Model: Qwen3 ASR 0.6B 4-bit                   │
│ API: http://127.0.0.1:8765                    │
│ Discovery: ~/Library/Application Support/...  │
├──────────────────────────────────────────────┤
│ Advanced Settings ▸                           │
└──────────────────────────────────────────────┘
```

### 8.2 Components

Recommended components:

```text
SetupDashboard
  SetupHero
  SetupProgress
  RuntimeSummary
  RuntimeActions
  AdvancedSettings
    SettingsPanel
  LogPanel
```

`SettingsPanel` should become a child of `AdvancedSettings`.

### 8.3 UI State Rules

- During `checking`, primary button disabled.
- During `installing`, all destructive or conflicting controls disabled.
- `Advanced Settings` can remain expandable during error state.
- `Save Settings` triggers `save_settings` followed by `detect_setup`.
- `Reset to Defaults` asks for confirmation, then calls `reset_setup`.
- Setup step failures should not cause layout shift.

### 8.4 Copy Guidelines

Use plain user-facing text:

- `EchoNote is ready.`
- `Set up EchoNote to use local transcription.`
- `EchoNote found an issue it can repair.`
- `Python 3.11 or newer is required.`
- `Port 8765 is already in use.`
- `Open Obsidian and check EchoNote Status.`

Avoid exposing command lines in the default view.

## 9. Obsidian Plugin Changes

Plugin remains intentionally thin.

Required changes:

- Error copy should point users to EchoNote desktop app.
- If discovery is missing, message should say:

```text
Open EchoNote and click Set Up EchoNote or Start Service.
```

- Status panel can continue to label the runtime section as `Companion` internally, but user-facing copy should prefer `EchoNote desktop app`.

Not required:

- Plugin does not run setup.
- Plugin does not install Python dependencies.
- Plugin does not write setup settings.

## 10. Discovery Compatibility

`companion.json` remains the integration contract.

Current v0.3.0 app field:

```json
{
  "version": 1,
  "app": "EchoNote",
  "service": "echonote-asr"
}
```

Compatibility rule:

- v0.3.0 plugin validates `app: "EchoNote"`.
- Historical v0.2.0 docs may still show `EchoNote ASR Companion`; do not use those values in new fixtures or runtime code.

No schema version bump is required if only `app` display name changes and current v0.3.0 plugin/desktop app are released together. If backward compatibility with v0.2.0 plugin is needed, schema v2 or dual acceptance must be reconsidered.

## 11. Error Handling

### 11.1 Error Classes

Recommended internal categories:

```ts
type SetupErrorCode =
  | "UNSUPPORTED_PLATFORM"
  | "PYTHON_NOT_FOUND"
  | "PYTHON_VERSION_UNSUPPORTED"
  | "ASR_SERVICE_NOT_FOUND"
  | "VENV_CREATE_FAILED"
  | "DEPENDENCY_INSTALL_FAILED"
  | "PORT_UNAVAILABLE"
  | "SERVICE_START_FAILED"
  | "HEALTH_CHECK_FAILED"
  | "MODEL_CHECK_FAILED"
  | "DISCOVERY_WRITE_FAILED";
```

These codes can stay internal to desktop app diagnostics. They do not need to be added to plugin `EchoNoteErrorCode` unless surfaced in Obsidian.

### 11.2 Recoverability

Each `SetupStep` must mark `recoverable`.

Examples:

- Python missing: recoverable by user installing Python or selecting path.
- Unsupported OS: not recoverable in app.
- Port occupied: recoverable by selecting another port.
- Dependency install failed: usually recoverable after network or Python fix.
- Discovery write failed: recoverable if permissions/path issue is fixed.

## 12. Testing Strategy

### 12.1 Rust Unit Tests

Add tests for:

- Python version parser.
- Python candidate resolution order.
- ASR service path validation.
- Setup status derivation from step results.
- Port availability checker.
- Dependency probe command construction.
- Settings migration with missing setup fields.
- Installer refuses unsupported platform.
- Installer does not kill non-EchoNote port owner.

### 12.2 Frontend Typecheck And UI Tests

Add or update:

- `npm run typecheck`
- Setup status rendering tests if test framework exists later.
- Browser screenshot verification for default view and advanced expanded view.

### 12.3 Integration Tests

Extend fake-backend smoke test:

1. Run `detect_setup`.
2. Confirm setup response is serializable.
3. Run setup/repair in fake backend mode.
4. Start service.
5. Verify `/health`.
6. Verify discovery file.
7. Verify plugin runtime resolver.

### 12.4 Manual QA Matrix

| Scenario | Expected |
| --- | --- |
| Clean repo, no venv | `Set Up EchoNote` creates venv and starts fake backend. |
| Existing valid venv | Status becomes `ready` without install. |
| Python missing | Clear `Python 3.11+ required` message. |
| Port occupied by non-ASR process | `Repair required`, no kill. |
| Port occupied by healthy ASR | Reuse service. |
| Invalid ASR service path | Prompt advanced path selection. |
| Dependency install failure | Error with diagnostic report. |
| Advanced setting changed | Save triggers detect refresh. |

## 13. Migration Plan

1. Read existing settings.
2. Fill missing setup fields with defaults.
3. Run `detect_setup`.
4. If existing settings are valid, show `Ready`.
5. If existing settings are invalid but repairable, show `Repair EchoNote`.
6. Do not delete or overwrite existing settings until user triggers setup/repair/save.

## 14. Rollout Plan

Recommended implementation order:

1. Types and command contracts.
2. Detector with read-only checks.
3. UI shell with mocked setup response.
4. Advanced settings disclosure.
5. Installer/repairer in fake backend path.
6. Process manager integration.
7. Diagnostics and logs.
8. Plugin copy updates.
9. Smoke test and docs.

## 15. Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| `pip install` takes long or fails | Setup feels broken | Show step progress and capture logs. |
| Python discovery differs by user shell | Python not found | Probe common paths and allow Advanced Settings override. |
| MLX dependency install is fragile | Real ASR setup fails | Keep fake smoke path and clear MLX-specific error. |
| Packaged app cannot find source ASR service | Setup impossible for non-source users | Explicitly label source-only release; add bundled-resource mode later. |
| Discovery `app` rename breaks older plugin | Runtime unavailable | Release plugin and desktop app together; consider dual acceptance if needed. |
| Automatic repair changes user settings unexpectedly | Loss of trust | Only modify settings after user action and explain changes. |

## 16. Open Questions

- Should v0.3.0 install `.[mlx]` by default, or install base dependencies first and then optional MLX?
- Should source-only release still call the app `EchoNote`, or should docs call it `EchoNote desktop app` to avoid confusion with the Obsidian plugin?
- Should port conflict offer automatic alternative port selection in MVP?
- Should setup write an explicit `setupVersion` for future migrations?
- Should plugin accept both `app: "EchoNote"` and `app: "EchoNote ASR Companion"` during a transition period?
