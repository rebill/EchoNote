# EchoNote v1.0.0 独立桌面应用技术方案

## 1. 文档信息

| 项 | 内容 |
|---|---|
| 版本 | 1.0.0（开发期 1.0.0-alpha.N） |
| 分支 | `feat/standalone-app`（基于 `release/v0.9.0`） |
| 相关文档 | `V1_0_0_DESKTOP_APP_PRD.md`、`V1_0_0_DESKTOP_APP_TASKS.md`、`v1-performance-budgets.json` |
| 状态 | M0 草案。第 14 节记录已完成的验证；标"待定"的结论等空闲时段评测和 Windows 实测后冻结 |

## 2. 目标与约束

- 一个进程完成采集、转录、存储、总结和界面，不依赖 Python、Obsidian、虚拟声卡。
- 平台：macOS 14.2+（Apple Silicon、Intel），Windows 10/11 x64。低配基线 4 核 8 GB，无独显。
- 全部推理在 CPU 上完成。默认模型随安装包提供，离线可用。
- 没有 Apple 和 Windows 签名证书：macOS 用 ad-hoc 签名，Windows 不签名。
- 性能指标见 PRD 第 8 节，冻结值写入 `v1-performance-budgets.json`。

## 3. 总体架构

```
┌──────────────────────── Tauri 2 进程 ────────────────────────┐
│ WebView（Svelte 5）  ← IPC 命令 / 事件（≤10 Hz 批量）→  Rust   │
│                                                              │
│  echonote-core                                               │
│  ├─ capture   麦克风(cpal) / 系统音频(macOS Tap, WASAPI)       │
│  ├─ timeline  每轨样本时钟、补零、16 kHz 重采样                 │
│  ├─ segmenter VAD + 硬上限切分                                │
│  ├─ asr       sherpa-onnx 离线模型 + 调度器                    │
│  ├─ dedup     麦克风轨串音去重                                 │
│  ├─ store     SQLite（WAL、FTS5）+ 每轨 Ogg/Opus 文件          │
│  └─ llm       OpenAI 兼容 / Anthropic / Ollama 客户端          │
└──────────────────────────────────────────────────────────────┘
```

`echonote-core` 不依赖 Tauri，`echonote-bench` 和单元测试直接调用它。应用壳（`app/`）只负责窗口、托盘、IPC 和系统集成。

## 4. 线程模型

| 线程 | 职责 | 约束 |
|---|---|---|
| 音频回调（系统创建，每个源一个） | 把样本写入该源的 SPSC 环形缓冲 | 不分配内存、不加锁、不做系统调用 |
| capture | 每 20 ms 读取各环形缓冲，补零对齐，重采样到 16 kHz，写 Opus，喂 VAD | 写 Opus 和 VAD 都是轻量操作 |
| asr-worker | 唯一的离线解码线程，按第 9 节规则执行定稿和预览 | 模型线程数 2（待评测冻结） |
| store | 单写连接，批量提交段落和状态 | 读连接池供界面查询 |
| tokio 运行时 | LLM 请求、模型下载、IPC 事件合并 | 不运行 CPU 密集任务 |

环形缓冲大小按 10 秒音频分配。ASR 落后时音频在 capture 线程里的时间线缓冲中等待，不丢弃。

M1 实现（`echonote-core::engine`）：`Engine::open` 打开库、启动 ASR 线程和 store 线程；`start` 建会议和两轨录音文件，起一个会话线程，每 20 ms 一拍：读源、时间线、写 Opus、喂管线、派发解码，每 500 ms 提交一批定稿。状态机为 starting → recording ⇄ paused → stopping → finalized，启动时 `recover` 处理遗留的 recording 会议（第 12 节），标为 recovered。麦克风请求了但打不开是错误；系统音频打不开只发通知，录音继续。设备失效后立即重开一次，之后每 2 s 重试。

- ASR 线程只持有一个模型，按引用计数，空闲 10 分钟卸载。按下录音立即开始采集，模型在后台加载，加载期间音频排队；加载失败只发通知，录音和存盘照常。
- store 线程独占写连接，任务以闭包发送；界面查询用只读连接。
- 麦克风定稿先放着，等系统音频的进度排除串音（最多 3 s）再放出；系统音频预览判定为串音时，麦克风这一段不再出预览，直到它结束。
- 排队的定稿音频超过 60 s 时丢掉内存里的 PCM，解码时从 Ogg 文件读回（写入器先 flush 当前页）。串音参考按麦克风的进度减 30 s 修剪，解码落后很多时系统音频的字也不会被提前删掉。

## 5. 音频采集

所有源实现同一个 `AudioSource` trait：`format()`、`drain()`（取出样本和时间戳）、`failed()`（设备失效，会话重新打开）、`finished()`、`dropped()`。测试用 `FakeSource`（按实时或尽快速度喂样本，可以在指定时间模拟设备失效）和 WAV 文件源。

M1 实现（`echonote-core::capture`）：

- 每个源一个 rtrb SPSC 环形缓冲，按 10 s 分配。回调写入样本和一个 `Stamp{frame, frames, at_ns}`，`at_ns` 是共用单调时钟上第一帧的采集时刻。缓冲满了整块丢弃，帧计数照常前进，时间线据此补零。回调不分配内存、不加锁。
- 麦克风（`MicSource`，cpal 0.18）：支持 f32、i16、i32、u16 输入。`at_ns` 按回调时刻减去 cpal 报告的 callback − capture 时间差。没指定设备时每秒查一次默认输入设备，变了就让会话重新打开。
- macOS 系统音频（`SystemSource`，`native/macos_tap.m`）：先按 Tap 格式打开、分配环形缓冲，再启动。IOProc 用 `kAudioDevicePropertyIOProcStreamUsage` 只启用最后一条（Tap）流；回调传入缓冲年龄 `now->mHostTime − inputTime->mHostTime`，Rust 侧倒推采集时刻。默认输出设备和 Tap 格式的属性监听设置变更标志，会话据此重建。新代码只在本机编译和单元测试过，真机采集要在 app bundle 里弹授权窗，用 `scripts/dev/live_demo.sh` 复测。
- Windows 系统音频（`capture/system_windows.rs`，windows-rs 0.62）：先试进程回环（`ActivateAudioInterfaceAsync`，排除自身进程树，48 kHz 立体声 i16，`AUTOCONVERTPCM`，事件驱动）；失败时退回默认渲染端点的 loopback，并向同一端点播放静音流，每秒检查默认端点是否变化。时间戳用 QPC（100 ns）。`ECHONOTE_LOOPBACK=endpoint` 强制走端点模式，方便在 Windows 11 上测 Windows 10 路径。目前只通过了 Windows 目标的 clippy，没有在 Windows 上运行过。
- 录音期间 `KeepAwake` 阻止系统睡眠（macOS `beginActivity`，Windows `SetThreadExecutionState`）。

### 5.1 麦克风

cpal 默认输入设备，取设备原生格式（MacBook Air 实测 48 kHz 单声道 f32，回调周期 10.67 ms）。设备切换时重建流，时间线补零。

### 5.2 macOS 系统音频（Core Audio Process Tap，14.2+）

M0 已在 macOS 15.7.7 上验证，代码在 `crates/echonote-core/native/macos_tap.m`：

