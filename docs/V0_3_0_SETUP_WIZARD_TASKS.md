# EchoNote v0.3.0 Setup Wizard Task Breakdown

## 1. Goal

v0.3.0 的交付目标是让新手用户打开 EchoNote 桌面应用后，不需要手动填写 Python path、ASR service path、port、backend、model ID，就能通过向导完成本地 ASR runtime 的检测、设置、修复、启动和验证。

MVP 截止线：

> 用户打开 EchoNote，点击 `Set Up EchoNote`；应用完成 fake backend runtime 设置并启动服务；Obsidian 插件能发现 discovery 并完成一次 fake backend 转录。

完成状态：

> Completed by Codex on 2026-05-22. Verified with companion TypeScript typecheck/build, Rust tests, plugin typecheck/build, fake-backend smoke test, Tauri production build, and browser preview layout checks. GitHub issues #15-#28 were closed with the same verification summary.

相关文档：

- [v0.3.0 PRD](./V0_3_0_SETUP_WIZARD_PRD.md)
- [v0.3.0 Technical Design](./V0_3_0_SETUP_WIZARD_TECH_DESIGN.md)

## 2. Suggested Issues

GitHub issues 已创建并挂到 [v0.3.0 milestone](https://github.com/rebill/EchoNote/milestone/2)。

| Order | Task | Track | Dependencies | Parallel Notes |
| --- | --- | --- | --- | --- |
| 0 | [#15 T0 Freeze setup status and command contracts](https://github.com/rebill/EchoNote/issues/15) | Contract | None | Must finish first. |
| 1 | [#16 T1 Add setup response types in Rust and TypeScript](https://github.com/rebill/EchoNote/issues/16) | Contract | #15 | Can run with UI mock work. |
| 2 | [#17 T2 Implement read-only setup detector](https://github.com/rebill/EchoNote/issues/17) | Desktop/Rust | #16 | Blocks reliable UI status. |
| 3 | [#18 T3 Build setup dashboard shell with mocked data](https://github.com/rebill/EchoNote/issues/18) | Desktop/UI | #16 | Can run in parallel with detector. |
| 4 | [#19 T4 Move current settings into Advanced Settings disclosure](https://github.com/rebill/EchoNote/issues/19) | Desktop/UI | #18 | Independent from installer. |
| 5 | [#20 T5 Add setup commands and wire detector to UI](https://github.com/rebill/EchoNote/issues/20) | Desktop/Integration | #17, #18 | First end-to-end read-only milestone. |
| 6 | [#21 T6 Implement Python and ASR service path resolution](https://github.com/rebill/EchoNote/issues/21) | Desktop/Rust | #17 | Needed by installer. |
| 7 | [#22 T7 Implement venv creation and dependency probe/install flow](https://github.com/rebill/EchoNote/issues/22) | Desktop/Rust | #21 | Highest setup risk. |
| 8 | [#23 T8 Integrate setup installer with process manager](https://github.com/rebill/EchoNote/issues/23) | Desktop/Rust | #22 | Blocks one-click setup. |
| 9 | [#24 T9 Add setup-aware diagnostics and logs](https://github.com/rebill/EchoNote/issues/24) | Desktop/Rust | #17, #22 | Can run after step model exists. |
| 10 | [#25 T10 Update plugin user-facing copy for EchoNote desktop app](https://github.com/rebill/EchoNote/issues/25) | Plugin | #15 | Does not block desktop work. |
| 11 | [#26 T11 Extend fake-backend smoke test for setup flow](https://github.com/rebill/EchoNote/issues/26) | Testing | #20, #23 | Final integration gate. |
| 12 | [#27 T12 Update docs, release notes, and troubleshooting](https://github.com/rebill/EchoNote/issues/27) | Docs/Release | #25, #26 | Finish near release. |
| 13 | [#28 T13 Manual QA pass for first-run and repair scenarios](https://github.com/rebill/EchoNote/issues/28) | QA | #26 | Release blocker. |

## 3. Todo List

### Contract

- [x] [#15](https://github.com/rebill/EchoNote/issues/15) T0 Freeze setup status and command contracts.
  - Define `SetupStatus`, `SetupStep`, `SetupResponse`, and `SetupPrimaryAction`.
  - Confirm command names: `detect_setup`, `install_or_repair_runtime`, `start_service_with_defaults`, `reset_setup`.
  - Confirm whether discovery `app` accepts only `EchoNote` or dual values during transition.

- [x] [#16](https://github.com/rebill/EchoNote/issues/16) T1 Add setup response types in Rust and TypeScript.
  - Add Rust serializable types under `companion/src-tauri/src/setup_types.rs`.
  - Add TypeScript types under `companion/src/lib/setup.ts`.
  - Keep camelCase API shape consistent across Rust and TypeScript.
  - Add fixture-style sample setup responses for UI development.

### Desktop Rust

- [x] [#17](https://github.com/rebill/EchoNote/issues/17) T2 Implement read-only setup detector.
  - Add `setup_detector.rs`.
  - Detect OS and CPU architecture.
  - Resolve Python candidates without modifying files.
  - Validate Python version `>=3.11`.
  - Validate ASR service path.
  - Probe venv and dependency imports.
  - Check port availability without killing processes.
  - Check existing ASR service health.
  - Convert detector results into `SetupResponse`.

- [x] [#21](https://github.com/rebill/EchoNote/issues/21) T6 Implement Python and ASR service path resolution.
  - Use explicit settings path first.
  - Then existing `asr-service/.venv/bin/python`.
  - Then `python3` and `python`.
  - Validate ASR service by `pyproject.toml` and `echonote_asr/`.
  - Add tests for candidate order and failure modes.

- [x] [#22](https://github.com/rebill/EchoNote/issues/22) T7 Implement venv creation and dependency probe/install flow.
  - Add `setup_installer.rs`.
  - Create venv only after user action.
  - Install dependencies with `Command` args, not shell strings.
  - Capture stdout/stderr into logs.
  - Return step-level failure on install errors.
  - Add tests for command construction and safety rules.

- [x] [#23](https://github.com/rebill/EchoNote/issues/23) T8 Integrate setup installer with process manager.
  - Save resolved settings after successful repair/setup.
  - Start ASR service through existing process manager.
  - Wait for `/health`.
  - Check `/model/status`.
  - Write discovery after service state is known.
  - Return `running` or `ready` response.

- [x] [#24](https://github.com/rebill/EchoNote/issues/24) T9 Add setup-aware diagnostics and logs.
  - Include setup status in diagnostic report.
  - Include setup step summaries.
  - Include Python candidate summary.
  - Include install command exit codes.
  - Redact secrets and avoid environment dumps.

### Desktop UI

- [x] [#18](https://github.com/rebill/EchoNote/issues/18) T3 Build setup dashboard shell with mocked data.
  - Add `SetupDashboard.ts`.
  - Add `SetupProgress.ts`.
  - Add `RuntimeSummary` section.
  - Add primary action button state rules.
  - Render `checking`, `not_configured`, `ready`, `running`, `repair_required`, `unsupported`, and `error`.

- [x] [#19](https://github.com/rebill/EchoNote/issues/19) T4 Move current settings into Advanced Settings disclosure.
  - Wrap existing `SettingsPanel` in `AdvancedSettings`.
  - Hide Python path, ASR service path, port, backend, and custom model ID by default.
  - Preserve `Save Settings`.
  - Add `Reset to Defaults`.
  - After save/reset, call `detect_setup`.

- [x] [#20](https://github.com/rebill/EchoNote/issues/20) T5 Add setup commands and wire detector to UI.
  - Add `detectSetup`, `installOrRepairRuntime`, `startServiceWithDefaults`, `resetSetup` API helpers.
  - Run `detectSetup` on app load.
  - Disable conflicting controls during `checking` and `installing`.
  - Show user-friendly step summaries.
  - Keep logs/diagnostic actions available.

### Plugin

- [x] [#25](https://github.com/rebill/EchoNote/issues/25) T10 Update plugin user-facing copy for EchoNote desktop app.
  - Discovery missing: tell user to open EchoNote and click `Set Up EchoNote` or `Start Service`.
  - Runtime unavailable: avoid saying `EchoNote ASR Companion`.
  - Keep internal runtime resolver behavior unchanged.
  - Run plugin typecheck.

### Testing

- [x] [#26](https://github.com/rebill/EchoNote/issues/26) T11 Extend fake-backend smoke test for setup flow.
  - Add setup detector call.
  - Run install/repair in fake backend-safe path.
  - Start service through setup command or default-start command.
  - Verify `/health`.
  - Verify discovery file.
  - Verify plugin resolver.

- [x] [#28](https://github.com/rebill/EchoNote/issues/28) T13 Manual QA pass for first-run and repair scenarios.
  - Fresh checkout, no venv.
  - Existing valid venv.
  - Python missing or invalid path.
  - Port occupied by non-ASR process.
  - Existing healthy ASR service.
  - Invalid ASR service path.
  - Dependency install failure.
  - Advanced settings save and reset.

### Docs And Release

- [x] [#27](https://github.com/rebill/EchoNote/issues/27) T12 Update docs, release notes, and troubleshooting.
  - Update README with setup wizard flow.
  - Update troubleshooting for setup errors.
  - Update release checklist.
  - Update changelog.
  - Add screenshots or textual flow once UI lands.

## 4. Parallel Development Plan

### Batch 0: Contract First

- T0 Freeze setup status and command contracts.
- T1 Add setup response types.

This batch fixes shared assumptions for Rust, TypeScript UI, plugin copy, tests, and docs.

### Batch 1: Read-Only Setup And UI Shell

Can run after T1:

- T2 setup detector.
- T3 setup dashboard shell.
- T10 plugin copy updates.

Goal:

> EchoNote can show a setup state without changing the user's machine.

### Batch 2: Default UI Completion

Can run after T3:

- T4 Advanced Settings disclosure.
- T5 UI command wiring.

Goal:

> The default UI no longer looks like a developer settings form.

### Batch 3: Installer And Runtime Integration

Can run after detector and path resolution:

- T6 Python/path resolution.
- T7 venv/dependency install flow.
- T8 process manager integration.
- T9 diagnostics and logs.

Goal:

> `Set Up EchoNote` can prepare and start fake backend runtime from the desktop app.

### Batch 4: Final Gate

After setup path works:

- T11 fake-backend smoke test.
- T12 docs/release updates.
- T13 manual QA.

Goal:

> v0.3.0 release is shippable as source-only with a new setup wizard.

## 5. Dependency Graph

```text
T0 contract
 └─ T1 shared types
     ├─ T2 setup detector
     │   ├─ T5 UI command wiring
     │   ├─ T6 path resolution
     │   │   └─ T7 venv/dependency install
     │   │       └─ T8 process manager integration
     │   │           └─ T11 smoke test
     │   └─ T9 diagnostics/logs
     └─ T3 setup dashboard shell
         ├─ T4 advanced settings disclosure
         └─ T5 UI command wiring

T10 plugin copy can start after T0.
T12 docs can start early, finish after T11.
T13 QA after T11.
```

## 6. Suggested Ownership Split

If multiple developers or agents work in parallel:

- Contract owner: T0, T1.
- Rust setup owner: T2, T6, T7, T8, T9.
- UI owner: T3, T4, T5.
- Plugin owner: T10.
- Integration/QA owner: T11, T12, T13.

If one person works sequentially, use this order:

```text
T0 -> T1 -> T2 -> T3 -> T4 -> T5 -> T6 -> T7 -> T8 -> T9 -> T10 -> T11 -> T12 -> T13
```

## 7. Release Readiness Checklist

- [x] Default screen no longer exposes Python path, ASR service path, port, backend, or custom model ID.
- [x] `Advanced Settings` reveals the existing manual configuration form.
- [x] `detect_setup` is read-only and runs on app launch.
- [x] `Set Up EchoNote` can create or validate runtime in source-tree mode.
- [x] `Repair EchoNote` fixes missing venv/dependencies when possible.
- [x] Port conflict does not kill non-EchoNote processes.
- [x] Setup progress shows step-level status.
- [x] Diagnostic report includes setup step summaries.
- [x] Existing v0.2.0 settings migrate without data loss.
- [x] Fake-backend setup smoke test passes.
- [x] Plugin can discover the desktop app and complete fake transcription.
- [x] README and troubleshooting describe the new flow.
- [x] CHANGELOG has v0.3.0 setup wizard notes.

## 8. Out Of Scope For v0.3.0 Tasks

- Bundled Python runtime.
- Bundled ASR service resources inside signed app.
- Model download manager.
- App notarization and `.dmg` release.
- Automatic audio device setup.
- Plugin-side setup installer.

## 9. Resolved Implementation Decisions

- The default setup path installs fake-backend-safe dependencies; MLX dependencies are selected only when the backend is `mlx-audio`.
- Setup reports a recoverable port conflict instead of automatically choosing a different port.
- Discovery `app` is frozen to `EchoNote`; the plugin rejects historical `EchoNote ASR Companion` discovery.
- Setup completion is stored with `setupCompletedAt` and `setupVersion`.
- v0.3.0 remains source-only; README, troubleshooting, and release docs describe that constraint.
