# EchoNote v1 发布流程

旧产品（插件、Companion、ASR 服务）的发布见 `docs/RELEASE.md`。

## 1. 准备

- 版本号一致：`app/package.json`、根 `Cargo.toml` 的 `workspace.package.version`。`tauri.conf.json` 读 `app/package.json`。
- `CHANGELOG.md` 写好这个版本的条目。
- 模型清单 `crates/echonote-models/manifest.json` 改过时，重新跑 `scripts/dev/verify_release_digest.py` 核对上游摘要。
- `third_party/notices.json` 跟上依赖变化：升级 sherpa-onnx、onnxruntime、libopus 或前端运行时后，更新版本号和许可证文件。`gen_notices.py` 遇到没有许可证文件、也不在 `crate_licenses` 里的 crate 会报错。

## 2. 本机构建（可选）

```sh
python3 scripts/dev/fetch_sherpa_onnx.py .sherpa-onnx aarch64-apple-darwin
export SHERPA_ONNX_ARCHIVE_DIR=$PWD/.sherpa-onnx
python3 scripts/dev/stage_bundle_models.py
python3 scripts/dev/gen_notices.py
cd app && pnpm install --frozen-lockfile && pnpm bundle --target aarch64-apple-darwin --bundles app,dmg && cd ..
python3 scripts/dev/check_bundle.py target/aarch64-apple-darwin/release aarch64-apple-darwin
```

`check_bundle.py` 检查模型、`THIRD_PARTY_NOTICES.txt`、`LICENSE.txt` 是否在应用里，安装包是否在 `docs/v1-performance-budgets.json` 的 `app.installerMiB`（300 MiB）以内。

## 3. 打标签

```sh
git tag v1.0.0
git push origin v1.0.0
```

`.github/workflows/release.yml` 先跑 Desktop 检查，再构建 macOS arm64、macOS x64 和 Windows x64 安装包，最后建一个草稿 Release，附 `SHA256SUMS.txt`。带 `-` 的版本号（如 `v1.0.0-beta.1`）标为预发布。手动运行工作流只构建和上传构件，不建 Release。

## 4. 发布前检查

- 在干净的 macOS 14.2+ 和 Windows 10/11 上断网安装、录音、回放、导出。
- 安装说明里的 Gatekeeper 和 SmartScreen 步骤与实际一致。
- **模型再分发**：默认模型 SenseVoiceSmall 用 FunASR Model Open Source License 1.1，条款写明“仅供参考和学习”，要求注明来源和作者（`NOTICE` 和 `THIRD_PARTY_NOTICES.txt` 已写）。公开发布安装包前，确认这个用途符合许可证；需要商用时要换默认模型（技术设计第 15 节）。
- 确认后在 GitHub 上把草稿 Release 改为发布。