1. `CATapDescription initMonoGlobalTapButExcludeProcesses:`，排除 EchoNote 自身进程（回放时不录到自己），`privateTap = YES`，`muteBehavior = CATapUnmuted`。
2. `AudioHardwareCreateProcessTap`，读 `kAudioTapPropertyFormat`（实测 48 kHz 单声道 f32）。
3. 创建私有聚合设备：**必须把当前默认输出设备作为主子设备**（`MainSubDevice` + `SubDeviceList`）。只含 Tap 的聚合设备没有时钟，IOProc 不运行（M0 第一次实测零回调）。
4. `AudioDeviceCreateIOProcID` + `AudioDeviceStart`，IOProc 取最后一个输入缓冲（子设备若有输入流，Tap 流排在后面）。M1 用 `kAudioDevicePropertyIOProcStreamUsage` 只启用 Tap 流，避免打开 AirPods 等设备的输入流（会把蓝牙耳机切到低音质的 HFP 模式），并监听 Tap 格式变化。

实测（2 次 45–48 秒录音）：回调周期 10.67 ms，无迟到回调，无丢样；从启动到第一个回调 0.33–0.69 s；无声时继续送全零缓冲，时间线连续；麦克风与 Tap 相对漂移 0.4–1.6 ppm。

权限：Info.plist 需要 `NSMicrophoneUsageDescription` 和 `NSAudioCaptureUsageDescription`，进程必须以 app bundle 启动才会弹窗。TCC 按代码签名记录授权，ad-hoc 签名的 cdhash 随二进制变化，**每次升级都会重新弹两个授权窗**。缓解：源码分发时让用户在钥匙串里创建一个自签名代码签名证书，构建脚本用它签名，designated requirement 稳定后授权可跨版本保留（M2 验证）。

默认输出设备变化（插耳机、切蓝牙）时监听 `kAudioHardwarePropertyDefaultOutputDevice`，重建聚合设备，时间线补零。

时间戳：M0 探针按回调到达时间对齐两轨，测得的 +10 ms 正好是一个 IOProc 周期，可能是锚点重复计算了一个缓冲。M1 改用 IOProc 的 `AudioTimeStamp` 主机时间和 cpal 回调里的采集时间戳。

WebKit 在独立进程里播放音频，按 `getpid()` 排除不到，所以录音期间禁用回放（两个平台都一样）。

### 5.3 Windows 系统音频（WASAPI loopback）

- 首选进程回环（`AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`，排除 EchoNote 自身）。据微软文档需要 Windows 10 build 20348 以上，实际等于 Windows 11。
- 其余 Windows 10 用默认渲染设备的普通 loopback，回放时会录到 EchoNote 自己的声音：录音期间禁用回放即可。
- loopback 在输出静音时不送数据包。做法与 OBS 相同：录音期间向同一个输出端点播放静音流，loopback 就一直按设备时钟送数据，不用在墙钟和设备时钟之间来回切换。cpal 做不到，M1 用 windows-rs 写原生 WASAPI 源。
- M0 已提供 `capture_probe` 的 Windows 版本（cpal loopback）和 `scripts/dev/windows_m0.ps1`，需要在 Windows 机器上实测静音间隙、设备切换和漂移。

## 6. 时间线

- 每轨维护样本时钟，零点是录音开始时的单调时钟。第一个回调按其主机时间减去缓冲时长定位。
- 缺失的区间补零（设备切换、loopback 静音、回调迟到）。
- 漂移：实测内置设备 < 2 ppm（1 小时 < 8 ms）。用 rubato 可调比例的重采样器缓慢修正，不插入或丢弃样本，两轨偏差保持在 20 ms 以内；USB、蓝牙设备的漂移在 M1 实测。
- 重采样到 16 kHz 单声道（rubato，多声道先平均）。录音文件和识别都用 16 kHz。
- 两轨对齐误差：M0 实测麦克风相对系统音频约 +10 ms（扬声器到麦克风的声学和设备延迟）。

M1 实现（`capture/timeline.rs`）：rubato 4 异步 sinc 重采样（128 抽头，BlackmanHarris2，10 ms 输入块），多声道先平均。每块按时间戳算出它在时间线上应在的位置：落后超过 50 ms 补零；偏差超过 10 s 视为时钟跳变，重新锚定；其余用比例控制慢慢调整重采样比例（时间常数 10 s，限幅 ±0.5%），稳态误差等于漂移 × 10 s，测试覆盖 ±500 和 ±1500 ppm，误差都在 20 ms 以内。暂停时不写音频，补 0.6 s 静音结束当前一句，恢复后整条时间线后移暂停的时长。设备失效 0.5 s 后按墙钟补零到当前时刻减 0.2 s，重新打开的设备从正确位置接上。

## 7. 分段

M0 发现 sherpa-onnx 的 `max_speech_duration` 不能切开连续语音。它在超时后提高阈值等待停顿，而播客、视频和语速快的人没有足够长的低概率区间。真机外放录音里单段长到 15–32 秒，S1 重解码放大到 4.8–11 倍，定稿 p95 延迟 10.5 秒。

M0 的模拟器（`sim.rs`）先用叠加方式验证：sherpa 照常分段，段长达到 12 s（`--hard-cap`）时，EchoNote 在最近 2 s 内能量最低的 20 ms 帧处切开并立即定稿，sherpa 之后送出的段落只取切点之后的部分。真机录音上 p95 定稿延迟从 10.5 s 降到 2.7 s，解码线程占用从 1.20 降到 0.84，用户自己的一句话不再和串音合并在同一段里。

叠加方式下两个分段器管理同一段音频：sherpa 仍然缓存整段，`max_speech` 仍会抬高阈值，短于 0.25 s 的残段被丢弃，切点可能落在字中间。M1 改为 EchoNote 是唯一的分段器：

- VAD 只输出每个窗口是否为语音。sherpa 的分段直接丢弃，只读它的结束位置。
- 定稿是统一时间线缓冲上的样本区间 `[start, end)`，不复制 sherpa 返回的样本。
- 句尾静音 0.5 s 结束一段，最短语音 0.25 s，起点向前保留 2 个窗口加最短语音时长（M1 实测与 sherpa 完全一致）。
- 保留 sherpa 的宽松模式：缓冲语音超过 `max_speech`（10 s）后，阈值升到 0.9，低于阈值 0.1 s 就结束一段。长句因此能在短停顿处切开，硬上限只兜底连续语音。结束位置取 sherpa 实际报告的值，两种模式都准确。
- 连续语音超过 29 s 时 flush sherpa 的缓冲（不重置模型状态）。M1 试过 14 s 就 reset：模型状态清零后，紧接着的轻声没被检出；14 s 就 flush 又会提前退出宽松模式，184032 录音里一句轻声又和串音合进同一段，被模型吞掉。
- 硬上限切分时前后两段各多解码 0.2 s（共享约 0.4 s），用 token 时间戳把重叠部分的字分给各自一段。没有 token 时间戳的模型（Paraformer）不重叠。
- 分段器和第 9 节的调度器写成 `echonote-core::pipeline` 里的同一个状态机，时钟由调用方传入。应用里的真实线程和 `echonote-bench` 驱动同一份代码。
- 硬上限取 8 s 还是 12 s 由 `continuous-zh` 评测集决定（第 14 节）。

M1 用新管线重放 M0 的两段真机录音（SenseVoice，S1，硬上限 12 s），和 M0 叠加方式的 cap12 结果对比：分段几乎一致，所有不变量成立；共用预览名额后解码音频放大从 10.5–11.0 倍降到 7.1–7.3 倍，解码线程占用从 0.84–0.91 降到 0.54–0.55，CPU 从 1.61 降到 1.05–1.07 核秒每音频秒。定稿 p95 1.9–2.8 s 与 M0 持平。

## 8. 识别模型

