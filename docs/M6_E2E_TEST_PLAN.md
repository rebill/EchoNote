# M6 端到端集成测试计划

## 1. 目标

验证 EchoNote MVP 的真实完整链路：

```text
音频输入
  -> Web Audio 录音与分段
  -> 本地 ASR 服务
  -> mlx-community/Qwen3-ASR-0.6B-4bit 真实转录
  -> Transcript 写入 Obsidian 会议笔记
  -> Stop Meeting
  -> AI Summary 写回会议笔记
```

## 2. 前置条件

- Obsidian 测试 Vault 已安装最新版 EchoNote 插件。
- `plugin/main.js` 已重新拷贝到测试 Vault。
- EchoNote 设置页已配置：
  - `Python path`
  - `ASR service path`
  - `ASR service port`
  - `Audio input device`
  - LLM Provider 配置
- `asr-service/.venv` 已安装 `mlx-audio`。
- `/tmp/echonote-test.wav` 的 M3 Spike 已跑通。

## 3. 启动真实 ASR 服务

```bash
cd /Users/br/Git/github/rebill/EchoNote
source asr-service/.venv/bin/activate

python -m echonote_asr \
  --host 127.0.0.1 \
  --port 8765 \
  --model mlx-community/Qwen3-ASR-0.6B-4bit \
  --backend mlx-audio \
  --log-level info
```

另开终端确认：

```bash
curl http://127.0.0.1:8765/health
```

加载模型：

```bash
curl -X POST http://127.0.0.1:8765/model/load \
  -H 'Content-Type: application/json' \
  -d '{"model_id":"mlx-community/Qwen3-ASR-0.6B-4bit"}'
```

确认状态：

```bash
curl http://127.0.0.1:8765/model/status
```

预期 `status` 为 `ready`。

## 4. 测试场景 A：真实 ASR 会议记录

步骤：

1. 打开 Obsidian 测试 Vault。
2. 打开 EchoNote Status Panel。
3. 确认：
   - `ASR Service: running`
   - `Model: ready`
   - `Selected Model: mlx-community/Qwen3-ASR-0.6B-4bit`
   - `Audio Input` 为预期输入设备
4. 点击 `Start Meeting`。
5. 对所选输入源播放或讲话 3-5 分钟。
6. 点击 `Stop Meeting`。

验收：

- 自动创建 `Meetings/... Meeting.md`。
- Transcript 中出现真实转录文本。
- Transcript 不出现 `fake transcript`。
- Transcript 不出现 `STTOutput(...)`。
- `Chunk Queue` 最终回到 `0 pending`。
- `Recording` 最终回到 `idle`。

## 5. 测试场景 B：暂停/继续

步骤：

1. 点击 `Start Meeting`。
2. 录制 20 秒。
3. 点击 `Pause Recording`。
4. 等待 20 秒。
5. 点击 `Resume Recording`。
6. 再录制 20 秒。
7. 点击 `Stop Meeting`。

验收：

- 暂停期间不产生新的 Transcript。
- 继续后 Transcript 继续追加。
- Stop 后队列归零。

## 6. 测试场景 C：保存完整会议音频

步骤：

1. EchoNote 设置中开启 `Save raw audio`。
2. 录制 1-2 分钟。
3. 点击 `Stop Meeting`。
4. 到配置的音频目录检查文件。

验收：

- 每场会议只生成一个完整 WAV 文件。
- 文件路径类似：

```text
Meetings/audio/{{meeting_title}}/{{meeting_title}}.wav
```

- 回放 WAV 有声音。
- 不生成大量 chunk WAV 文件。

## 7. 测试场景 D：BlackHole / Multi-Output Device

步骤：

1. 系统 Output 选择 `EchoNote Output Mix` 或 `Multi-Output Device`。
2. EchoNote Audio Input 选择 `BlackHole 2ch`。
3. 播放系统声音或会议软件声音。
4. 开始 EchoNote 会议记录。
5. 停止后回放保存的 WAV。

验收：

- 保存的 WAV 包含系统/会议软件输出。
- 如果使用真实 ASR，Transcript 包含系统/会议软件声音对应文本。

## 8. 测试场景 E：AI Summary

步骤：

1. 打开刚生成的会议笔记。
2. 点击 `Summarize Meeting`。

验收：

- `Summary`、`Decisions`、`Action Items`、`Key Points`、`Open Questions` 被更新。
- `Transcript` 完整保留。
- LLM 配置缺失时显示明确错误，不写坏笔记。

## 9. 稳定性测试

建议执行一次 10-30 分钟测试。

观察：

- Obsidian 是否卡顿。
- ASR 服务是否持续运行。
- `Chunk Queue` 是否长期积压。
- Transcript 是否持续写入。
- Stop 后是否能完成队列清理和音频保存。

## 10. M6 通过标准

全部满足：

- 真实 ASR backend 可端到端工作。
- 会议笔记可自动创建。
- Transcript 为真实转录。
- Stop Meeting 后无尾段对象污染。
- 暂停/继续可用。
- 可保存单个完整 WAV。
- AI Summary 可写回会议笔记且不覆盖 Transcript。

