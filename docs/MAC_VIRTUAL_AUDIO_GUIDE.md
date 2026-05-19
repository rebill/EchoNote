# macOS 虚拟音频设备配置指南

## 1. 目标

EchoNote 默认只能录制所选音频输入设备。如果只选择 Mac 内置麦克风，EchoNote 只能录到自己的声音，不能直接录到 Zoom、飞书会议、腾讯会议、Google Meet 等会议软件的输出声音。

MVP 采用虚拟音频设备方案：

```text
麦克风 + 会议软件输出
  -> 虚拟混音输入设备
  -> EchoNote 选择该输入设备
  -> 本地 ASR 转录
```

EchoNote 不直接捕获系统输出音频，不使用 ScreenCaptureKit，也不提供 native helper。

## 2. 推荐工具

可选工具：

- BlackHole 2ch：免费，适合基础混音。
- Loopback：商业软件，配置体验更好，适合长期使用。

MVP 文档以 BlackHole / Loopback 作为推荐路径，不绑定具体工具。

## 3. EchoNote 插件要求

EchoNote 设置页需要提供：

- 刷新音频输入设备列表。
- 音频输入设备下拉选择。
- 当前选择设备名称显示。

录音时使用所选输入设备：

```ts
navigator.mediaDevices.getUserMedia({
  audio: {
    deviceId: selectedDeviceId === "default" ? undefined : { exact: selectedDeviceId },
    echoCancellation: false,
    noiseSuppression: false,
    autoGainControl: false
  }
});
```

关闭回声消除、降噪和自动增益，是为了避免浏览器音频处理破坏虚拟混音信号。

## 4. BlackHole + Multi-Output Device 配置手册

这一节适用于最常见需求：

```text
自己能听到会议声音
EchoNote 能录到会议软件输出
```

这种配置通常只能让 EchoNote 录到会议软件输出，不一定包含你的麦克风。要同时录制麦克风和会议软件输出，请看后面的 Loopback 方案，或再配置聚合/混音输入设备。

### 4.1 安装 BlackHole

先安装 BlackHole 2ch。

安装完成后，建议重启一次 Obsidian。如果系统没有立即识别 BlackHole，可以重启 Mac 或重启 CoreAudio。

### 4.2 打开 Audio MIDI Setup

Multi-Output Device 的入口不在 `System Settings → Sound`，而是在 macOS 自带工具 `Audio MIDI Setup` 里。

打开方式一：

1. 按 `Command + Space` 打开 Spotlight。
2. 搜索：

```text
Audio MIDI Setup
```

中文系统可能显示为：

```text
音频 MIDI 设置
```

打开方式二：

```text
Finder → Applications → Utilities → Audio MIDI Setup.app
```

中文系统路径通常是：

```text
访达 → 应用程序 → 实用工具 → 音频 MIDI 设置
```

如果打开后没有看到设备列表，在菜单栏选择：

```text
Window → Show Audio Devices
```

中文可能是：

```text
窗口 → 显示音频设备
```

### 4.3 创建 Multi-Output Device

在 `Audio MIDI Setup` 左下角找到 `+` 按钮。

点击 `+`，选择：

```text
Create Multi-Output Device
```

中文可能是：

```text
创建多输出设备
```

然后在右侧设备列表中勾选两个输出：

```text
你的耳机 / MacBook Speakers
BlackHole 2ch
```

建议：

- 如果你使用耳机，就勾选耳机和 BlackHole。
- 如果你使用 Mac 外放，就勾选 MacBook Speakers 和 BlackHole。
- 勾选 `Drift Correction` 时，通常给非主设备勾选即可。如果不确定，可以先保持默认。

你可以把新设备重命名为：

```text
EchoNote Output Mix
```

### 4.4 设置系统输出

打开：

```text
System Settings → Sound → Output
```

选择刚创建的：

```text
EchoNote Output Mix
```

或系统默认名称：

```text
Multi-Output Device
```

此时系统声音会同时输出到：

