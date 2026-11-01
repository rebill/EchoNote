# 旧产品移除提案

状态：**提案，等待确认。** 本文档只描述计划，不删除任何文件。PRD 第 9 节规定旧目录在 v1 发布后移除，移除前单独确认。

## 范围

| 目录或文件 | 内容 | 本分支里的文件数 |
|---|---|---|
| `plugin/` | Obsidian 插件（`echonote` 0.9.0） | 74 |
| `companion/` | Tauri 1 代 ASR 运行时管理应用 | 91 |
| `asr-service/` | Python 本地 ASR 服务 | 19 |
| `vscode-extension/` | 只存在于主工作区 `codex/vscode-extension` 分支的未提交改动，本分支没有 | 0 |
| `versions.json` | Obsidian 插件版本对应的最低 Obsidian 版本 | 1 |
| `scripts/build_offline_bundle.py`、`scripts/tests/test_build_offline_bundle.py` | v0.9.0 离线安装包 | 2 |
| `scripts/v0_2_0_fake_backend_smoke.mjs`、`scripts/v0_4_0_real_diarization_smoke.py` | 旧版冒烟脚本 | 2 |
| `.github/workflows/ci.yml` | 只有 Plugin 和 ASR service 两个任务 | 1 |
| `.gitignore` 里 `main.js`、`asr-service/.venv/`、`companion/src-tauri/gen/`、`offline-bundle/` | 旧产品的构建产物 | 4 行 |
| `docs/` 里非 `V1_0_0_*`、`v1-*` 的文档和 `docs/contracts/` | 旧版 PRD、设计、任务、发布、用户文档和 Companion 发现协议 | 29 个文件和 1 个目录 |

保留：`CHANGELOG.md` 的历史条目、`License`、`CONTRIBUTING.md`（按 v1 改写）、所有 git 标签。

## 建议做法

1. **时机**：v1.0.0 正式发布、DMG 和 NSIS 在干净的 macOS 14.2+ 和 Windows 10/11 上装过之后。v1 出问题时，旧产品还能当退路。
2. **存档**：在删除前的最后一个提交上打标签 `legacy-final`。现有标签只到 `v0.8.1`，`v0.9.0` 的离线安装改动没有标签，这个标签能保住它。推送标签需要你确认。
3. **文档**：旧文档移到 `docs/legacy/`，不删除。它们记录了旧版 ASR 分段、说话人分离和 LLM 纠错的设计取舍，以后查原因时有用。
4. **删除**：在 `chore/remove-legacy` 分支上一个提交完成：删除上表的代码目录、脚本、`versions.json` 和 `ci.yml`，清理 `.gitignore`，更新 `CONTRIBUTING.md` 和 `docs/RELEASE.md` 里旧产品的步骤。
5. **检查**：`git grep -n "plugin/\|companion/\|asr-service/"` 只剩 `docs/legacy/` 和 `CHANGELOG.md`；Desktop 工作流全部通过。
6. **回退**：还原这一个提交即可。

## 需要你决定

- **`codex/vscode-extension` 分支**：它比 v1 分支多一个提交（`ec21525`，Windows CPU 离线 ASR 服务），主工作区里还有它未提交的改动：修改 `asr-service/` 的 4 个文件、`docs/API_CONTRACT.md` 和 `scripts/build_windows_cpu_zip.py`，新增 `asr-service/echonote_asr/batch_*.py`、对应测试和 `vscode-extension/`。删除 `asr-service/` 会和它冲突。三种选择：先把它合并或放弃，再删除；把 `asr-service/` 留到它有结论后再删；或者把它拆成独立仓库。
- **Obsidian 插件用户**：如果插件在 Obsidian 社区插件列表里，要在插件 README 和最后一个版本的说明里写明停止维护，并指向 v1 桌面应用。
- **旧文档**：移到 `docs/legacy/` 还是直接删除（标签里都还在）。
