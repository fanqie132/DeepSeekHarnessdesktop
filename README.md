# DeepSeek Harness 桌面客户端（非官方）

> 一个用 [Tauri 2](https://tauri.app) 打造的 DeepSeek Harness（`dsh`）Windows 桌面客户端套壳。加载官方 Web UI，提供原生窗口、系统托盘、自动更新等体验。

## 重要声明

- **非官方项目**，与 DeepSeek 官方无关，不隶属于 DeepSeek。
- 应用内 UI 加载的是官方 DeepSeek Harness Web 版（`http://127.0.0.1:3080`），页面与功能由 DeepSeek 官方维护，会随官方版本自动更新。
- 鲸鱼图标为 DeepSeek 官方品牌标志，版权归 DeepSeek 所有。
- 本客户端以 MIT 协议开源，仅供学习与个人使用。

## 功能

- 原生 Windows 窗口（WebView2 内核），加载官方 Web UI，界面与原版一致
- 系统托盘：关闭窗口最小化到托盘，"退出"才真正退出并清理后台进程
- 自动更新：启动时检测 `@deepseek-ai/dsh` 最新版，发现新版弹窗提示"重启更新"
- 自包含：捆绑 Node.js 运行时，dsh 依赖首次启动自动下载，安装后不依赖系统环境

## 下载安装

到 [Releases](https://github.com/fanqie132/DeepSeekHarnessdesktop/releases) 下载最新的 `DeepSeek Harness_x64-setup.exe`，双击安装即可。

> 安装包约 25MB（不含 dsh 运行时）。**首次启动**会自动下载运行时（约 76MB，随 DeepSeek 版本更新），需要联网，完成后即可正常使用。

## 从源码构建

### 环境要求

| 依赖 | 说明 |
|---|---|
| [Node.js](https://nodejs.org) | ≥ 20（开发环境） |
| [pnpm](https://pnpm.io) | ≥ 10（Node.js 内置 corepack：`corepack enable`） |
| [Rust](https://www.rust-lang.org) | stable（MSVC 工具链） |
| [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) | C++ 桌面开发负载（Rust 链接器） |

### 步骤

```powershell
# 1. 安装开发期 runtime 依赖（已配置 hoisted 结构，规避 Windows 长路径问题）
cd runtime
pnpm install

# 2. 下载 Node.js 运行时（v24 或更高，官方 https://nodejs.org）
#    将 node.exe 放到 src-tauri/resources/node/node.exe

# 3. 构建安装包
cd ..
pnpm tauri build
```

构建产物：`src-tauri/target/release/bundle/nsis/DeepSeek Harness_0.1.0_x64-setup.exe`

> 说明：
> - `runtime/node_modules` 与捆绑的 `node.exe` 体积大且可重新生成，故不纳入 Git 仓库。
> - 发布版安装包**不包含** runtime；首次启动时从 GitHub Release 下载 `runtime.zip` 并解压到安装目录（见 `src-tauri/src/runtime.rs`）。发布新版时请用 `tar -a -cf runtime.zip -C .. runtime` 重新打包并覆盖上传 Release 的 `runtime` tag。

## 技术栈

| 层 | 技术 |
|---|---|
| 壳 | Rust + Tauri 2 |
| 内嵌浏览器 | WebView2（Windows 10/11 自带） |
| 内容 | `@deepseek-ai/dsh`（DeepSeek 官方 npm 包） |
| 更新 | registry 版本检测 + pnpm 更新 runtime + 自动重启 |

## 目录结构

```
dsh-desktop/
├── src-tauri/          # Rust 壳
│   ├── src/            # 主逻辑（进程托管、托盘、更新器）
│   ├── resources/      # 捆绑资源（node.exe 构建时放入）
│   └── icons/          # 图标（鲸鱼 logo 归 DeepSeek 品牌）
├── src/                # 壳的前端（启动 loading 页）
├── runtime/            # dsh 运行时（pnpm 管理的依赖，不入库）
└── index.html
```

## License

[MIT](LICENSE)