- 你的耳机/扬声器。
- BlackHole 2ch。

### 4.5 设置 EchoNote 输入

打开 Obsidian：

```text
Settings → Community plugins → EchoNote options
```

在 `Audio` 区：

1. 点击 `Refresh audio input devices`。
2. 在 `Audio input device` 下拉框选择：

```text
BlackHole 2ch
```

然后开始会议记录。

### 4.6 验证是否成功

推荐先用 EchoNote fake ASR 验证链路：

1. 系统 Output 选择 `EchoNote Output Mix`。
2. EchoNote Input 选择 `BlackHole 2ch`。
3. 播放一段系统声音，比如视频或音乐。
4. EchoNote 开始录制 20 秒。
5. 如果打开了 `Save raw audio`，停止后回放保存的 WAV。

如果保存的 WAV 有系统声音，说明 BlackHole + Multi-Output Device 配置成功。

也可以用 QuickTime 验证：

1. 打开 `QuickTime Player`。
2. 选择：

```text
File → New Audio Recording
```

3. 点录音按钮旁边的小箭头。
4. Microphone 选择：

```text
BlackHole 2ch
```

5. 播放系统声音并录制几秒。
6. 回放 QuickTime 录音。

如果 QuickTime 能录到，EchoNote 也应该能录到。

### 4.7 常见误区

不要把系统 Output 直接切到 `BlackHole 2ch` 后就开始测试。

这样声音只会进入 BlackHole，你的耳机/扬声器可能听不到声音。更推荐：

```text
System Output: Multi-Output Device / EchoNote Output Mix
EchoNote Input: BlackHole 2ch
```

如果 EchoNote 选择 `BlackHole 2ch` 但录音没有声音，通常说明系统声音没有被送进 BlackHole。请回到 `Audio MIDI Setup` 检查 Multi-Output Device 是否同时勾选了 BlackHole 和你的实际播放设备。

如果 EchoNote 下拉框里看不到 `BlackHole 2ch`：

- 确认 BlackHole 已安装。
- 重启 Obsidian。
- 点击 `Refresh audio input devices`。
- 确认 macOS 已允许 Obsidian 使用麦克风。

## 5. Loopback 配置思路

典型配置：

1. 创建一个 Loopback virtual device，例如 `EchoNote Meeting Mix`。
2. 添加音频源：
   - 当前麦克风。
   - 会议软件或系统输出。
3. 打开监听，确保自己仍能听到会议声音。
4. 在 EchoNote 设置页选择 `EchoNote Meeting Mix`。

Loopback 的优势是可以按应用选择声音来源，配置比 BlackHole 更直观。

## 6. 验收方式

1. 打开 EchoNote 设置页。
2. 点击刷新音频输入设备。
3. 选择虚拟混音输入设备。
4. 开始会议记录。
5. 让会议软件播放声音，同时对麦克风说话。
6. Transcript 中应持续出现 fake ASR 分段。
7. 切换到真实 ASR backend 后，应能转录混合后的会议内容。

MVP 的 fake ASR 只能证明输入设备链路、分段和写入流程工作正常；不能判断音频内容是否真的包含会议软件输出。真实 ASR 或保存完整音频后回放，才能验证混音内容。

## 7. 常见问题

### EchoNote 看不到 BlackHole 或 Loopback

- 确认工具已安装。
- 重启 Obsidian。
- 在 EchoNote 设置页刷新音频输入设备。
- macOS 隐私设置中允许 Obsidian 使用麦克风。

### 只能录到麦克风，录不到会议软件声音

- 检查会议软件输出是否路由到了虚拟设备。
- 检查虚拟设备是否包含系统输出源。
- 如果使用 BlackHole，确认已配置 Multi-Output Device 或混音链路。

### 会议声音能录到，但自己听不到声音

- 需要配置监听输出。
- BlackHole 通常需要 Multi-Output Device。
- Loopback 中需要打开 Monitor 或配置输出设备。
