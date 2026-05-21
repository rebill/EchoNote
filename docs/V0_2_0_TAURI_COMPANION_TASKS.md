# EchoNote v0.2.0 Tauri Companion Task Breakdown

## 1. Goal

v0.2.0 的交付目标是让用户可以通过 macOS Companion GUI 启动和诊断本地 ASR service，并让 Obsidian 插件在 `Auto` 模式下优先使用 Companion 管理的 ASR endpoint。

MVP 截止线：

> 用户打开 Companion，点击 `Start Service`；再打开 Obsidian，`Auto` 模式能发现 Companion 并完成一次 fake backend 转录。

相关文档：

- [v0.2.0 PRD](./V0_2_0_TAURI_COMPANION_PRD.md)
- [v0.2.0 Technical Design](./V0_2_0_TAURI_COMPANION_TECH_DESIGN.md)
- [GitHub milestone: v0.2.0](https://github.com/rebill/EchoNote/milestone/1)

## 2. GitHub Issues

| Order | Issue | Track | Dependencies | Parallel Notes |
| --- | --- | --- | --- | --- |
| 0 | [#2 Freeze Companion discovery schema and runtime mode contract](https://github.com/rebill/EchoNote/issues/2) | Contract | None | Must finish first. |
| 1 | [#3 Scaffold Tauri Companion app](https://github.com/rebill/EchoNote/issues/3) | Companion | #2 | Can run with plugin runtime mode and docs. |
| 2 | [#4 Add Companion settings store](https://github.com/rebill/EchoNote/issues/4) | Companion | #3 | Can run with plugin discovery reader after #2. |
| 3 | [#5 Add ASR service process manager](https://github.com/rebill/EchoNote/issues/5) | Companion | #3, #4 | Blocks health polling. |
| 4 | [#6 Add health and model polling](https://github.com/rebill/EchoNote/issues/6) | Companion | #5 | Blocks final discovery state quality. |
| 5 | [#7 Add discovery file writer](https://github.com/rebill/EchoNote/issues/7) | Contract/Companion | #2, #4, #6 | Plugin can use fixtures before real writer lands. |
| 6 | [#8 Add Companion logs and diagnostics](https://github.com/rebill/EchoNote/issues/8) | Companion | #4, #5 | Can start after process stdout/stderr exists. |
| 7 | [#9 Add plugin ASR runtime mode](https://github.com/rebill/EchoNote/issues/9) | Plugin | #2 | Can run with Companion scaffold. |
| 8 | [#10 Add plugin Companion discovery reader](https://github.com/rebill/EchoNote/issues/10) | Plugin | #2 | Can be fixture-driven before real Companion exists. |
| 9 | [#11 Integrate runtime resolver into MeetingSessionController](https://github.com/rebill/EchoNote/issues/11) | Plugin | #9, #10 | Critical plugin integration point. |
| 10 | [#12 Update status panel for Companion state](https://github.com/rebill/EchoNote/issues/12) | Plugin/UI | #9, #10 | Can start once status types are agreed. |
| 11 | [#13 Add fake-backend E2E smoke test](https://github.com/rebill/EchoNote/issues/13) | Testing | #5, #6, #7, #10, #11 | Final integration gate. |
| 12 | [#14 Update docs and release checklist for Companion](https://github.com/rebill/EchoNote/issues/14) | Docs/Release | PRD + Tech Design | Can start early; final pass happens last. |

## 3. Todo List

### Contract

- [ ] [#2](https://github.com/rebill/EchoNote/issues/2) Freeze `companion.json` schema and runtime mode contract.

### Companion

- [ ] [#3](https://github.com/rebill/EchoNote/issues/3) Scaffold Tauri Companion app.
- [ ] [#4](https://github.com/rebill/EchoNote/issues/4) Add Companion settings store.
- [ ] [#5](https://github.com/rebill/EchoNote/issues/5) Add ASR service process manager.
- [ ] [#6](https://github.com/rebill/EchoNote/issues/6) Add health and model polling.
- [ ] [#7](https://github.com/rebill/EchoNote/issues/7) Add discovery file writer.
- [ ] [#8](https://github.com/rebill/EchoNote/issues/8) Add Companion logs and diagnostics.

### Plugin

- [ ] [#9](https://github.com/rebill/EchoNote/issues/9) Add plugin ASR runtime mode.
- [ ] [#10](https://github.com/rebill/EchoNote/issues/10) Add plugin Companion discovery reader.
- [ ] [#11](https://github.com/rebill/EchoNote/issues/11) Integrate runtime resolver into `MeetingSessionController`.
- [ ] [#12](https://github.com/rebill/EchoNote/issues/12) Update status panel for Companion state.

### Verification And Release

- [ ] [#13](https://github.com/rebill/EchoNote/issues/13) Add fake-backend E2E smoke test.
- [ ] [#14](https://github.com/rebill/EchoNote/issues/14) Update docs and release checklist for Companion.

## 4. Parallel Development Plan

### Batch 0: Contract First

- [#2](https://github.com/rebill/EchoNote/issues/2)

This issue should finish before implementation branches diverge. It fixes shared assumptions for:

- `companion.json`
- stale discovery behavior
- `Auto` / `Companion` / `Manual` runtime semantics
- plugin fallback rules

### Batch 1: Independent Starts

Can run immediately after #2:

- [#3](https://github.com/rebill/EchoNote/issues/3) Companion scaffold
- [#9](https://github.com/rebill/EchoNote/issues/9) Plugin runtime mode
- [#10](https://github.com/rebill/EchoNote/issues/10) Plugin discovery reader with fixtures
- [#14](https://github.com/rebill/EchoNote/issues/14) Docs draft updates

### Batch 2: Core Companion And Plugin Integration

Can run after scaffold and contract work:

- [#4](https://github.com/rebill/EchoNote/issues/4) Companion settings store
- [#5](https://github.com/rebill/EchoNote/issues/5) ASR service process manager
- [#11](https://github.com/rebill/EchoNote/issues/11) Runtime resolver integration
- [#12](https://github.com/rebill/EchoNote/issues/12) Status panel Companion state

### Batch 3: Runtime Quality

Can run once process manager and state model exist:

- [#6](https://github.com/rebill/EchoNote/issues/6) Health/model polling
- [#7](https://github.com/rebill/EchoNote/issues/7) Discovery file writer
- [#8](https://github.com/rebill/EchoNote/issues/8) Logs and diagnostics

### Batch 4: Final Gate

Should run after the main runtime path is integrated:

- [#13](https://github.com/rebill/EchoNote/issues/13) fake-backend E2E smoke test
- [#14](https://github.com/rebill/EchoNote/issues/14) final docs/release checklist pass

## 5. Dependency Graph

```text
#2 contract
 ├─ #3 companion scaffold
 │   ├─ #4 settings store
 │   │   ├─ #7 discovery writer
 │   │   └─ #8 logs/diagnostics
 │   └─ #5 process manager
 │       ├─ #6 health/model polling
 │       └─ #8 logs/diagnostics
 ├─ #9 plugin runtime mode
 │   └─ #11 meeting session integration
 └─ #10 plugin discovery reader
     └─ #11 meeting session integration

#12 status panel can start after #9/#10 contracts
#13 E2E after #5/#6/#7/#10/#11
#14 docs can start early, finish last
```

## 6. Suggested Ownership Split

If multiple developers or agents work in parallel:

- Companion owner: [#3](https://github.com/rebill/EchoNote/issues/3), [#4](https://github.com/rebill/EchoNote/issues/4), [#5](https://github.com/rebill/EchoNote/issues/5), [#6](https://github.com/rebill/EchoNote/issues/6)
- Plugin owner: [#9](https://github.com/rebill/EchoNote/issues/9), [#10](https://github.com/rebill/EchoNote/issues/10), [#11](https://github.com/rebill/EchoNote/issues/11), [#12](https://github.com/rebill/EchoNote/issues/12)
- Integration owner: [#2](https://github.com/rebill/EchoNote/issues/2), [#7](https://github.com/rebill/EchoNote/issues/7), [#8](https://github.com/rebill/EchoNote/issues/8), [#13](https://github.com/rebill/EchoNote/issues/13), [#14](https://github.com/rebill/EchoNote/issues/14)

If one person works sequentially, use this order:

```text
#2 -> #3 -> #4 -> #5 -> #6 -> #7 -> #9 -> #10 -> #11 -> #12 -> #8 -> #13 -> #14
```

## 7. Release Readiness Checklist

- [ ] Companion can start fake backend without Terminal.
- [ ] Companion writes valid `companion.json`.
- [ ] Plugin `Auto` mode discovers Companion.
- [ ] Plugin `Manual` mode still works.
- [ ] Companion logs and diagnostic report are available.
- [ ] Fake-backend E2E smoke test passes.
- [ ] README documents Companion and Manual modes.
- [ ] Troubleshooting documents Companion diagnostics.
- [ ] `CHANGELOG.md` has v0.2.0 notes.
- [ ] Release decision is made for Companion artifact: source-only, `.app`, or `.dmg`.