统一通过 sherpa-onnx 2.0.1 Rust crate 调用，静态链接（自带 onnxruntime），`OfflineAsr` 和 `StreamingAsr` 封装在 `echonote-core::asr`。

| 模型 | 用途 | 文件大小 | 状态 |
|---|---|---|---|
| SenseVoice-Small int8（2025-09-09） | 默认实时模型，随安装包 | 226 MB | 候选，AISHELL 抽样 CER 约 3–5% |
| Qwen3-ASR 0.6B int8 | 可选高精度模型，会后重转 | 约 940 MB，峰值内存约 1.3 GB | 候选，输出繁体，展示前转简体 |
| Fun-ASR-Nano int8 | 高精度备选 | 约 960 MB | 待评测 |
| Paraformer-zh int8 | 中文备选 | 227 MB | 待评测 |
| 流式 Zipformer zh-en | 仅 S2 的预览模型 | 190 MB | 待评测 |
| Silero VAD | 默认 VAD | 0.6 MB | 采用 |
| TEN VAD | 不采用（许可证见第 15 节） | — | — |

安装包预算 300 MB：SenseVoice、Silero 加应用约 250 MB（压缩后更小）。如果选 S2，再加 190 MB 的流式模型会超预算，需要把流式模型改为首次使用时下载，或者换更小的流式模型。

加载策略：按下录音立即开始采集，模型在后台加载（M0 实测 SenseVoice 加载约 1–4 s，视机器负载），加载完成前音频在时间线里排队，加载后依次补转。空闲 10 分钟卸载模型，托盘常驻时内存低于 150 MB。

内存：M0 的 505 MB 是 bench 进程的峰值 RSS（`ru_maxrss`），不含 WebView 的辅助进程（通常 100–200 MB）。sherpa-onnx 2.0.1 没有暴露 onnxruntime 的内存池选项，要调只能自己编译 sherpa-onnx。建议把预算拆成：核心进程稳定 ≤ 450 MB、峰值 ≤ 550 MB，WebView 进程 ≤ 150 MB。这改变了 PRD 的 500 MB，需要用户确认。

积压上限：内存里只保留当前段（不超过硬上限）、预留和去重窗口需要的音频。ASR 积压超过约 60 s 时丢弃内存中的 PCM，把区间标记为待转录，之后从 Ogg 文件走恢复路径补转。

## 9. 实时调度

`echonote-bench live` 用虚拟时间模拟同一套调度：每次解码真实执行，按实测耗时推进单个工作线程的时钟。

- 一个离线解码线程服务两轨。定稿优先于预览，按就绪顺序处理。
- 预览不排队：解码线程空闲且到了预览间隔，才用当前段最新的音频生成一个预览，所以过载时自然少出预览。解码期间该段已结束或被切开的预览直接丢弃。
- 预览间隔至少 0.6 s，且不小于上一次预览耗时的 2 倍（自适应降频）。
- 过载时先丢预览，从不丢音频和定稿。停止录音后排空队列。
- 两种预览策略：
  - **S1**：用离线模型重解码正在增长的当前段。实现简单，只需一个模型；代价随段长增长，靠第 7 节的硬上限约束。
  - **S2**：流式 Zipformer 在独立线程出预览，离线模型只做定稿。代价与音频时长成正比，但多一个模型和 190 MB。
- 两轨共用一个预览名额（不是每轨一个）；疑似串音时跳过麦克风轨的预览。M1 先用系统音频轨优先近似：系统音频在说话时只给它出预览。代价是双方同时说话时麦克风轨没有预览（重放里一次首个预览等了 9.7 s），所以首个预览按轨分别统计，门禁只看系统音频轨。真正的串音判断随第 10 节去重一起做。
- 门禁以 CPU 核秒每音频秒（`cpu_s_per_audio_s`）为准：4 核的 15% 等于 0.6。M0 数据里解码线程占用 1.0 约等于 2 核秒（2 个推理线程），所以"占用 ≤ 0.5"和"CPU ≤ 15%"互相矛盾，以 CPU 为准。M2 比低配机快，要在 4 核机器上实测，或按单核性能比例折算。另外预览 p95 ≤ 1.0 s，定稿 p95 ≤ 1.5 s。M1 重放结果仍未达标：CPU 1.05 核秒（M2 机器），定稿 p95 1.9–2.8 s。主要原因是两轨同时到达硬上限、排队解码，而麦克风轨那段多半是串音，所以要靠去重跳过串音段的解码。
- S2 降为备选：常驻第二个模型会突破内存预算，190 MB 会突破安装包预算。只在 S1 无法达标时重新评估。
- CI 门禁用固定的解码耗时模型（按音频长度计算），队列和延迟门禁不受 CI 机器速度影响。

机器无关指标用于跨机器比较和 CI 门禁：`decoded_audio_per_audio_s`（重解码放大）、`decodes_per_audio_min`、`max_final_queue`、`forced_splits`、定稿单调且不重叠。

## 10. 串音去重

不戴耳机时，扬声器里对方的声音会进麦克风。M0 真机结果（MacBook Air 内置扬声器和麦克风，2 段录音）：

- 麦克风上的串音比系统音频低约 9–11 dB，ASR 几乎逐字识别出来。不去重的话对方的话会出现两遍。
- 文本相似度（麦克风 token 在同时段系统音频定稿中按顺序出现的比例）：串音段 0.71–0.99，用户自己说的段 0.00。
- 能量包络相关（0–200 ms 滞后）：串音段只有 0.52–0.93，单独用不可靠。合成评测集上它表现很好，真机上不行。
- 128 ms 线性回声模型在这两段录音上只消掉 2.7 dB（窗口里混有用户说话和房间噪声，不是干净测量），不能据此判断 AEC 效果。

v1 方案：

1. 按时间对齐 token：用 token 时间戳，只和同一时间窗里的系统音频 token 比较。两个 token 文本相同、且麦克风起点比系统音频晚 -0.3 到 0.5 s（声学延迟 0–200 ms，加上两侧各约一个 60 ms 模型帧的时间戳抖动）才算匹配，按顺序取最长匹配。M0 的整段 LCS 对中文逐字匹配，短的常见短语可能碰巧达到 0.6。
2. 串音判定按连续匹配：连续匹配不少于 3 个 token 的片段算串音；整段都匹配且不少于 2 个 token（如报时"八点"）也算。双方同时说话时零散对上的常用字不算。
3. token 级裁剪：删除串音片段后，剩余 token 不少于 4 个且占比不低于 30% 时保留剩余部分（双方同时说话时不丢用户的话），否则整段标记为串音。
4. 串音段不删除，存为 `duplicate_of` 并在界面隐藏，可以手动恢复。
5. 系统音频段最长 12 s，固定等待它的定稿会经常超时。先用系统音频的预览做临时判断；系统音频定稿确认之前，麦克风定稿按预览样式显示。
6. 检测到外放串音时提示戴耳机。
7. 门禁：误删率（用户自己的话被判成串音，对应 PRD A3）和漏判率在双方同时说话的评测集上测，M1 补这个评测集。
8. 两轨都有定稿待解码时，系统音频的定稿先解码：麦克风定稿反正要等它来判断串音。
9. AEC（WebRTC AEC3，以系统音频为参考信号）在 M1 第一周测 ERLE，需要一段干净的外放录音（只放音频、不说话、关闭其他声音）。AEC 会改变管线：麦克风处理要等参考信号，预留一个有界回看的处理阶段；外放时麦克风轨不再带对方的话，ASR 负载也会下降。原始麦克风音频始终存盘。

