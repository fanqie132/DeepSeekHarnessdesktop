# DeepSeek Harness Desktop 运行时镜像

本仓库是 **DeepSeek Harness 桌面壳** 的运行时镜像源，供桌面客户端自动更新使用。

## 它做什么

- 每 6 小时自动从 npm 官方源检查 `@deepseek-ai/dsh` 最新版本；
- 有新版时把运行环境打包成 `runtime.zip` 发布到本仓库的 `runtime` Release；
- 客户端通过 `runtime-version.txt`（几字节元数据）判断是否需要更新，再下载 `runtime.zip` 完成自动升级。

## 仓库结构

```
.github/workflows/sync-runtime.yml   # 自动同步工作流（每 6 小时）
runtime/                             # 运行环境依赖清单（@deepseek-ai/dsh）
```

## 说明

- 本仓库仅托管"运行时镜像"相关内容，不含桌面壳源码；
- 桌面壳源码与构建产物不在此仓库发布；
- `runtime.zip` 由 GitHub Actions 自动构建，客户端下载后即可运行，无需本地安装 Node.js。
