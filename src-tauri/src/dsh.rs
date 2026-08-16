use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// CREATE_NO_WINDOW：让 node（控制台程序）在无窗口的后台运行，不弹出控制台窗口。
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Windows Job Object：套壳进程无论以何种方式退出，Job 关闭时自动终止其内所有子进程，
/// 彻底杜绝 dsh 服务残留孤儿进程（比 taskkill 更可靠）。
#[cfg(windows)]
struct KillOnCloseJob(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl KillOnCloseJob {
    fn new() -> Option<Self> {
        use windows_sys::Win32::Foundation::*;
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            Some(Self(job))
        }
    }

    fn assign_pid(&self, pid: u32) {
        use windows_sys::Win32::Foundation::*;
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};
        unsafe {
            let handle = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if handle != INVALID_HANDLE_VALUE {
                AssignProcessToJobObject(self.0, handle);
                CloseHandle(handle);
            }
        }
    }
}

#[cfg(windows)]
impl Drop for KillOnCloseJob {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        unsafe { CloseHandle(self.0) };
    }
}

/// Job 句柄在线程间传递是安全的（关闭时机由 Drop 保证）。
#[cfg(windows)]
unsafe impl Send for KillOnCloseJob {}
#[cfg(windows)]
unsafe impl Sync for KillOnCloseJob {}

/// 托管 dsh web 子进程生命周期，支持启停与重启。
pub struct DshManager {
    inner: Mutex<DshInner>,
}

struct DshInner {
    child: Option<Child>,
    forwarder: Option<Child>,
    node: PathBuf,
    entry: PathBuf,
    log: PathBuf,
    #[cfg(windows)]
    job: Option<KillOnCloseJob>,
}

impl DshManager {
    /// node: node 可执行文件路径；entry: dsh 的 bin.js 路径；log: 子进程日志文件。
    pub fn new(node: PathBuf, entry: PathBuf, log: PathBuf) -> Self {
        Self {
            inner: Mutex::new(DshInner {
                child: None,
                forwarder: None,
                node,
                entry,
                log,
                #[cfg(windows)]
                job: KillOnCloseJob::new(),
            }),
        }
    }

    pub fn start(&self) {
        let mut inner = self.inner.lock().unwrap();
        if inner.child.is_some() {
            return;
        }
        let child = spawn_dsh(&inner);
        let forwarder = spawn_forwarder(&inner);
        inner.child = Some(child);
        inner.forwarder = forwarder;
    }

    /// 停止当前 dsh 进程树（更新/替换 runtime 前调用，释放文件锁）。
    pub fn stop(&self) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(child) = inner.child.take() {
            kill_tree(child);
        }
        if let Some(forwarder) = inner.forwarder.take() {
            kill_tree(forwarder);
        }
    }

    /// 让独立 taskkill 进程后台清理 dsh 进程树，不等待（用于托盘退出，界面立即关闭）。
    pub fn stop_detached(&self) {
        let mut inner = self.inner.lock().unwrap();
        let mut spawn_kill = |pid: u32| {
            let mut cmd = Command::new("taskkill");
            cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
            #[cfg(windows)]
            cmd.creation_flags(CREATE_NO_WINDOW);
            let _ = cmd.spawn();
        };
        if let Some(child) = inner.child.take() {
            spawn_kill(child.id());
        }
        if let Some(forwarder) = inner.forwarder.take() {
            spawn_kill(forwarder.id());
        }
    }

    /// 停止当前 dsh 进程树并重新启动（更新/插件生效后调用）。
    /// 完全结束旧进程（含子进程），等待端口释放后再启动新进程。
    pub fn restart(&self) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(child) = inner.child.take() {
            kill_tree(child);
            wait_port_free(3080, Duration::from_secs(10));
        }
        if let Some(forwarder) = inner.forwarder.take() {
            kill_tree(forwarder);
        }
        let child = spawn_dsh(&inner);
        let forwarder = spawn_forwarder(&inner);
        inner.child = Some(child);
        inner.forwarder = forwarder;
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
        "[dsh-desktop] starting: node={:?} entry={:?} GITHUB_TOKEN={}",
        inner.node,
        inner.entry,
        registry_has_github_token()
    );
    let stdout = log_file.try_clone().expect("failed to clone log handle");
    let mut cmd = Command::new(&inner.node);
    // dsh 保持默认 127.0.0.1 绑定（dsh 出于安全拒绝 0.0.0.0）；局域网访问由 forwarder 转发
    cmd.arg(&inner.entry).arg("web");
    cmd.stdout(Stdio::from(stdout))
        .stderr(Stdio::from(log_file));
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    #[cfg(windows)]
    merge_user_env(&mut cmd);
    let child = cmd.spawn().expect("failed to start dsh web");
    #[cfg(windows)]
    if let Some(job) = &inner.job {
        job.assign_pid(child.id());
    }
    child
}

