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

## 5. 音频采集

所有源实现同一个 `AudioSource` trait：`start(sink)`、`stop()`、`format()`，以及设备变更事件。测试用假源和 WAV 文件源。

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

## 7. 分段

M0 发现 sherpa-onnx 的 `max_speech_duration` 不能切开连续语音。它在超时后提高阈值等待停顿，而播客、视频和语速快的人没有足够长的低概率区间。真机外放录音里单段长到 15–32 秒，S1 重解码放大到 4.8–11 倍，定稿 p95 延迟 10.5 秒。

M0 的模拟器（`sim.rs`）先用叠加方式验证：sherpa 照常分段，段长达到 12 s（`--hard-cap`）时，EchoNote 在最近 2 s 内能量最低的 20 ms 帧处切开并立即定稿，sherpa 之后送出的段落只取切点之后的部分。真机录音上 p95 定稿延迟从 10.5 s 降到 2.7 s，解码线程占用从 1.20 降到 0.84，用户自己的一句话不再和串音合并在同一段里。

叠加方式下两个分段器管理同一段音频：sherpa 仍然缓存整段，`max_speech` 仍会抬高阈值，短于 0.25 s 的残段被丢弃，切点可能落在字中间。M1 改为 EchoNote 是唯一的分段器：

- VAD 只输出每个窗口是否为语音，每次切分后 flush。
- 定稿是统一时间线缓冲上的样本区间 `[start, end)`，不复制 sherpa 返回的样本。
- 句尾静音 0.5 s 结束一段，最短语音 0.25 s，起点向前保留 2 个窗口加最短语音时长。
- 硬上限切分时前后两段重叠约 0.4 s，用 token 时间戳去掉重叠部分的重复字（`OfflineAsr::decode` 目前丢弃时间戳，M1 保留）。
- 分段器和第 9 节的调度器写成 `echonote-core` 里的同一个状态机，时钟由调用方传入。应用里的真实线程和 `echonote-bench` 驱动同一份代码。
- 硬上限取 8 s 还是 12 s 由 `continuous-zh` 评测集决定（第 14 节）。

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
- 每轨最多一个待处理的预览请求，执行时解码当时已有的最新音频。
- 预览间隔至少 0.6 s，且不小于上一次预览耗时的 2 倍（自适应降频）。
- 过载时先丢预览，从不丢音频和定稿。停止录音后排空队列。
- 两种预览策略：
  - **S1**：用离线模型重解码正在增长的当前段。实现简单，只需一个模型；代价随段长增长，靠第 7 节的硬上限约束。
  - **S2**：流式 Zipformer 在独立线程出预览，离线模型只做定稿。代价与音频时长成正比，但多一个模型和 190 MB。
- 两轨共用一个预览名额（不是每轨一个）；疑似串音时跳过麦克风轨的预览。
- 门禁以 CPU 核秒每音频秒（`cpu_s_per_audio_s`）为准：4 核的 15% 等于 0.6。M0 数据里解码线程占用 1.0 约等于 2 核秒（2 个推理线程），所以"占用 ≤ 0.5"和"CPU ≤ 15%"互相矛盾，以 CPU 为准。M2 比低配机快，要在 4 核机器上实测，或按单核性能比例折算。另外预览 p95 ≤ 1.0 s，定稿 p95 ≤ 1.5 s。
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

1. 按时间对齐 token：用 token 时间戳，只和同一时间窗（加 0–200 ms 声学延迟）里的系统音频 token 比较。M0 的整段 LCS 对中文逐字匹配，短的常见短语可能碰巧达到 0.6。
2. 段级规则：对齐后相似度 ≥ 0.6、且不少于 4 个 token 的麦克风段标记为串音。
3. token 级裁剪：对标记段删除对齐上的 token；剩余 token 不少于 4 个且占比不低于 30% 时保留剩余部分（双方同时说话时不丢用户的话）。
4. 串音段不删除，存为 `duplicate_of` 并在界面隐藏，可以手动恢复。
5. 系统音频段最长 12 s，固定等待它的定稿会经常超时。先用系统音频的预览做临时判断；系统音频定稿确认之前，麦克风定稿按预览样式显示。
6. 检测到外放串音时提示戴耳机。
7. 门禁：误删率（用户自己的话被判成串音，对应 PRD A3）和漏判率在双方同时说话的评测集上测，M1 补这个评测集。
8. AEC（WebRTC AEC3，以系统音频为参考信号）在 M1 第一周测 ERLE，需要一段干净的外放录音（只放音频、不说话、关闭其他声音）。AEC 会改变管线：麦克风处理要等参考信号，预留一个有界回看的处理阶段；外放时麦克风轨不再带对方的话，ASR 负载也会下降。原始麦克风音频始终存盘。

## 11. 存储

SQLite（rusqlite，bundled，启用 FTS5）：WAL、`synchronous=NORMAL`、外键开启。一个写连接在 store 线程，界面查询走只读连接。迁移版本号存 `PRAGMA user_version`，迁移前复制数据库文件。

