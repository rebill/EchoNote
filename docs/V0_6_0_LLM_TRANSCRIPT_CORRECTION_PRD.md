# EchoNote v0.6.0 LLM Transcript Correction PRD

## 1. 背景

ASR 识别后的 transcript 常见问题包括同音错字、专有名词误识别、英文产品名空格或大小写错误，以及标点断句不自然。v0.5.0 已提供用户维护的确定性纠错规则：

```text
wrong text => correct text
```

但规则无法覆盖所有一次性错字。v0.6.0 增加可选的 LLM transcript 纠错后处理，在最终 transcript 生成后进行一轮保守纠错。

## 2. 目标

- 在录音停止后的最终 transcript 阶段提供 LLM 保守纠错。
- 纠错作为实验性能力默认关闭，用户必须显式开启。
- 复用当前 Summary 的 LLM Provider 和模型配置。
- 按 transcript turn 批量纠错，并保留 turn id、speaker、时间戳和顺序。
- 失败时保留 ASR final transcript，不阻塞停止会议流程。
- 提供手动命令对当前会议笔记重新进行 LLM 纠错。
- 纠错前保存 transcript artifact，便于恢复和审计。
- 在会议 note metadata 区域记录纠错状态，不写入 `## Transcript`。

## 3. 非目标

- 不对实时 live transcript chunk 调用 LLM。
- 不自动触发 Summary 生成。
- 不新增独立 LLM Provider 或独立纠错模型设置。
- 不开放自定义纠错 prompt。
- 不提供纠错强度选项。
- 不做完整版本管理或正文 diff。
- 不允许 LLM 总结、补充内容、重排、合并或拆分发言。

## 4. 用户体验

### 4.1 设置

新增设置：

- `Enable LLM transcript correction`

默认关闭。开启说明需要明确：最终 transcript 会发送给当前 LLM Provider 做保守纠错。该能力只适合作为轻量检查，专有名词优先使用确定性的 `wrong => correct` 规则。

### 4.2 自动纠错

停止录音后流程：

1. 实时 transcript 已写入会议 note。
2. ASR `/transcript/finalize` 生成 speaker-aware final turns。
3. 写入 ASR final transcript。
4. 如果开启 LLM transcript correction，保存纠错前 transcript artifact。
5. 调用 LLM 进行 turn-level 保守纠错。
6. 成功后替换 `## Transcript`。
7. 在 metadata 区域写入纠错状态。

LLM 纠错失败时：

- 保留 ASR final transcript。
- 提示 `LLM transcript correction failed. Final ASR transcript was kept.`
- 状态面板展示失败详情。

### 4.3 手动纠错

新增命令：

- `EchoNote: Correct Transcript with LLM`

行为：

- 读取当前会议 note 的 `## Transcript`。
- 解析时间戳、speaker 和 text。
- 保存纠错前 artifact。
- 调用 LLM 纠错。
- 替换 `## Transcript`。
- 更新 metadata。

如果当前 transcript 已纠错过，仍允许再次执行；每次执行前都保存新的 before artifact。

## 5. 纠错规则

纠错顺序：

1. 用户维护的 `wrong => correct` 规则。
2. LLM 保守纠错。
3. 用户维护的 `wrong => correct` 规则再兜底一次。

LLM 允许：

- 修明显错别字和同音错字。
- 修专有名词大小写和中英文空格。
- 补常见标点和轻微断句。

LLM 不允许：

- 改写语义。
- 补充未出现在 transcript 中的信息。
- 总结或提炼。
- 合并、拆分、删除或重排 turns。
- 修改 speaker、时间戳或 turn id。

## 6. 隐私

EchoNote 的 ASR 默认本地运行，但 LLM Provider 可能是云端。LLM transcript correction 默认关闭。用户开启后，最终 transcript 会发送给当前 LLM Provider。

## 7. Artifact

纠错前 transcript 保存到会议 note 同目录的 `.echonote-artifacts/` 子目录，例如：

```text
Meetings/.echonote-artifacts/2026-06-04 10-30 Meeting.transcript.before-llm.20260604-143000.md
```

该 artifact 不依赖 `Save raw audio`。

## 8. 验收标准

- 默认设置下不会自动调用 LLM 纠错。
- 开启后，ASR final transcript 写入成功后会自动尝试 LLM 纠错。
- LLM 配置缺失时明确提示，不静默跳过。
- 纠错失败时保留 ASR final transcript。
- 手动命令可对当前 note 的 `## Transcript` 重新纠错。
- 纠错前 artifact 被保存到 `.echonote-artifacts/`。
- 会议 metadata 能看到 LLM 纠错状态。
- LLM 输出结构异常或删改幅度过大时，不合格 turn 保留原文。
