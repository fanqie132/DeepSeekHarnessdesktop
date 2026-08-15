use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use tauri::{AppHandle, Manager};

/// runtime 压缩包的下载地址（GitHub Release 固定资产，发布者每次覆盖更新）。
const RUNTIME_URL: &str =
    "https://github.com/fanqie132/dsh-desktop/releases/download/runtime/runtime.zip";

/// runtime 根目录：开发期用项目内 runtime，发布期用打包资源目录。
pub fn runtime_dir(app: &AppHandle) -> PathBuf {
    if cfg!(debug_assertions) {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri 应有父目录")
            .join("runtime")
    } else {
        crate::strip_verbatim(app.path().resource_dir().expect("无法定位资源目录")).join("runtime")
    }
}

/// dsh 的 bin.js 入口路径。
pub fn runtime_entry(app: &AppHandle) -> PathBuf {
    runtime_dir(app)
        .join("node_modules")
        .join("@deepseek-ai")
        .join("dsh")
        .join("lib")
        .join("bin.js")
}

/// runtime 是否已就绪（存在可运行的 dsh）。
pub fn is_ready(app: &AppHandle) -> bool {
    runtime_entry(app).exists()
}

/// 下载最新 runtime.zip 并原子替换 runtime 目录（首次安装与更新共用）。
/// 调用前应确保没有进程占用 runtime 内的文件。
pub fn fetch_and_replace_runtime(app: &AppHandle) -> Result<(), String> {
    let rt = runtime_dir(app);
    let parent = rt.parent().expect("runtime 应有父目录").to_path_buf();
    let zip_path = parent.join("dsh-runtime-download.zip");
    download_file(RUNTIME_URL, &zip_path)?;

    // staging 与 runtime 同盘，避免跨磁盘 rename 失败
    let staging_root = parent.join("dsh-runtime-staging");
    if staging_root.exists() {
        fs::remove_dir_all(&staging_root).map_err(|e| format!("清理临时目录失败：{e}"))?;
    }
    fs::create_dir_all(&staging_root).map_err(|e| format!("创建临时目录失败：{e}"))?;
    extract_zip(&zip_path, &staging_root)?;

    // zip 内含 runtime/ 前缀目录
    let staged_runtime = staging_root.join("runtime");
    if !staged_runtime
        .join("node_modules")
        .join("@deepseek-ai")
        .join("dsh")
        .join("lib")
        .join("bin.js")
        .exists()
    {
        return Err("下载内容缺少 dsh 运行时".into());
    }

    // 原子替换：旧目录改名备份，新目录移入，失败回滚
    let backup = parent.join("runtime-old");
    if backup.exists() {
        fs::remove_dir_all(&backup).map_err(|e| format!("清理备份失败：{e}"))?;
    }
    if rt.exists() {
        fs::rename(&rt, &backup).map_err(|e| format!("备份旧运行时失败：{e}"))?;
    }
    if let Err(e) = fs::rename(&staged_runtime, &rt) {
        if backup.exists() {
            let _ = fs::rename(&backup, &rt);
        }
        return Err(format!("替换运行时失败：{e}"));
    }
    if backup.exists() {
        let _ = fs::remove_dir_all(&backup);
    }
    let _ = fs::remove_file(&zip_path);
    let _ = fs::remove_dir_all(&staging_root);
    Ok(())
}

/// 用系统自带 curl.exe 下载（自动读取 HTTPS_PROXY 等系统代理环境变量）。
/// 依赖 Windows 10 1803+ 内置的 curl。
fn download_file(url: &str, dest: &Path) -> Result<(), String> {
    let status = Command::new("curl")
        .args(["-s", "-L", "--fail", "-o"])
        .arg(dest)
        .arg(url)
        .status()
        .map_err(|e| format!("调用 curl 失败：{e}"))?;
    if !status.success() {
        return Err(format!("下载运行时失败（curl 退出码 {:?}）", status.code()));
    }
    Ok(())
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let file = File::open(zip_path).map_err(|e| format!("打开压缩包失败：{e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("解析压缩包失败：{e}"))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("读取压缩项失败：{e}"))?;
        let outpath = entry
            .enclosed_name()
            .ok_or_else(|| "压缩包内存在非法路径".to_string())?;
        let full = dest.join(outpath);
        if entry.is_dir() {
            fs::create_dir_all(&full).map_err(|e| format!("创建目录失败：{e}"))?;
        } else {
            if let Some(p) = full.parent() {
                fs::create_dir_all(p).map_err(|e| format!("创建目录失败：{e}"))?;
            }
            let mut out = File::create(&full).map_err(|e| format!("创建文件失败：{e}"))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压文件失败：{e}"))?;
        }
    }
    Ok(())
}
