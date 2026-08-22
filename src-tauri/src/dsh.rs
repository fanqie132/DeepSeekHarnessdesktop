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
    cert_dir: PathBuf,
    forward_port: u16,
    #[cfg(windows)]
    job: Option<KillOnCloseJob>,
}

impl DshManager {
    /// node: node 可执行文件路径；entry: dsh 的 bin.js 路径；log: 子进程日志文件；cert_dir: 转发器证书目录。
    pub fn new(node: PathBuf, entry: PathBuf, log: PathBuf, cert_dir: PathBuf) -> Self {
        Self {
            inner: Mutex::new(DshInner {
                child: None,
                forwarder: None,
                node,
                entry,
                log,
                cert_dir,
                forward_port: 8787,
                #[cfg(windows)]
                job: KillOnCloseJob::new(),
            }),
        }
    }

    /// 启动 dsh 本体（转发器由 set_forwarder 单独控制，默认关闭）。
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
        if let Some(forwarder) = inner.forwarder.take() {
            kill_tree(forwarder);
        }
    }

    /// 开关局域网转发器（B3）：开启则拉起（含鉴权 token），关闭则终止进程。
    /// 返回开启成功后的连接信息（地址 + token），关闭返回 None。
    pub fn set_forwarder(&self, enabled: bool) -> Option<(String, String)> {
        let mut inner = self.inner.lock().unwrap();
        if !enabled {
            if let Some(f) = inner.forwarder.take() {
                kill_tree(f);
            }
            return None;
        }
        if inner.forwarder.is_some() {
            return None; // 已在运行
        }
        match spawn_forwarder(&inner) {
            Some(child) => {
                inner.forwarder = Some(child);
                let ip = lan_ipv4()?.to_string();
                let token = ensure_forwarder_token(&inner.cert_dir)?;
                Some((format!("https://{ip}:{}", inner.forward_port), token))
            }
            None => None,
        }
    }

    /// 转发器当前是否在运行。
    pub fn forwarder_running(&self) -> bool {
        self.inner.lock().unwrap().forwarder.is_some()
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
    /// 注意：转发器不在此自动重启，由托盘开关状态决定。
    pub fn restart(&self) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(child) = inner.child.take() {
            kill_tree(child);
            wait_port_free(3080, Duration::from_secs(10));
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
        "[dsh-desktop] starting: node={:?} entry={:?} GITHUB_TOKEN={}",
        inner.node,
        inner.entry,
        registry_has_github_token()
    );
    let stdout = log_file.try_clone().expect("failed to clone log handle");
    let mut cmd = Command::new(&inner.node);
    // dsh 保持默认 127.0.0.1 绑定（dsh 出于安全拒绝 0.0.0.0）；局域网访问由 forwarder 转发。
    // --no-open：阻止 dsh 自动用系统默认浏览器打开 UI（界面由本壳的 WebView 加载）
    cmd.arg(&inner.entry).arg("web").arg("--no-open");
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

/// 检测当前局域网 IPv4 地址（私网段，排除 loopback 与 Tailscale 的 100.x）。
fn lan_ipv4() -> Option<std::net::Ipv4Addr> {
    let ifaces = if_addrs::get_if_addrs().ok()?;
    for iface in ifaces {
        if let std::net::IpAddr::V4(ip) = iface.ip() {
            let o = ip.octets();
            let is_private = o[0] == 10
                || (o[0] == 172 && (16..=31).contains(&o[1]))
                || (o[0] == 192 && o[1] == 168);
            let is_tailscale = o[0] == 100 && (64..=127).contains(&o[1]);
            if is_private && !is_tailscale {
                return Some(ip);
            }
        }
    }
    None
}

/// 确保局域网转发器证书存在（SAN 含当前局域网 IP），返回 (pfx 路径, 密码)。
/// 证书不存在**或本机 IP 已变化**时重新生成（B4：旧实现只查文件存在，换 IP 后手机端报证书错误）。
pub fn ensure_forwarder_cert(cert_dir: &std::path::Path) -> Option<(PathBuf, String)> {
    use std::fs;
    let pass = "dsh-fwd".to_string();
    let pfx = cert_dir.join("forwarder.pfx");
    let ip_file = cert_dir.join("forwarder.ip.txt");
    let current_ip = lan_ipv4()?.to_string();
    // 复用条件：文件存在 且 记录的生成 IP 与当前一致
    if pfx.exists() {
        let recorded = fs::read_to_string(&ip_file).unwrap_or_default();
        if recorded.trim() == current_ip {
            return Some((pfx, pass));
        }
        // IP 变化：删旧 pfx + 清理证书库里的旧自签证书，走重签
        let _ = fs::remove_file(&pfx);
        let _ = fs::remove_file(&ip_file);
        let _ = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-ChildItem Cert:\\CurrentUser\\My | Where-Object { $_.FriendlyName -eq 'Dsh Forwarder' } | Remove-Item -Force",
            ])
            .creation_flags(if cfg!(windows) { 0x0800_0000 } else { 0 })
            .output();
    }
    if fs::create_dir_all(cert_dir).is_err() {
        return None;
    }
    // 用 PowerShell 生成自签名证书（SAN: 当前局域网 IP + 127.0.0.1 + localhost），导出 pfx
    let ps = format!(
        "New-SelfSignedCertificate -Subject 'CN={ip}' -TextExtension @('2.5.29.17={{text}}ipaddress={ip}&ipaddress=127.0.0.1&dns=localhost') -CertStoreLocation Cert:\\CurrentUser\\My -FriendlyName 'Dsh Forwarder' -NotAfter (Get-Date).AddYears(5) | Export-PfxCertificate -FilePath '{pfx}' -Password (ConvertTo-SecureString '{pass}' -AsPlainText -Force) -Force",
        ip = current_ip, pfx = pfx.display(), pass = pass
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .creation_flags(if cfg!(windows) { 0x0800_0000 } else { 0 })
        .output();
    match out {
        Ok(o) if o.status.success() && pfx.exists() => {
            // 记录生成时的 IP，供下次变化检测
            let _ = fs::write(&ip_file, &current_ip);
            Some((pfx, pass))
        }
        _ => None,
    }
}

