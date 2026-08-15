use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// CREATE_NO_WINDOW：让 node（控制台程序）在无窗口的后台运行，不弹出控制台窗口。
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 托管 dsh web 子进程生命周期，支持启停与重启。
pub struct DshManager {
    inner: Mutex<DshInner>,
}

struct DshInner {
    child: Option<Child>,
    node: PathBuf,
    entry: PathBuf,
    log: PathBuf,
}

impl DshManager {
    /// node: node 可执行文件路径；entry: dsh 的 bin.js 路径；log: 子进程日志文件。
    pub fn new(node: PathBuf, entry: PathBuf, log: PathBuf) -> Self {
        Self {
            inner: Mutex::new(DshInner {
                child: None,
                node,
                entry,
                log,
            }),
        }
    }

    pub fn start(&self) {
        let mut inner = self.inner.lock().unwrap();
        if inner.child.is_some() {
            return;
        }
        let child = spawn_dsh(&inner);
        inner.child = Some(child);
    }

    /// 停止当前 dsh 进程树（更新/替换 runtime 前调用，释放文件锁）。
    pub fn stop(&self) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(child) = inner.child.take() {
            kill_tree(child);
        }
    }

    /// 停止当前 dsh 进程树并重新启动（更新后调用）。
    pub fn restart(&self) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(child) = inner.child.take() {
            kill_tree(child);
        }
        let child = spawn_dsh(&inner);
        inner.child = Some(child);
    }
}

fn spawn_dsh(inner: &DshInner) -> Child {
    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&inner.log)
        .expect("failed to open dsh log file");
    let _ = writeln!(
        log_file,
        "[dsh-desktop] starting: node={:?} entry={:?}",
        inner.node,
        inner.entry
    );
    let stdout = log_file.try_clone().expect("failed to clone log handle");
    let mut cmd = Command::new(&inner.node);
    cmd.arg(&inner.entry)
        .arg("web")
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(log_file));
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.spawn().expect("failed to start dsh web")
}

/// 终止一个进程的整棵进程树（Windows taskkill /T /F）。
fn kill_tree(mut child: Child) {
    let _ = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .spawn()
        .and_then(|mut c| c.wait());
    let _ = child.kill();
}

impl Drop for DshManager {
    fn drop(&mut self) {
        if let Some(child) = self.inner.lock().unwrap().child.take() {
            kill_tree(child);
        }
    }
}