实现在 `echonote-core::crosstalk`，`echonote-bench live` 和 `live_demo` 共用。M1 结果（SenseVoice，只出定稿）：

| 数据 | 规则 | 召回 | 误删用户段 | 去重后麦克风错误率 |
|---|---|---|---|---|
| 合成双人会议，带外放串音（104 段，61 段串音） | M0 整段相似度 ≥ 0.6 | 0.934 | 0 | 21.4% |
| 同上 | 时间对齐 | 1.000 | 0 | 10.0% |
| 同一会议，麦克风无串音（54 段） | 时间对齐 | — | 0（无段被标记） | 10.3%（不变） |

去重后的错误率低于无串音基线，是因为双方同时说话的段里串音被裁掉了、用户的话保留下来（56 段隐藏，6 段裁剪）。两段 M0 真机录音里，外放段全部判为串音，用户自己说的一句保留。合成数据的串音路径是人工构造的，真实的双方同时说话评测集仍要在 M1 录制。

## 11. 存储

SQLite（rusqlite，bundled，SQLite 3.53，启用 FTS5）：WAL、`synchronous=NORMAL`、外键开启。一个写连接在 store 线程，界面查询走只读连接。迁移版本号存 `PRAGMA user_version`，迁移前用 `VACUUM INTO` 备份为 `<库文件>.v<旧版本>.bak`（含 WAL 里的数据）。库文件比应用新时拒绝打开。实现在 `echonote-core::store`，M1 建了下表中会议、轨道、段落、修订、设置和搜索索引，其余表随用到它们的里程碑以新迁移加入：V2（M3）加了 `glossary`。

| 表 | 主要字段 |
|---|---|
| `meetings` | id、title、started_at、ended_at、duration_ms、status（recording / finalized / recovered）、language、template_id |
| `tracks` | meeting_id、kind（mic / sys）、file、sample_rate、duration_ms、device_name、transcribed_until_ms（崩溃恢复从这里补转） |
| `segments` | meeting_id、track、start_ms、end_ms、text、raw_text（去串音或修改前的识别文本）、model、source（asr / llm / user，当前文本的来源）、crosstalk（trim / duplicate / 空）、revision |
| `segment_revisions` | segment_id、revision、text、source、created_at（当前文本在 `segments`，这里只存旧版本） |
| `summaries` | meeting_id、template_id、provider_id、content、created_at |
| `qa_messages` | meeting_id、role、content、citations（段落 id 列表） |
| `templates`、`glossary`、`llm_providers`、`settings` | 配置；API Key 不在库里，只存钥匙串条目名 |
| `jobs` | kind（retranscribe / summarize / correct）、meeting_id、state、progress、error，用于中断后恢复 |
| `segments_fts` | FTS5 `trigram` 分词，无内容表（`contentless_delete`），由触发器维护 |

trigram 至少需要 3 个字符，1–2 个字的查询退回 `LIKE`（单用户数据量下足够快）。FTS 只索引当前可见的文本：串音重复段不进索引，恢复后才加入；修改后旧文本移出索引。用无内容表而不是外部内容表，是因为外部内容表删除时要提供旧值，而重复段本来就不在索引里。查询按短语匹配，用户输入里的 FTS 语法和 `LIKE` 通配符都按字面处理。

串音段落的 `crosstalk` 标记取代原设计的 `duplicate_of`：判定依据是系统音频的 token，常常还没有对应的系统段落（预览阶段就判定了）。恢复重复段只清除标记；恢复裁剪段把 `raw_text` 作为用户修改写回，旧文本进修订表。

`commit_finals` 在一个事务里写入一批定稿并推进各轨 `transcribed_until_ms`（只增不减）。store 只管数据行，录音文件的创建和删除由调用方负责。

高精度重转会产生不同的段落边界，按段落记录的修订和问答引用会失效。重转在原有段落边界内解码；需要重新分段时保存为新的转录版本。

文件布局：`<数据目录>/meetings/<id>/mic.opus`、`sys.opus`。删除会议时删除整个目录和所有行。

## 12. 录音文件与回放

- 每轨一个 Ogg/Opus 文件：16 kHz 单声道，24 kbps VBR，20 ms 帧（libopus）。每小时每轨约 11 MB。
- 约每秒写出一个 Ogg 页并 flush，应用崩溃最多丢 1 秒音频。flush 防不了断电：后台线程每 10 秒 fsync 一次（macOS 用 `F_FULLFSYNC`），不在 capture 线程上做。
- 每批定稿写库时记录每轨"已转录到"的位置。启动时扫描 `status = recording` 的会议，读出 Ogg 实际时长，从该位置补转尾部，标记为 recovered。

M1 实现（`echonote-core::recording`，libopus 在 `crates/echonote-opus`）：

- libopus 1.6.1 的源码放在仓库里（约 3 MB，只有纯 C 浮点编解码器，不含 DNN 和 SIMD），用 `cc` 编译。现有 Rust 绑定（opusic-sys、audiopus_sys、opus-static-sys、libopus_sys、opus-codec、opus-head-sys）都要求 CMake，而且 macOS 上的 Windows 类型检查也会去跑 CMake。`scripts/dev/vendor_opus.py` 从 Xiph 下载并校验 sha256，按上游的 `*_sources.mk` 取文件。`DOCS_RS=1` 时跳过 C 编译，Windows 类型检查命令不变。
- 编码复杂度取 5。M2 Air（机器有其他负载）上用 AISHELL-1 抽样 598 条（3033 s）测：复杂度 10、5、3 的编码速度分别是实时的 116、224、312 倍（占一个核的 0.86%、0.45%、0.32%）；解码约 2400 倍，3 小时的文件 5 秒内解完。码率含 Ogg 开销 22–23 kbps，每小时每轨约 10 MB。x64 在 Rosetta 下编码 202 倍。
- 对识别的影响（SenseVoice，`--no-itn`）：原始 WAV CER 3.59%，Opus 往返后复杂度 5 为 3.72%、复杂度 3 为 3.66%，差别是 8794 字里多错 6–11 个字，复杂度之间的差异在噪声范围内。高精度重转和崩溃恢复都从 Ogg 解码，这个代价可以接受。
- 每页一次写入，不经过用户态缓冲。崩溃时丢失的是正在积累的那一页和编码器延迟（6.5 ms）。`Syncer` 线程在开始时、每 10 秒和结束时调用 `sync_data`（macOS 上就是 `F_FULLFSYNC`），并同步所在目录，让新文件名落盘。
- 时间线：文件第 n 个样本就是写入的第 n 个样本。编码器延迟写成 Ogg pre-skip，最后一页的 granule 位置截掉末帧补零。ffmpeg 和 WKWebView 的 `decodeAudioData` 解出的长度都与写入的样本数一致。
- 恢复：`probe` 逐页校验 CRC，遇到截断或损坏的尾页就停，报告可解码的样本数；`repair` 截掉坏尾；`Reader::open_at` 从任意位置解码，提前 80 ms 预滚，预滚足够时与从头解码逐位一致。用 `live_demo` 回放实测：`kill -9` 后文件保留 10.993 s，同一次运行的 WAV 副本有 11.774 s，丢失不到一页。启动扫描和补转随会话状态机实现。
- WKWebView（macOS 15.7.7）能加载、跳转、播放正常结束的文件和进程被杀后的文件；`decodeAudioData` 时长与写入一致（24.67 s、10.99 s），`audio.duration` 偏长 2.1–2.6%（25.2 s、11.28 s）。