/// 确保转发器访问 token 存在（B1 鉴权）：首次随机生成 32 位 hex 并持久化，
/// 之后复用。无 token 的设备一律拒绝。
fn ensure_forwarder_token(cert_dir: &std::path::Path) -> Option<String> {
    use std::fs;
    let f = cert_dir.join("forwarder-token.txt");
    if let Ok(t) = fs::read_to_string(&f) {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return Some(t);
        }
    }
    use rand::Rng;
    const CHARSET: &[u8] = b"0123456789abcdef";
    let token: String = (0..32)
        .map(|_| CHARSET[rand::thread_rng().gen_range(0..CHARSET.len())] as char)
        .collect();
    fs::create_dir_all(cert_dir).ok()?;
    fs::write(&f, &token).ok()?;
    Some(token)
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

/// 启动局域网转发器：让同一 WiFi 下的手机通过 https://<局域网IP>:8787 访问 dsh。
/// B3：证书不可用时拒绝启动（绝不回退 HTTP 明文暴露局域网）；
/// B1：必须携带访问 token，无 token 的设备一律 403。
fn spawn_forwarder(inner: &DshInner) -> Option<Child> {
    let path = std::env::temp_dir().join("dsh-forwarder.cjs");
    if std::fs::write(&path, FORWARDER_JS).is_err() {
        return None;
    }
    // HTTPS 证书是硬性要求（安全上下文 + 不做明文回退）
    let (pfx, pass) = ensure_forwarder_cert(&inner.cert_dir)?;
    // 访问 token 是硬性要求
    let token = ensure_forwarder_token(&inner.cert_dir)?;
    let mut cmd = Command::new(&inner.node);
    cmd.arg(&path);
    // 转发器日志写进 dsh.log，便于排查
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(&inner.log) {
        cmd.stdout(Stdio::from(f.try_clone().expect("log clone")))
            .stderr(Stdio::from(f));
    }
    cmd.env("FORWARD_PFX", &pfx);
    cmd.env("FORWARD_PFX_PASS", pass);
    cmd.env("FORWARD_TOKEN", token);
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
