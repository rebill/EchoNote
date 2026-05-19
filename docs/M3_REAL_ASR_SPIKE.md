# M3 真实 ASR Spike 记录

## 1. 目标

验证 `mlx-community/Qwen3-ASR-0.6B-4bit` 是否能在目标 Mac 上完成单段音频转录，并确认以下信息：

- 安装依赖。
- 模型加载方式。
- 输入音频格式。
- 首次加载时间。
- 单段转录耗时。
- 输出文本格式。
- 主要错误和解决方式。

## 2. 结论摘要

当前实现采用 `mlx-audio` 作为首选真实 ASR backend。

理由：

- Hugging Face 模型卡为 `mlx-community/Qwen3-ASR-0.6B-4bit` 提供了 `mlx-audio` 加载示例。
- 该路径可以直接使用 PRD 中指定的 MLX 社区 4bit 模型 ID。
- M2 的假 ASR 服务保留为默认 backend，避免真实模型依赖影响插件端联调。

备选路径：

- `mlx-qwen3-asr` 社区库。该库更偏专用封装，但默认示例指向 `Qwen/Qwen3-ASR-0.6B`，不作为 MVP 首选。

## 3. 已实现内容

- 新增真实 ASR adapter：[asr-service/echonote_asr/transcriber.py](../asr-service/echonote_asr/transcriber.py)
- 新增真实 ASR Spike CLI：[asr-service/echonote_asr/spike_real_asr.py](../asr-service/echonote_asr/spike_real_asr.py)
- ASR 服务新增 backend 参数：
  - `fake`
  - `mlx-audio`
- `fake` 仍为默认 backend。
- `pyproject.toml` 新增可选依赖组：

```bash
pip install -e 'asr-service[mlx]'
```

## 4. 音频输入要求

Spike 输入音频必须是：

- WAV
- mono
- 16kHz
- PCM 16-bit

如果原始音频不是 16kHz，可用：

```bash
ffmpeg -y \
  -i /Users/br/.claude/skills/hyper-video/reference_audio.wav \
  -ac 1 -ar 16000 -sample_fmt s16 \
  /tmp/echonote-test.wav
```

## 5. 运行步骤

在仓库根目录执行：

```bash
cd /Users/br/Git/github/rebill/EchoNote
source asr-service/.venv/bin/activate
pip install -e 'asr-service[mlx]'
```

运行单文件 Spike：

```bash
python -m echonote_asr.spike_real_asr \
  --audio /tmp/echonote-test.wav \
  --model mlx-community/Qwen3-ASR-0.6B-4bit \
  --language zh
```

预期输出：

```json
{
  "python": "3.x.x",
  "platform": "macOS-...",
  "model_id": "mlx-community/Qwen3-ASR-0.6B-4bit",
  "audio_path": "/tmp/echonote-test.wav",
  "audio_bytes": 123456,
  "language": "zh",
  "load_seconds": 0.0,
  "transcribe_seconds": 0.0,
  "text": "..."
}
```

## 6. HTTP 服务真实 backend 验证

启动服务：

```bash
python -m echonote_asr \
  --host 127.0.0.1 \
  --port 8765 \
  --model mlx-community/Qwen3-ASR-0.6B-4bit \
  --backend mlx-audio \
  --log-level info
```

加载模型：

```bash
curl -X POST http://127.0.0.1:8765/model/load \
  -H 'Content-Type: application/json' \
  -d '{"model_id":"mlx-community/Qwen3-ASR-0.6B-4bit"}'
```

转录：

```bash
curl -X POST http://127.0.0.1:8765/transcribe \
  -F audio=@/tmp/echonote-test.wav \
  -F chunk_id=chunk-001 \
  -F started_at_ms=0 \
  -F ended_at_ms=15000 \
  -F language=zh
```

## 7. 本机运行结果

运行时间：2026-05-19

命令：

```bash
python -m echonote_asr.spike_real_asr \
  --audio /tmp/echonote-test.wav \
  --model mlx-community/Qwen3-ASR-0.6B-4bit \
  --language zh
```

结果：

```json
{
  "ok": true,
  "python": "3.14.3",
  "platform": "macOS-15.7.4-arm64-arm-64bit-Mach-O",
  "model_id": "mlx-community/Qwen3-ASR-0.6B-4bit",
  "audio_path": "/private/tmp/echonote-test.wav",
  "audio_bytes": 196786,
  "language": "zh",
  "load_seconds": 29.443,
  "transcribe_seconds": 1.281,
  "text": "夜深了，翻开书页，让温柔的文字像丝绒般轻抚你的心绪。"
}
```

记录：

- Python 版本：3.14.3
- 平台：macOS 15.7.4 arm64
- 模型 ID：`mlx-community/Qwen3-ASR-0.6B-4bit`
- 输入音频格式：WAV / mono / 16kHz / PCM 16-bit
- 音频大小：196,786 bytes
- 首次模型下载：9 files，约 713 MB，约 26 秒
- 首次加载时间：29.443 秒
- 单段转录耗时：1.281 秒
- 输出文本：`夜深了，翻开书页，让温柔的文字像丝绒般轻抚你的心绪。`
- 主要错误：未发生

## 8. M3 验收结论

M3 真实 ASR Spike 已通过：

- `mlx-community/Qwen3-ASR-0.6B-4bit` 可以在目标 Mac 上完成单段音频转录。
- 16kHz mono PCM16 WAV 输入格式可用。
- `MlxAudioTranscriber.load(model_id)` 可加载模型。
- `MlxAudioTranscriber.transcribe_wav(wav_path)` 可生成非空文本。
- 首次下载和加载耗时可接受，但需要在后续状态面板中明确展示模型加载中状态。
- 单段转录耗时约 1.3 秒，支持 MVP 的准实时分段转录假设。
