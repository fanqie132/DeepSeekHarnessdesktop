use std::collections::{HashMap, HashSet};
use std::net::TcpStream;
use std::time::{Duration, Instant};

const DSH_HOST: &str = "127.0.0.1";
const DSH_PORT: u16 = 3080;

/// 后台线程：轮询 dsh 会话。
/// 1) 会话 running 从 true→false（每轮回复结束）→ 播放“完成”提示音（同会话 30s 节流）。
/// 2) goal phase active→complete → 播放“完成”提示音。
pub fn spawn() {
    std::thread::spawn(|| {
        // 等待 dsh 服务就绪
        let ready_deadline = Instant::now() + Duration::from_secs(120);
        loop {
            if TcpStream::connect((DSH_HOST, DSH_PORT)).is_ok() {
                break;
            }
            if Instant::now() > ready_deadline {
                return;
            }
            std::thread::sleep(Duration::from_millis(1000));
        }

        let mut running_by_session: HashMap<String, bool> = HashMap::new();
        let mut last_complete_at: HashMap<String, Instant> = HashMap::new();
        let mut phase_by_session: HashMap<String, String> = HashMap::new();
        let mut completed_goal: HashSet<String> = HashSet::new();

        loop {
            if let Ok(sessions) = fetch_sessions() {
                for (id, running, goal_phase) in sessions {
                    // 每轮回复结束：running true→false
                    let prev_running = running_by_session.insert(id.clone(), running);
                    if !running && prev_running == Some(true) {
                        let now = Instant::now();
                        let throttled = last_complete_at
                            .get(&id)
                            .map(|t| now.duration_since(*t) < Duration::from_secs(30))
                            .unwrap_or(false);
                        if !throttled {
                            last_complete_at.insert(id.clone(), now);
                            play_complete();
                        }
                    }
                    // goal 完成：phase active→complete
                    let prev_phase = phase_by_session.insert(id.clone(), goal_phase.clone());
                    if goal_phase == "complete"
                        && prev_phase.as_deref() == Some("active")
                        && !completed_goal.contains(&id)
                    {
                        completed_goal.insert(id);
                        play_complete();
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

/// 从 dsh 拉取会话列表，返回 (sessionId, running, goal.phase)。
fn fetch_sessions() -> Result<Vec<(String, bool, String)>, String> {
    let body = ureq::post(&format!("http://{DSH_HOST}:{DSH_PORT}/api/session.list"))
        .set("Content-Type", "application/json")
        .set("Host", format!("{DSH_HOST}:{DSH_PORT}").as_str())
        .send_string(&format!(
            r#"{{"type":"client-request","rpcId":"n{}","method":"session.list","payload":{{}}}}"#,
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
        ))
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;

    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let items = json["result"]["value"]["items"].as_array().cloned().unwrap_or_default();
    let mut out = Vec::new();
    for item in items {
        let id = item["sessionId"].as_str().unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        let running = item["running"].as_bool().unwrap_or(false);
        let phase = item["projections"]["values"]["goal"]["phase"]
            .as_str()
            .unwrap_or("")
            .to_string();
        out.push((id, running, phase));
    }
    Ok(out)
}

/// 播放“任务完成”系统提示音（Windows MessageBeep，两次）。
fn play_complete() {
    #[cfg(windows)]
    unsafe {
        // 直接链接 user32 的 MessageBeep（windows-sys 未包含该函数）
        #[link(name = "user32")]
        extern "system" {
            fn MessageBeep(u_type: u32) -> i32;
        }
        MessageBeep(0x00000040); // MB_ICONASTERISK
        std::thread::sleep(Duration::from_millis(120));
        MessageBeep(0x00000040);
    }
}