Spike E 结果（macOS 15.7.7 系统 WKWebView，`scripts/dev/webview_audio_probe.sh`，20 s 测试音）：

| 格式 | canPlayType | `<audio>` 加载/跳转/播放 | decodeAudioData | MSE |
|---|---|---|---|---|
| Ogg/Opus | probably | 通过；duration 报 20.56 s（实际 20.007 s） | 通过 | 否 |
| WebM/Opus | probably | 通过 | 通过 | 是 |
| CAF/Opus | probably | 通过 | 通过 | 否 |
| MP4/Opus | 否 | 能播 | 失败 | 否 |
| AAC (m4a) | probably | 通过 | 通过 | 是 |

结论：录音文件用 Ogg/Opus（逐页校验，截断后仍可播放）。

M1 长文件跳转（`scripts/dev/webview_seek_probe.sh`，macOS 15.7.7 系统 WKWebView）：用录音写入器生成 3 小时带时间码的文件（`opus_tool timecode`：每 0.2 s 一个音高，每 5 s 的前 1 s 用 5 个音高编码周期序号；奇数个 10 分钟是静音，码率随位置变化，和会议录音一样），同样的音频另存一份 WAV。每次跳转后经零增益的 Web Audio 图播放 7.5 s（听不到），从音高解出实际播放位置，和 `currentTime` 比较，第二次跳到 5 s 的结果作为输出延迟的基准。

| 文件 | `audio.duration` | 跳转后实际位置减 `currentTime` |
|---|---|---|
| Ogg/Opus 3 h（33 MB） | 6560 s（实际 10800 s） | 590 s 处 −19 s；1234–5124 s 处 +462 s 到 +3361 s；6560 s 以后的 4 个目标被截到 6560 s，没有声音 |
| WAV 3 h（345 MB） | 10800 s | 10 个目标全部在 ±27 ms 以内 |

WebKit 不给 Ogg 建索引，按平均码率把时间换算成字节位置，VBR 加上长时间静音就会错得很远，20 分钟的文件已经差 19–24 s。所以回放改为：Tauri 自定义协议从 Ogg/Opus 现场解码，输出 16 kHz 16 位单声道 WAV（两轨混成一路），把 Range 请求换算成样本位置，用 `Reader::open_at` 只解码请求的区间。WAV 每秒 32 KB，`<audio>` 的时长和跳转都按字节精确换算。录音文件格式不变；WebView2 走同一条路径，两个平台行为一致。协议只接收会议 id，作用域限定在 `meetings/` 目录。点击段落跳转的精度（目标 ±100 ms）在 M3 的应用里复测，macOS 14.2 的 WebKit 仍未验证。

M3 实现：

- `echonote-core::recording::playback`：`MixedWav` 按字节区间解码，各轨样本相加后限幅到 16 位，缺失的轨按静音处理；区间可以从头部或半个样本中间开始。`parse_range` 处理 `bytes=` 区间（含后缀形式），多个区间只返回第一个，越界或格式错误返回 416。每次响应最多 2 MiB（约 65 s 音频），播放器自己续请求。
- 应用的 `echonote-audio` 协议在阻塞线程池里处理：从库里查轨道，文件名只取 `file_name()` 拼到该会议目录下；时长取各轨最长的 `duration_ms`。录音期间返回 409，界面隐藏播放器并提示录音结束后可回放（系统音频 tap 会录到 WebKit 进程的声音）。响应带 `Cache-Control: no-store`，会议时长变化（恢复补转）时播放器重建。
- 暂停的时间不进时间线，录音文件也是连续的，所以段落的 `start_ms` 直接就是文件里的位置。
- 点击段落时间戳：等 `loadedmetadata` 后设 `currentTime` 再播放。冒烟测试（静音点击第三条）：45 s 的会议，`duration` 45.29 s 与轨长一致，跳到 5.798 s 后 `currentTime` 5.80 s，15 ms 内开始播放，没有 CSP 违规。

## 13. 其他模块

**模型管理**：清单记录 id、版本、文件、大小、sha256 和镜像地址（ModelScope 优先，其次 Hugging Face、GitHub）。下载断点续传，校验后原子改名，下载前检查磁盘空间。沿用 `scripts/dev/fetch_assets.py` 的逻辑，改用 Rust reqwest，跟随系统代理。

**LLM**：

- 支持三类接口：OpenAI 兼容（含国内各家和 LM Studio）、Anthropic Messages、Ollama。共用一个流式接口抽象。
- 请求只从 Rust 发出，WebView 拿不到 Key，也没有 CORS 问题。
- 总结按模板生成。转录超过上下文窗口的 70% 时分段总结再合并。
- 会后问答：转录放得进上下文就整段放入，否则用 FTS 召回相关段落。回答附段落引用，可以点击跳转。
- 转录纠错：按批发送定稿和术语表，返回结果按 diff 写成新的 revision，可以撤销。
- 会中要点：每 3 分钟（有新定稿时）生成一次，失败不影响录音。
- 第一次向某个远程服务发送转录前弹窗确认。任务只在内存里，退出后由用户重新开始（见决策 15）。

**IPC**：Tauri 命令加事件。实时事件在 Rust 侧合并，最多 10 Hz，内容是段落增量（新增或更新）和电平。界面用虚拟列表，每轨只保留当前预览。

**安全**：严格 CSP（不加载远程脚本，不允许内联脚本）；不启用 shell 插件；capabilities 只开放用到的命令；文件访问限定在数据目录；API Key 存系统钥匙串；日志不记录转录文本和 Key；非 HTTPS 的 LLM 地址只允许 localhost，否则需要用户确认。转录可能带有提示注入（会上有人念出指令），所以 LLM 输出的 Markdown 渲染前要清理，CSP 的 `img-src` 只允许 `'self' asset: data:`，防止注入的图片链接把转录内容带出去。LLM 请求关闭重定向（reqwest 跨主机重定向时会保留 `x-api-key` 这类自定义头）。IPC 命令只接收 id，不接收文件路径。

**应用壳（M2，`app/`）**：

- 前端 Svelte 5 + Vite，构建产物只有外链的 JS 和 CSS。CSP 为 `default-src 'self'`，`script-src`、`style-src` 都不允许内联；动态样式只用 `style:` 指令（CSSOM 赋值，不受 CSP 限制），不用 Svelte 过渡（会插入 `<style>`）。capability 只有一个：按名字允许应用自己的命令，加上事件监听；不开放任何插件 API、文件或 shell。
- 引擎事件由 `live` 线程合并，每 100 ms 最多发一次增量：电平取峰值，预览和通知发当前值，定稿按提交顺序。窗口在录音中打开时先订阅再取快照（定稿从库里读），按段落 id 去重。
- 关闭窗口会销毁 WebView，应用留在托盘，内存随之释放；再次打开时按配置重建窗口（Windows 上不能在事件回调里同步建窗口，所以放到线程里）。最后一个窗口关闭触发的退出请求被拦下；退出只走自己的 Quit（托盘和 macOS 应用菜单），先结束录音再退出。
- 设置作为一个 JSON 存在库的 `settings` 表；全局快捷键默认 `CmdOrCtrl+Shift+Alt+R`，注册失败时保留旧的并提示。
- 权限：麦克风在开始录音前询问；macOS 系统音频没有公开的查询接口，用 `TCCAccessPreflight` 判断，拒绝时不建 Tap，只录麦克风并提示去系统设置；Windows 读麦克风隐私开关，关着时拒绝开始录音。
- 日志在 `<数据目录>/logs/app.log`，1 MB 轮转，只记录事件类型和错误码。
- 调试构建的开关：`ECHONOTE_DATA_DIR`、`ECHONOTE_FAKE_INPUT`、`ECHONOTE_SMOKE`。`scripts/dev/app_smoke.sh` 把合成会议的前 45 s 通过真实界面走一遍：开始到采集 292 ms，无 CSP 违规，每秒 8.8 次增量，13 段里 3 段串音隐藏，显示说话方标签，日志里没有转录文本。

