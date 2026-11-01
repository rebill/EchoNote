# EchoNote v1 安装说明

EchoNote v1 的安装包没有经过 Apple 公证，也没有 Windows 代码签名证书。macOS 包用 ad-hoc 签名，Windows 包不签名。第一次打开时系统会拦截，按下面的步骤放行。只放行从本项目发布页下载、并且校验过 SHA-256 的文件。

## 系统要求

- macOS 14.2 或更新版本，Apple 芯片选 `aarch64` DMG，Intel 芯片选 `x64` DMG。
- Windows 10 或 11，64 位。
- 4 核 CPU、8 GB 内存可以实时转写。不需要显卡。
- 默认模型和语音检测模型都在安装包里，安装和录音不需要联网。

## 校验下载的文件

发布页附带 `SHA256SUMS.txt`。在下载目录里执行：

```sh
# macOS
shasum -a 256 EchoNote_*.dmg
```

```powershell
# Windows PowerShell
Get-FileHash .\EchoNote_*-setup.exe -Algorithm SHA256
```

输出的值要和 `SHA256SUMS.txt` 里同名文件的值完全一致。不一致就删掉文件，不要打开。

## macOS

1. 打开 DMG，把 EchoNote 拖到“应用程序”。
2. 第一次打开时，系统会提示“无法验证 EchoNote 是否包含恶意软件”。点“完成”或“取消”。
3. 放行：
   - macOS 15 及更新版本：打开“系统设置 → 隐私与安全性”，滚到“安全性”一节，点 EchoNote 旁边的“仍要打开”，再在弹窗里点“仍要打开”并输入密码。macOS 15 去掉了按住 Control 点按打开的办法。
   - macOS 14：在“应用程序”里按住 Control 点按 EchoNote，选“打开”，再点“打开”。也可以用上面的“仍要打开”。
4. 只需要放行一次。以后更新替换了应用，可能需要再放行一次。

也可以在终端里去掉下载标记，效果相同：

```sh
xattr -dr com.apple.quarantine /Applications/EchoNote.app
```

### 权限

- 第一次点“开始录音”时，系统会请求麦克风权限和系统音频录制权限。麦克风记为“我”，系统声音记为“对方”。
- 拒绝系统音频录制只会影响“对方”：录音继续，只录麦克风。可以之后在“系统设置 → 隐私与安全性”里打开。
- 只有录音时才会使用这两项权限。

### 钥匙串提示

配置了大模型服务后，API Key 存在登录钥匙串里。ad-hoc 签名的应用每次更新后签名都会变，macOS 会再次询问 EchoNote 能否读取这个钥匙串项目。输入登录密码后点“始终允许”，这一版就不会再问。想要更新后也不再询问，可以用自签名证书自己构建，见下文。

### 更新和卸载

- 更新：用新版本替换“应用程序”里的 EchoNote。资料库、录音和模型都不在应用里，不受影响。
- 卸载：删除 `/Applications/EchoNote.app`。要连数据一起删，退出应用后再删除 `~/Library/Application Support/com.rebill.echonote`，并在“钥匙串访问”里删除名称为 `com.rebill.echonote` 的项目。

## Windows

1. 运行 `EchoNote_<版本>_x64-setup.exe`。安装到当前用户目录，不需要管理员权限。
2. SmartScreen 会提示“Windows 已保护你的电脑”。确认文件校验通过后，点“更多信息”，再点“仍要运行”。
3. EchoNote 依赖 Microsoft Edge WebView2 运行时。Windows 11 和已更新的 Windows 10 已自带。没有的话，安装程序会从微软下载安装，这一步需要联网；离线的电脑请先装好 WebView2 运行时（微软提供离线安装包）。

### 权限

- 麦克风：如果“设置 → 隐私和安全性 → 麦克风”里关闭了桌面应用的麦克风访问，EchoNote 会提示没有权限。打开后重新开始录音。
- 系统声音不需要额外权限。Windows 11 录除 EchoNote 自己以外所有程序的声音；Windows 10 录默认播放设备的声音，会议软件要从默认播放设备输出才能录到。

### 更新和卸载

- 更新：直接运行新版本的安装程序。
- 卸载：在“设置 → 应用”里卸载 EchoNote。要连数据一起删，再删除 `%APPDATA%\com.rebill.echonote`，并在“凭据管理器 → Windows 凭据”里删除名称包含 `com.rebill.echonote` 的项目。

## 自签名构建

自签名证书不能让其他电脑信任这个应用：Gatekeeper 和 SmartScreen 仍然会提示。它的用处是让签名在每次构建之间保持不变，这样 macOS 钥匙串在更新后不再询问。构建步骤见 `README.md`。

### macOS

1. 打开“钥匙串访问”，菜单“钥匙串访问 → 证书助理 → 创建证书”。名称填 `EchoNote Local`，身份类型选“自签名根证书”，证书类型选“代码签名”。
2. 确认证书可用：

   ```sh
   security find-identity -v -p codesigning
   ```

3. 写一个只在本机使用的配置文件 `app/src-tauri/tauri.sign.conf.json`（已在 `.gitignore` 里，不会提交），用这个证书代替 ad-hoc 签名：

   ```json
   { "bundle": { "macOS": { "signingIdentity": "EchoNote Local" } } }
   ```

   然后构建：

   ```sh
   cd app
   pnpm bundle --config src-tauri/tauri.sign.conf.json --bundles app,dmg
   ```

4. 第一次用新证书签名的版本，钥匙串还会询问一次，点“始终允许”。之后用同一个证书构建的版本不再询问。

### Windows

在 PowerShell 里创建证书并记下指纹：

```powershell
$cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=EchoNote Local" -CertStoreLocation Cert:\CurrentUser\My
$cert.Thumbprint
```

把指纹写进一个只在本机使用的配置文件，例如 `app/src-tauri/tauri.sign.conf.json`（已在 `.gitignore` 里，不会提交）：

```json
{ "bundle": { "windows": { "certificateThumbprint": "<指纹>", "digestAlgorithm": "sha256", "timestampUrl": "http://timestamp.digicert.com" } } }
```

然后构建：

```powershell
cd app
pnpm bundle --config src-tauri/tauri.sign.conf.json --bundles nsis
```

只有把这个证书导入“受信任的根证书颁发机构”和“受信任的发布者”的电脑才会认这个签名。SmartScreen 依据下载信誉判断，自签名不会消除它的提示。