| 表 | 主要字段 |
|---|---|
| `meetings` | id、title、started_at、ended_at、duration_ms、status（recording / finalized / recovered）、language、template_id |
| `tracks` | meeting_id、kind（mic / sys）、file、sample_rate、duration_ms、device_name |
| `segments` | meeting_id、track、start_ms、end_ms、text、raw_text、model、status（final / corrected / edited）、duplicate_of、revision |
| `segment_revisions` | segment_id、revision、text、source（asr / llm / user）、created_at |
| `summaries` | meeting_id、template_id、provider_id、content、created_at |
| `qa_messages` | meeting_id、role、content、citations（段落 id 列表） |
| `templates`、`glossary`、`llm_providers`、`settings` | 配置；API Key 不在库里，只存钥匙串条目名 |
| `jobs` | kind（retranscribe / summarize / correct）、meeting_id、state、progress、error，用于中断后恢复 |
| `segments_fts` | FTS5 `trigram` 分词，外部内容表指向 `segments` |

trigram 至少需要 3 个字符，1–2 个字的中文查询退回 `LIKE`（单用户数据量下足够快）。FTS 只索引当前可见的文本（不含串音段和旧修订）。

高精度重转会产生不同的段落边界，按段落记录的修订和问答引用会失效。重转在原有段落边界内解码；需要重新分段时保存为新的转录版本。

文件布局：`<数据目录>/meetings/<id>/mic.opus`、`sys.opus`。删除会议时删除整个目录和所有行。

## 12. 录音文件与回放

- 每轨一个 Ogg/Opus 文件：16 kHz 单声道，24 kbps VBR，20 ms 帧（libopus）。每小时每轨约 11 MB。
- 约每秒写出一个 Ogg 页并 flush，应用崩溃最多丢 1 秒音频。flush 防不了断电：后台线程每 10 秒 fsync 一次（macOS 用 `F_FULLFSYNC`），不在 capture 线程上做。
- 每批定稿写库时记录每轨"已转录到"的位置。启动时扫描 `status = recording` 的会议，读出 Ogg 实际时长，从该位置补转尾部，标记为 recovered。

Spike E 结果（macOS 15.7.7 系统 WKWebView，`scripts/dev/webview_audio_probe.sh`，20 s 测试音）：

| 格式 | canPlayType | `<audio>` 加载/跳转/播放 | decodeAudioData | MSE |
|---|---|---|---|---|
| Ogg/Opus | probably | 通过；duration 报 20.56 s（实际 20.007 s） | 通过 | 否 |
| WebM/Opus | probably | 通过 | 通过 | 是 |
| CAF/Opus | probably | 通过 | 通过 | 否 |
| MP4/Opus | 否 | 能播 | 失败 | 否 |
| AAC (m4a) | probably | 通过 | 通过 | 是 |

结论：录音用 Ogg/Opus（逐页校验，截断后仍可播放；WebView2 原生支持）。界面不用 `audio.duration`，用库里的时长和段落时间戳。WebKit 报的时长偏差 2.8%，如果跳转误差也按比例增长，3 小时的文件会差几分钟，M1 用 3 小时测试文件验证长文件跳转。macOS 14.2 的 WebKit 较旧，未验证；如果不支持，回退为自定义协议按 Range 现场解码 Opus 输出 WAV，文件格式不变。点击段落跳转的精度（目标 ±100 ms）在 M3 用定时提示音文件验证。

回放通过 Tauri 自定义协议提供文件，支持 Range，作用域限定在 `meetings/` 目录。

## 13. 其他模块

**模型管理**：清单记录 id、版本、文件、大小、sha256 和镜像地址（ModelScope 优先，其次 Hugging Face、GitHub）。下载断点续传，校验后原子改名，下载前检查磁盘空间。沿用 `scripts/dev/fetch_assets.py` 的逻辑，改用 Rust reqwest，跟随系统代理。

**LLM**：

- 支持三类接口：OpenAI 兼容（含国内各家和 LM Studio）、Anthropic Messages、Ollama。共用一个流式接口抽象。
- 请求只从 Rust 发出，WebView 拿不到 Key，也没有 CORS 问题。
- 总结按模板生成。转录超过上下文窗口的 70% 时分段总结再合并。
- 会后问答：转录放得进上下文就整段放入，否则用 FTS 召回相关段落。回答附段落引用，可以点击跳转。
- 转录纠错：按批发送定稿和术语表，返回结果按 diff 写成新的 revision，可以撤销。
- 会中要点：每 3 分钟或每 20 个新定稿生成一次，失败不影响录音。
- 第一次向某个远程服务发送转录前弹窗确认。任务写入 `jobs`，断网或退出后可以继续。

**IPC**：Tauri 命令加事件。实时事件在 Rust 侧合并，最多 10 Hz，内容是段落增量（新增或更新）和电平。界面用虚拟列表，每轨只保留当前预览。

**安全**：严格 CSP（不加载远程脚本，不允许内联脚本）；不启用 shell 插件；capabilities 只开放用到的命令；文件访问限定在数据目录；API Key 存系统钥匙串；日志不记录转录文本和 Key；非 HTTPS 的 LLM 地址只允许 localhost，否则需要用户确认。转录可能带有提示注入（会上有人念出指令），所以 LLM 输出的 Markdown 渲染前要清理，CSP 的 `img-src` 只允许 `'self' asset: data:`，防止注入的图片链接把转录内容带出去。LLM 请求关闭重定向（reqwest 跨主机重定向时会保留 `x-api-key` 这类自定义头）。IPC 命令只接收 id，不接收文件路径。

