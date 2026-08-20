use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use tauri::{AppHandle, Emitter, Manager};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// runtime 压缩包的下载地址（GitHub Release 固定资产，发布者每次覆盖更新）。
const RUNTIME_URL: &str =
    "https://github.com/fanqie132/dsh-desktop/releases/download/runtime/runtime.zip";

/// runtime 根目录：开发期用项目内 runtime，发布期用可写的 AppData 目录（避免 Program Files 权限与占用问题）。
pub fn runtime_dir(app: &AppHandle) -> PathBuf {
    if cfg!(debug_assertions) {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri 应有父目录")
            .join("runtime")
    } else {
        // 优先使用 LocalAppData（可写、随用户），失败回退到 resource_dir
        if let Ok(p) = app.path().app_local_data_dir() {
            return crate::strip_verbatim(p).join("runtime");
        }
        crate::strip_verbatim(app.path().resource_dir().expect("无法定位资源目录")).join("runtime")
    }
}

/// 旧版 runtime 位置（安装目录下），用于迁移
fn legacy_runtime_dir(app: &AppHandle) -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        return None;
    }
    app.path()
        .resource_dir()
        .ok()
        .map(|p| crate::strip_verbatim(p).join("runtime"))
}

/// 若新位置无 runtime 但旧位置有，自动迁移（原子 rename，同盘优先）
fn try_migrate_legacy(app: &AppHandle) {
    let new_rt = runtime_dir(app);
    if new_rt.exists() {
        return;
    }
    if let Some(old) = legacy_runtime_dir(app) {
        if old.exists() && old != new_rt {
            if let Some(parent) = new_rt.parent() {
                let _ = fs::create_dir_all(parent);
            }
            // 同盘 rename 最快，跨盘则回退为复制
            if fs::rename(&old, &new_rt).is_err() {
                let _ = copy_dir_all(&old, &new_rt);
                let _ = fs::remove_dir_all(&old);
            }
        }
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            fs::copy(entry.path(), dst_path)?;
        }
    }
    Ok(())
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

/// runtime 是否已就绪（存在可运行的 dsh），含旧版迁移
pub fn is_ready(app: &AppHandle) -> bool {
    try_migrate_legacy(app);
    runtime_entry(app).exists()
}

/// 下载最新 runtime.zip 并原子替换 runtime 目录（首次安装与更新共用）。
/// 调用前应确保没有进程占用 runtime 内的文件。
pub fn fetch_and_replace_runtime(app: &AppHandle) -> Result<(), String> {
    try_migrate_legacy(app);
    let _ = app.emit(
        "runtime-progress",
        serde_json::json!({"stage": "download", "message": "正在下载运行时（约 76MB）..."}),
    );

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
    extract_zip(&zip_path, &staging_root, app)?;

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
/// 依赖 Windows 10 1803+ 内置的 curl；失败时尝试 ureq 回退并输出详细错误。
fn download_file(url: &str, dest: &Path) -> Result<(), String> {
    // 优先 curl（走系统代理），失败则回退到 ureq
    let mut cmd = Command::new("curl");
    cmd.args(["-s", "-L", "--fail", "-o"]).arg(dest).arg(url);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    match cmd.output() {
        Ok(out) if out.status.success() => {
            if dest.exists() && fs::metadata(dest).map(|m| m.len() > 1024).unwrap_or(false) {
                return Ok(());
            }
            // curl 成功但文件异常，继续回退
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            // 不直接返回，尝试 ureq 回退
            let _ = fs::write(
                std::env::temp_dir().join("dsh-runtime-download-curl.log"),
                format!("curl fail code {:?} stdout:{stdout} stderr:{stderr}", out.status.code()),
            );
        }
        Err(e) => {
            let _ = fs::write(
                std::env::temp_dir().join("dsh-runtime-download-curl.log"),
                format!("curl spawn fail: {e}"),
            );
        }
    }
    // ureq 回退（不依赖外部 curl）
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .map_err(|e| format!("下载失败（curl 与 ureq 均失败，最后 ureq: {e}）"))?;
    let mut file = File::create(dest).map_err(|e| format!("创建下载文件失败：{e}"))?;
    let mut reader = resp.into_reader();
    std::io::copy(&mut reader, &mut file).map_err(|e| format!("写入下载文件失败：{e}"))?;
    Ok(())
}

fn extract_zip(zip_path: &Path, dest: &Path, app: &AppHandle) -> Result<(), String> {
    let file = File::open(zip_path).map_err(|e| format!("打开压缩包失败：{e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("解析压缩包失败：{e}"))?;
    let total = archive.len();
    let _ = app.emit(
        "runtime-progress",
        serde_json::json!({"stage": "extract", "current": 0, "total": total}),
    );
    for i in 0..total {
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
        // 每 200 个文件或最后一个时上报进度，避免事件过于频繁
        if i % 200 == 199 || i + 1 == total {
            let _ = app.emit(
                "runtime-progress",
                serde_json::json!({"stage": "extract", "current": i + 1, "total": total}),
            );
        }
    }
    Ok(())
}
