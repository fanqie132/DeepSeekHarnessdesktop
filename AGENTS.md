# dsh-desktop 项目工作约定

DeepSeek Harness 桌面壳（Tauri 2 + Rust + TS）。当前工作以根目录《执行方案与进度.md》为准：开工前先读它，完成任务后更新对应条目状态并在文末进度日志追加一行。

## Git 守卫（用户强制要求：时刻可回档）

- 修改源码、配置或发布流程前，先执行 `git status --short`。
- 工作区干净时，先创建 `checkpoint: before <task>` 本地提交，再开始较大改动。
- 工作区不干净时，先展示变更，禁止覆盖、重置或丢弃用户改动。
- **每个任务（A1/B1/C2…）完成并验证后，立即单独 commit**，格式 `feat:`/`fix:`/`docs:`/`chore:`/`checkpoint:`，保证任意时刻可回档。
- 修改后展示 `git diff --stat` 与验证结果；未经用户明确要求，不推送 GitHub、不强推、不改写历史、不删除 remote。
- 发布安装包不提交进 Git；使用 Git tag 和 GitHub Release 交付。
- 远端 origin 为 github.com/fanqie132/DeepSeekHarnessdesktop，仅同步用，不在此仓库做破坏性操作。