**编辑与词表（M3，`echonote-core::glossary`、`app/src-tauri/src/edit.rs`）**：

- 段落编辑写成 `source = user` 的新修订，旧文本进修订表。"还原"用 `restore_revision(段落, 1)` 把识别原文作为又一次修订写回，所以还原本身也能撤销；M4 的 LLM 纠错逐段回退用同一个接口。串音的"显示""补回"走原来的 `restore_crosstalk`。正在录音的会议不能编辑（实时视图不会收到编辑）。
- 词表是迁移 V2 的 `glossary` 表：词（忽略 ASCII 大小写唯一）和它常见的错误写法，每行一个。最多 500 个词，每个词 20 个写法，每项 60 个字符。
- 替换只在显示和导出时做，库里存的仍是识别或用户输入的文本，改词表对所有会议立刻生效，也不会污染修订历史。会议详情、实时增量（定稿和预览）、快照、搜索结果都经过同一个 `Glossary::apply`。匹配从左到右，每个位置取最长的；词本身也参与匹配但保持原样，所以 "EchoNote" 不会因为 "Echo" 是别的词的写法而被改；替换结果不再参与匹配。ASCII 字母不区分大小写，写法以字母或数字开头/结尾时要求词边界；中文没有词边界，精确匹配。
- 搜索：查询里含有某个词或它的写法时，换成该词的所有写法一起查（FTS 短语 OR，1–2 个字的写法走 `LIKE`），结果再经过替换显示。
- 词表的词按添加顺序给高精度模型做热词、给 LLM 纠错做提示（M4、M5 接入）。
- 冒烟测试在真实界面上双击第一行编辑、按 Enter 保存、点"还原"，再加一条把这行前两个字当写法的词条，检查显示和搜索都换成了词，删掉后恢复原样；日志里没有编辑内容和词表内容。

**导出、删除与存储（M3，`echonote-core::export`、`app/src-tauri/src/files.rs`）**：

- 导出是 core 里的纯函数：标题、日期、时间、时长、标签几行元信息，然后 `## 转录`，每段一行 `[mm:ss] 我：文字`（超过 1 小时为 `hh:mm:ss`），与旧插件笔记的结构和转录解析格式一致。段落之间空一行，任何渲染器里都是独立段落；隐藏的串音不导出，只有对方说过话才加说话方标签，词表替换后输出。Markdown 里会被 Markdown 或 Obsidian 当成格式、链接、标签、HTML、数学公式的字符加反斜杠；TXT 不转义。时间按本地时区：界面传会议开始时刻的 UTC 偏移，core 不依赖时区库。文件名是"标题 日期.md"，无标题时为"EchoNote 日期 时分.md"，去掉 Windows/macOS 不允许的字符。
- 保存对话框由 Rust 打开（`tauri-plugin-dialog`，挂在主窗口上），WebView 没有对话框或文件权限，只传会议 id 和格式。默认打开上次导出的文件夹（库里的 `export.dir`），没有时用"文稿"。调试构建可用 `ECHONOTE_EXPORT_DIR` 跳过对话框，冒烟测试用它检查导出文件。
- 删除由 `Engine::delete_meeting` 做：正在录音的会议拒绝；状态仍是 recording 的会议在启动恢复跑完之前拒绝（可能马上要恢复），恢复失败的之后可以删。先把会议目录改名为 `.trash-<id>`，再删库里的行（外键级联到轨道、段落、修订，FTS 由触发器清理；M4 的总结和问答表同样级联），失败时把目录改回来；最后删除改名后的目录，失败的留给下次启动清理。所以任何一步失败，要么什么都没删，要么只剩一个启动时会清掉的垃圾目录。
- 存储用量：设置页显示录音目录和库文件（含 WAL）的大小，会议详情显示这场会议录音的大小，都是直接统计文件。
- 冒烟测试导出两种格式（Markdown 26 行，9 行带时间戳，全部有说话方标签），在界面上点删除、确认，检查回到首页、列表里没有这场会议、录音目录释放 203 KB、`get_meeting` 返回 not-found。同一轮里点击时间戳到开始播放用了 2.7 s，当时机器负载 13（其他进程），空闲时为 15 ms。

**AI 功能（M4，`crates/echonote-llm`、`app/src-tauri/src/ai/`、`secrets.rs`）**：