/// 把当前用户的注册表环境变量（HKCU\Environment）合并进子进程环境。
/// 解决 `setx` 只写注册表、不改变已运行进程的问题：这样修改环境变量后，
/// 无需注销/重启 Explorer，重启 dsh 服务器即生效。
/// 注意：跳过 REG_EXPAND_SZ 类型（值含 %VAR% 未展开，如 TEMP/TMP），
/// 避免把字面 `%USERPROFILE%` 注入导致 node 无法创建临时目录。
#[cfg(windows)]
fn merge_user_env(cmd: &mut Command) {
    use winreg::enums::{HKEY_CURRENT_USER, RegType};
    use winreg::RegKey;
    if let Ok(env_key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Environment") {
        for item in env_key.enum_values() {
            if let Ok((name, value)) = item {
                if value.vtype == RegType::REG_EXPAND_SZ {
                    continue; // 不展开 %VAR%，交给父进程已有的展开值
                }
                let s = value.to_string();
                if !s.is_empty() {
                    cmd.env(name, s);
                }
            }
        }
    }
}

/// 注册表里是否有 GITHUB_TOKEN（用于日志确认注入依据）。
#[cfg(windows)]
fn registry_has_github_token() -> &'static str {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    match RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Environment")
        .and_then(|k| k.get_value::<String, _>("GITHUB_TOKEN"))
    {
        Ok(_) => "found-in-registry",
        Err(_) => "not-found",
    }
}

/// 终止一个进程的整棵进程树（Windows taskkill /T /F）。
fn kill_tree(mut child: Child) {
    let mut cmd = Command::new("taskkill");
    cmd.args(["/PID", &child.id().to_string(), "/T", "/F"]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let _ = cmd.spawn().and_then(|mut c| c.wait());
    let _ = child.kill();
}

/// 轮询等待端口可绑定（旧进程退出后端口释放），避免新进程绑定冲突。
fn wait_port_free(port: u16, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let ok = std::net::TcpListener::bind(("127.0.0.1", port)).is_ok();
        if ok {
            return; // 绑定成功即端口已释放（测试用的 listener 随作用域结束自动释放）
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

impl Drop for DshManager {
    fn drop(&mut self) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(child) = inner.child.take() {
            kill_tree(child);
        }
        if let Some(forwarder) = inner.forwarder.take() {
            kill_tree(forwarder);
        }
    }
}

/// 内嵌的局域网转发器脚本（监听 8787，Host 重写后转发到 127.0.0.1:3080）。
const FORWARDER_JS: &str = include_str!("../forwarder.cjs");

/// 启动局域网转发器：让同一 WiFi 下的手机通过 http://<局域网IP>:8787 访问 dsh。
fn spawn_forwarder(inner: &DshInner) -> Option<Child> {
    let path = std::env::temp_dir().join("dsh-forwarder.cjs");
    if std::fs::write(&path, FORWARDER_JS).is_err() {
        return None;
    }
    let mut cmd = Command::new(&inner.node);
    cmd.arg(&path);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    #[cfg(windows)]
    merge_user_env(&mut cmd);
    let child = cmd.spawn().ok()?;
    #[cfg(windows)]
    if let Some(job) = &inner.job {
        job.assign_pid(child.id());
    }
    Some(child)
}