**测试与 CI**：

- core 单元测试使用假源和 WAV 源；性质测试检查定稿单调、不重叠、不丢音频。
- `echonote-bench` 门禁使用冻结的小型夹具，检查不变量、`decoded_audio_per_audio_s`、队列上限和错误率。
- 前端：vitest 测组件；Playwright 跑 Vite 开发服务器，IPC 用 mock。macOS 没有 WKWebView driver，不做 tauri-driver 端到端测试。
- GitHub Actions 跑 macos-14 和 windows-latest，缓存 sherpa 归档，执行 fmt、clippy、test 和 bench 冒烟。评测音频不能提交和再分发，CI 只用可以公开的夹具。新增测试要确认被发现并执行。

**打包**：

- macOS 每个架构出一个 DMG（不做 universal，避免二进制翻倍）。用 ad-hoc 签名，首次打开需要右键打开或执行 `xattr -dr com.apple.quarantine`。
- Windows 用 NSIS，静态 CRT（`.cargo/config.toml` 已设置 `+crt-static`），不需要 VC++ 运行库，带 WebView2 引导程序。没有签名，SmartScreen 会提示。
- 目前测得的二进制大小（bench，含 sherpa 和 onnxruntime）：arm64 19.3 MB，x64 22.6 MB。

## 14. M0 验证结果

| Spike | 结果 | 未完成 |
|---|---|---|
| D 静态链接 | macOS arm64、x64 通过，只依赖系统库；x64 在 Rosetta 下输出与 arm64 一致；Windows 目标通过类型检查 | Windows 实际链接（MT 静态库 + `+crt-static`）和二进制大小 |
| B 系统音频 | macOS 15.7.7 通过，见 5.2 | Windows loopback 实测；macOS 14.2 首次授权流程 |
| C 串音 | 真机结论见第 10 节；发现分段问题，加硬上限，见第 7 节 | 干净外放录音测 ERLE；AEC3 评估 |
| E 回放 | macOS 15.7.7 结果见第 12 节 | macOS 14.2、WebView2 |
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
| libopus | BSD-3-Clause | NOTICE 中列出 |
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
| 6 | 录音用 Ogg/Opus 16 kHz 24 kbps | 崩溃安全；两个平台的 WebView 都能播放 |
| 7 | 默认 VAD 用 Silero，不用 TEN | TEN 附加条款有竞争限制 |
| 8 | 串音去重以文本相似度为主 | 真机上能量相关不可靠；AEC 待 M1 验证 |
| 9 | S1 优先，S2 降为备选 | S2 突破内存和安装包预算；S1 是否达标待空闲时段评测 |
| 10 | 实时门禁以 CPU 核秒每音频秒为准 | 解码线程占用与 CPU 预算的换算关系由 M0 数据得出 |
| 11 | 分段器和调度器是 core 里同一个状态机，时钟由调用方传入 | bench 测的就是产品代码，CI 门禁可以确定性复现 |

## 17. 风险

| 风险 | 影响 | 应对 |
|---|---|---|
| ad-hoc 签名导致每次升级重新弹两个授权窗 | 升级体验差 | 自签名证书签名（M2 验证），文档说明 |
| macOS 14.2–14.3 上 Process Tap 行为未验证 | 最低版本可能要提高到 14.4 | 找 14.x 机器或虚拟机实测 |
| Windows 10 没有进程回环 | 回放时会录到自己的声音 | 录音期间禁用回放 |
| 内存预算口径未定；sherpa 不暴露 onnxruntime 内存池选项 | 500 MB 可能达不到 | 按 8 节拆分预算，需要用户确认；必要时自己编译 sherpa-onnx |
| ad-hoc 签名下钥匙串访问可能每次升级都弹窗 | API Key 读取体验差 | M2 和自签名证书一起验证 |
| App Nap 降低后台录音时 capture 轮询的频率 | 音频积压、延迟变大 | 录音期间 `beginActivity` 声明用户活动 |
| 暂停的语义（时间线是否继续） | 时间戳和回放错位 | M1 状态机定义：暂停期间不写音频，时间线记录暂停区间 |
| 选 S2 时安装包超过 300 MB | 超预算 | 流式模型首次使用时下载 |
| 合成集上麦克风轨错误率偏高 | 用户自己的话识别变差 | M1 前查清原因 |
| 用户真实会议录音还没有 | 评测集代表性不足 | 用户提供 3–5 段后补测 |

## 18. 架构评审

M0 草案经过一次架构评审（architect-reviewer）。采纳的修改已写入上文：第 7 节唯一分段器，第 9 节门禁口径和共用预览名额，第 8 节内存口径和积压上限，第 10 节时间对齐去重和 AEC 时间点，第 5、6 节时间戳与 Windows 静音流，第 11、12 节持久化和重转版本，第 13 节提示注入防护。