- `echonote-llm` 是独立的 crate，只依赖 reqwest（native-tls：macOS 用 Security.framework，Windows 用 SChannel，不带 aws-lc）、serde、tokio 的 sync/time。两种接口：OpenAI 兼容（`/chat/completions`，Bearer）和 Anthropic Messages（`/messages`，`x-api-key`）；Ollama、LM Studio 和国内各家都走 OpenAI 兼容接口。SSE 自己解析，也接受不流式的 JSON 回答，过滤 `<think>` 块。错误分成固定的类别（密钥无效、额度不足、限流、找不到模型、超出上下文、连不上、超时、中途断开等），每类一个错误码给界面翻译；服务商的原话截到 300 字，只在界面上显示，日志只记错误码。只在还没有输出任何文字时重试（最多 3 次，指数退避，遵守 `retry-after`），已经输出过再断开就报"中途断开"，避免重复内容。400 时各试一次 `max_completion_tokens` 和去掉 temperature 两种写法。不跟随重定向；http 只允许本机地址，局域网服务要用户勾选；地址里不能带用户名密码；密钥和请求头的值在 Debug 输出里打码。
- 服务商配置是库里 `settings` 表的 `ai` 一项（JSON）：名称、预设、接口、地址、模型、上下文窗口、最多输出、代理、是否允许 http、额外请求头的名字。API Key 和请求头的值不进库，存在系统钥匙串（keyring crate：macOS Keychain、Windows 凭据管理器），服务名 `com.rebill.echonote`（调试构建加 `.debug`），每个服务商一项，内容是 JSON。表单里留空表示保留已保存的值，只有这时才读钥匙串，所以第一次保存新 Key 不会等授权弹窗。保存前用和请求相同的方式检查地址、代理和请求头（只允许可见 ASCII，防止换行注入请求头）。冒烟测试把密钥放在内存里，不碰钥匙串。钥匙串里的项被删掉时报"钥匙串里找不到保存的 API Key"，不发请求。
- 第一次把转录发给某个远程服务前，界面写明接收方（`scheme://host:port`）并要求确认；确认记在这个服务商上，地址换到别的服务器时重新询问，本机服务不询问。Rust 侧在开始任务时再检查一次，界面绕不过去。连接测试只发 "Reply with OK."，不带转录，不需要确认。
- 总结：模板有通用会议、周会、访谈、课程四个内置的，加上用户自定义（`templates` 表，最多 50 个）。转录按导出的 TXT 格式给模型（`[mm:ss] 我：文字`，词表替换后，隐藏的串音不发）。放得进 70% 上下文就一次流式生成，否则按行边界切段，并发 2 路分段做笔记，再分层合并，最后一轮流式输出。服务器实际窗口比配置的小时（报"超出上下文"），把这次的提示长度乘 0.85 记为上限，切得更细重来，所以配置偏大也能完成（A11）。第一行 `# 标题` 拆出来，会议没有标题时用它命名，用户改过的标题不覆盖。总结存 `summaries` 表，每次生成新增一行，显示最新的；导出时放在转录前面。
- 问答：转录放得进就整段放入，否则按问题做 BM25 召回（英文按词，中文按单字和相邻两字），带上命中行的前后各一行；附最近 6 轮问答和最新的总结。回答要求每个事实带 `[mm:ss]`，界面上可以点击跳到那里播放。问答存 `qa` 表，可以清空。
- 纠错：只发可见的段落，按批发送 JSON（批内用 1..n 的短 id，不发库里的 id），模型只返回改过的行。逐条检查：id 认识、只有一行、不为空、20 字以上的行长度在原来的 0.4–1.8 倍之间；不合格的丢掉并计数。失败的批次跳过，其余照常（密钥无效、连不上这类错误直接结束整个任务）。通过的改动用 `apply_corrections` 写成 `source = llm` 的新修订，同时在 `llm_corrections` 记下改之前的修订号；用户在模型运行期间改过的段落跳过。界面上这些段落显示逐字差异（中文按字、英文按词），可以逐条接受、回退，或全部接受；回退把改之前的文本作为新修订写回。用户手动编辑一段时，这段的待确认标记自动清除。
- 会中要点：默认关闭，在设置或录音界面开启。独立线程每 5 秒看一次：正在录音、已开启、距上次满 3 分钟、有新的定稿时，把新定稿和当前要点发给模型，返回最多 8 条要点替换旧的，存成这场会议的 `highlights` 一行。只读库（共享的只读连接）和写一行结果，不碰录音和识别线程；失败只在要点面板显示原因，下一轮再试（A10）。
- 任务：总结、问答、纠错由命令启动，先同步检查服务商、确认、钥匙串和转录，有问题直接返回给按钮；然后在 tauri 的异步运行时里执行，通过 `ai` 事件发流式文字（最多每 50 ms 一次）、进度、结果或错误。每场会议每种任务同时只能有一个，可以取消；删除会议时取消它的任务。任务状态只在内存里，退出时进行中的任务丢弃，用户重新点一次即可（设计里提过的可恢复 `jobs` 表没有做，见决策 15）。窗口重开时用 `running_jobs` 找回进行中的任务。
- 渲染：模型输出的 Markdown 在 Rust 侧先把图片和 HTML 变成文字，前端再用自己的子集解析器（标题、段落、列表、任务项、引用、代码、表格、加粗、斜体、行内代码）生成元素，不用 `{@html}`。链接只显示文字，图片只显示替代文字，所以转录里的提示注入不能让界面加载或打开任何地址。`[mm:ss]`、`[h:mm:ss]` 和 `[1:00, 2:30]` 变成时间引用按钮。
- 日志只记录任务类型、结果和错误码（如 `notes failed: llm-unauthorized`），不记录地址、提示、回答和转录。
- 冒烟测试用 `scripts/dev/mock_llm.py`（本机的假 OpenAI 服务，按任务返回固定回答，日志只记任务类型）：错误的 Key 报 `llm-unauthorized`；测试连接、获取模型列表、保存服务商；录音期间要点面板出现要点（间隔用 `ECHONOTE_HIGHLIGHTS_SECONDS=10` 缩短）；生成总结，标题自动改为模型给的主题，点总结里的时间引用开始播放；换成慢速模型，流式中途停止，原来的总结保留；提问，回答里有可点击的时间；AI 纠错改了 2 处，接受 1 处、回退 1 处，最后没有待确认的；Markdown 和 TXT 导出都带总结；应用日志里没有 Key、地址、转录或回答的内容。

**测试与 CI**：

- core 单元测试使用假源和 WAV 源；性质测试检查定稿单调、不重叠、不丢音频。
- `echonote-bench` 门禁使用冻结的小型夹具，检查不变量、`decoded_audio_per_audio_s`、队列上限和错误率。
- 前端：vitest 测组件；Playwright 跑 Vite 开发服务器，IPC 用 mock。macOS 没有 WKWebView driver，不做 tauri-driver 端到端测试。
- GitHub Actions 跑 macos-14 和 windows-latest，缓存 sherpa 归档，执行 fmt、clippy、test 和 bench 冒烟。评测音频不能提交和再分发，CI 只用可以公开的夹具。新增测试要确认被发现并执行。

M1 实现：`echonote-bench gate` 有 3 个合成夹具（3 分钟双方轮流说话，麦克风带低 10 dB、晚 50 ms 的外放串音，每三轮一次抢话；90 s 连续语音；20 分钟每分钟一句）。语音用音调代替，模型用 `FakeModel`（按音调的八度出字，串音和原声出同样的字），VAD 用能量检测，解码耗时按固定模型（0.03 s + 0.1 × 音频秒）计算，所以队列和延迟数字在任何机器上都一样。门禁项：不变量、`decoded_audio_per_audio_s`、`decodes_per_audio_min`、最大定稿队列、强制切分次数、定稿 p95、停止后排空时间、系统轨首个预览 p95、内存里最多持有的音频、丢失的字、最长定稿、误删用户段、串音隐藏比例。阈值在 `crates/echonote-bench/gates.json`，`--write-baseline` 按当前值加 10% 余量重写。本机两次运行输出逐字节相同；把解码耗时调到 4 倍，8 项延迟门禁失败。真实模型的错误率不进 CI（评测音频不能再分发），用 `echonote-bench eval` 在本机跑。工作流在 `.github/workflows/desktop.yml`；sherpa-onnx-sys 自己下载归档时不校验，工作流先用 `scripts/dev/fetch_sherpa_onnx.py` 按发布页的 sha256 下载并校验，再通过 `SHERPA_ONNX_ARCHIVE_DIR` 交给构建。

**打包**：

- macOS 每个架构出一个 DMG（不做 universal，避免二进制翻倍）。用 ad-hoc 签名，首次打开需要右键打开或执行 `xattr -dr com.apple.quarantine`。
- Windows 用 NSIS，静态 CRT（`.cargo/config.toml` 已设置 `+crt-static`），不需要 VC++ 运行库，带 WebView2 引导程序。没有签名，SmartScreen 会提示。
- 目前测得的二进制大小（bench，含 sherpa 和 onnxruntime）：arm64 19.3 MB，x64 22.6 MB。

## 14. M0 验证结果

| Spike | 结果 | 未完成 |
|---|---|---|
| D 静态链接 | macOS arm64、x64 通过，只依赖系统库；x64 在 Rosetta 下输出与 arm64 一致；Windows 目标通过类型检查（在 macOS 上：`LIBSQLITE3_SYS_USE_PKG_CONFIG=1 DOCS_RS=1 cargo check --target x86_64-pc-windows-msvc`，前者让 SQLite 不在没有 MSVC 头文件的机器上编译 C 代码） | Windows 实际链接（MT 静态库 + `+crt-static`）和二进制大小 |
| B 系统音频 | macOS 15.7.7 通过，见 5.2 | Windows loopback 实测；macOS 14.2 首次授权流程 |
| C 串音 | 真机结论见第 10 节；发现分段问题，加硬上限，见第 7 节 | 干净外放录音测 ERLE；AEC3 评估 |
| E 回放 | macOS 15.7.7 结果见第 12 节；M1 长文件测试后改为现场解码成 WAV | macOS 14.2、WebView2 |
| A 模型评测 | 只有在高负载机器上跑的初步数据 | 完整矩阵需要空闲时段重跑（`scripts/dev/run_m0_bench.py`，Windows 用 `windows_m0.ps1 -Step bench`） |

