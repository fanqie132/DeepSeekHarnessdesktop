use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

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
    Command::new(&inner.node)
        .arg(&inner.entry)
        .arg("web")
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(log_file))
        .spawn()
        .expect("failed to start dsh web")
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
