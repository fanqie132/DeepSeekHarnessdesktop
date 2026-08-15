# DeepSeek Harness Desktop Client (Unofficial)

> A Windows desktop wrapper for [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) (`dsh`), built with [Tauri 2](https://tauri.app). Loads the official Web UI with native window, system tray and auto-update experience.

## Important Notice

- **Unofficial project.** Not affiliated with or endorsed by DeepSeek.
- The in-app UI loads the official DeepSeek Harness web version at `http://127.0.0.1:3080`; pages and features are maintained by DeepSeek and update automatically with official releases.
- The whale logo is DeepSeek's official brand mark, owned by DeepSeek.
- This client is open-sourced under the MIT license for learning and personal use.

## Features

- Native Windows window (WebView2 engine) loading the official Web UI, identical to the original interface
- System tray: closing the window minimizes to tray; "Exit" fully quits and cleans up background processes
- Auto-update: checks for the latest `@deepseek-ai/dsh` on startup and prompts "Restart to update" when a new version is found
- Self-contained: bundles the Node.js runtime and dsh dependencies, no system environment required after install

## Download

Download the latest `DeepSeek Harness_x64-setup.exe` from the [Releases](https://github.com/fanqie132/DeepSeekHarnessdesktop/releases) page and run the installer.

> The installer is about 25 MB (without the dsh runtime). On **first launch** it automatically downloads the runtime (~76 MB, updated with DeepSeek releases) and requires internet connectivity.

## Build from Source

### Prerequisites

| Dependency | Notes |
|---|---|
| [Node.js](https://nodejs.org) | >= 20 (dev environment) |
| [pnpm](https://pnpm.io) | >= 10 (enable via corepack: `corepack enable`) |
| [Rust](https://www.rust-lang.org) | stable (MSVC toolchain) |
| [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) | C++ desktop workload (Rust linker) |

### Steps

```powershell
# 1. Install dev runtime dependencies (hoisted layout configured to avoid Windows long-path issues)
cd runtime
pnpm install

# 2. Download the Node.js runtime (v24 or newer from https://nodejs.org)
#    Place node.exe at src-tauri/resources/node/node.exe

# 3. Build the installer
cd ..
pnpm tauri build
```

Artifact: `src-tauri/target/release/bundle/nsis/DeepSeek Harness_0.1.0_x64-setup.exe`

> Note:
> - `runtime/node_modules` and the bundled `node.exe` are large but regenerable, so they are not committed to the Git repository.
> - The release installer does **not** bundle the runtime; on first launch it downloads `runtime.zip` from the GitHub Release and extracts it to the install directory (see `src-tauri/src/runtime.rs`). When publishing a new version, rebuild with `tar -a -cf runtime.zip -C .. runtime` and overwrite the `runtime` tag asset on Releases.

## Tech Stack

| Layer | Technology |
|---|---|
| Shell | Rust + Tauri 2 |
| Embedded browser | WebView2 (built into Windows 10/11) |
| Content | `@deepseek-ai/dsh` (official DeepSeek npm package) |
| Updates | registry version check + pnpm update runtime + auto restart |

## Directory Layout

```
dsh-desktop/
├── src-tauri/          # Rust shell
│   ├── src/            # core logic (process management, tray, updater)
│   ├── resources/      # bundled assets (node.exe placed at build time)
│   └── icons/          # icons (whale logo owned by DeepSeek)
├── src/                # shell frontend (startup loading page)
├── runtime/            # dsh runtime (pnpm-managed deps, not committed)
└── index.html
```

## License

[MIT](LICENSE)