初步数据（M2 MacBook Air，机器上有虚拟机负载，只作参考）：

- SenseVoice，AISHELL-1 抽样：前 30 条 CER 5.0%，部分完整运行到 300 条时约 3.2%。
- 合成双轨会议，S1：定稿 p95 1.22 s，首个预览 p95 756 ms，解码线程占用 0.31，整机 CPU 7.6%（8 核）。4 核低配机上会接近 15% 的上限。
- 峰值内存约 505 MB，稳定后 400–440 MB，略超 500 MB 预算。M1 尝试关闭 onnxruntime 内存池扩张、复用解码缓冲。
- 合成集上麦克风轨 CER 10.7%，系统轨 5.0%，原因待查（finals 已输出，可以逐句对比）。

待门禁决定（空闲时段评测后）：

1. 默认实时模型（SenseVoice 或 Paraformer）和高精度模型（Qwen3-ASR 或 Fun-ASR-Nano）。
2. S1 还是 S2，以及硬上限取 8 s 还是 12 s。
3. 解码线程数（1、2 或 4）。

## 15. 许可证

| 组件 | 许可证 | 处理 |
|---|---|---|
| sherpa-onnx | Apache-2.0 | NOTICE 中列出 |
| onnxruntime | MIT | NOTICE 中列出 |
| SenseVoice-Small、Paraformer-zh | FunASR Model Open Source License v1.1 | 必须注明来源、作者并保留模型名称；条款写明模型"仅供参考和学习"，没有提到商用；禁止贬损 FunASR，违反任何条款即自动终止许可。ModelScope 页面标的是 Apache 2.0，两者不一致，按更严格的 FunASR 许可执行 |
| Qwen3-ASR 0.6B、Fun-ASR-Nano-2512 | Apache-2.0（Hugging Face 模型卡） | NOTICE 中列出 |
| Silero VAD | MIT | NOTICE 中列出 |
| TEN VAD | Apache-2.0 加附加条款：不得以与 Agora 产品竞争的方式部署，只能用于自己的应用和直接终端用户 | 默认不打包，bench 里保留对比选项 |
| 流式 Zipformer zh-en | 待确认 | S2 采用前确认 |
| Tauri、cpal、rubato、rusqlite、Svelte | MIT 或 Apache-2.0 | NOTICE 中列出 |
| SQLite | 公有领域 | — |
| libopus | BSD-3-Clause | NOTICE 中列出；源码随仓库分发，保留 `crates/echonote-opus/opus/COPYING` |
| AISHELL-1、LibriSpeech | 评测用 | 不提交、不再分发 |

目前是自用和源码分发，FunASR 许可的限制可以接受。如果以后要公开发布安装包或商用，需要重新评估默认模型。

## 16. 决策记录

| # | 决策 | 理由 |
|---|---|---|
| 1 | 单进程 Tauri 2 + Rust，不用 Python sidecar | 安装简单，内存低，没有进程间音频传输 |
| 2 | 静态链接 sherpa-onnx | 单个二进制，不处理 dylib 和 rpath；arm64、x64 已验证 |
| 3 | 按音轨区分"我"和"对方"，v1 不做说话人分离 | 用户确认；减少模型和 CPU 开销 |
| 4 | 数据存 SQLite，Markdown 只用于导出 | 用户确认；支持搜索、修订和恢复 |
| 5 | EchoNote 是唯一的分段器，VAD 只判断语音；硬上限 12 s，切点重叠 0.4 s | M0 发现 sherpa 的 max_speech 不能切开连续语音 |
| 6 | 录音用 Ogg/Opus 16 kHz 24 kbps；回放由自定义协议现场解码成 WAV | 崩溃安全；WKWebView 对长 Ogg 文件的时长和跳转都不准（3 小时文件偏差达 56 分钟），WAV 精确到 ±27 ms |
| 7 | 默认 VAD 用 Silero，不用 TEN | TEN 附加条款有竞争限制 |
| 8 | 串音去重以文本相似度为主 | 真机上能量相关不可靠；AEC 待 M1 验证 |
| 9 | S1 优先，S2 降为备选 | S2 突破内存和安装包预算；S1 是否达标待空闲时段评测 |
| 10 | 实时门禁以 CPU 核秒每音频秒为准 | 解码线程占用与 CPU 预算的换算关系由 M0 数据得出 |
| 11 | 分段器和调度器是 core 里同一个状态机，时钟由调用方传入 | bench 测的就是产品代码，CI 门禁可以确定性复现 |
| 12 | libopus 源码放进仓库，用 `cc` 编译，不用 CMake 绑定 | 构建不依赖 CMake；macOS 上的 Windows 类型检查照常可用；只编纯 C，所有目标一种配置 |
| 13 | API Key 和请求头的值存系统钥匙串，每个服务商一项 JSON；库里只有请求头的名字 | PRD 要求不进库和日志；一项一个服务商，删除时一起清掉 |
| 14 | 转录确认按服务器地址（scheme、host、port）记录，本机服务不询问 | 换了接收方就要重新同意；本机服务不会把数据带出电脑 |
| 15 | AI 任务只在内存里，不做可恢复的 `jobs` 表 | 总结和问答几十秒内完成，重新点一次的代价小；持久化要处理半截的流式结果和重复写入，复杂度不值得 |
| 16 | LLM 纠错改的是词表替换后的文本，写成新修订，用 `llm_corrections` 记录待确认 | 词表替换不算成改动；回退和撤销复用修订表，不另存一份文本 |

## 17. 风险

| 风险 | 影响 | 应对 |
|---|---|---|
| ad-hoc 签名导致每次升级重新弹两个授权窗 | 升级体验差 | 自签名证书签名（M2 验证），文档说明 |
| macOS 14.2–14.3 上 Process Tap 行为未验证 | 最低版本可能要提高到 14.4 | 找 14.x 机器或虚拟机实测 |
| Windows 10 没有进程回环 | 回放时会录到自己的声音 | 录音期间禁用回放 |
| 内存预算口径未定；sherpa 不暴露 onnxruntime 内存池选项 | 500 MB 可能达不到 | 按 8 节拆分预算，需要用户确认；必要时自己编译 sherpa-onnx |
| ad-hoc 签名下钥匙串访问可能每次升级都弹窗 | API Key 读取体验差 | 同一构建读写自己的项不弹窗（M4 实测）；升级后签名变了会询问一次，选"始终允许"即可，写进故障排查文档；自签名证书可避免 |
| App Nap 降低后台录音时 capture 轮询的频率 | 音频积压、延迟变大 | 录音期间 `beginActivity` 声明用户活动 |
| 暂停的语义（时间线是否继续） | 时间戳和回放错位 | M1 状态机定义：暂停期间不写音频，时间线记录暂停区间 |
| 选 S2 时安装包超过 300 MB | 超预算 | 流式模型首次使用时下载 |
| 合成集上麦克风轨错误率偏高 | 用户自己的话识别变差 | M1 前查清原因 |
| 用户真实会议录音还没有 | 评测集代表性不足 | 用户提供 3–5 段后补测 |

## 18. 架构评审

M0 草案经过一次架构评审（architect-reviewer）。采纳的修改已写入上文：第 7 节唯一分段器，第 9 节门禁口径和共用预览名额，第 8 节内存口径和积压上限，第 10 节时间对齐去重和 AEC 时间点，第 5、6 节时间戳与 Windows 静音流，第 11、12 节持久化和重转版本，第 13 节提示注入防护。
