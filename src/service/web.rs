use super::data::*;
use anyhow::{bail, Context, Result};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs::File;
use std::process::Command;
use std::sync::Arc;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

#[derive(Debug, Default)]
pub struct ClashStatus {
    pub info: Option<StartBody>,
    pub process_id: Option<u32>,
}

impl ClashStatus {
    pub fn global() -> &'static Arc<Mutex<ClashStatus>> {
        static CLASHSTATUS: OnceCell<Arc<Mutex<ClashStatus>>> = OnceCell::new();

        CLASHSTATUS.get_or_init(|| Arc::new(Mutex::new(ClashStatus::default())))
    }
}

/// GET /version
/// 获取服务进程的版本
pub fn get_version() -> Result<HashMap<String, String>> {
    let version = env!("CARGO_PKG_VERSION");

    let mut map = HashMap::new();

    map.insert("service".into(), "Clash Verge Service".into());
    map.insert("version".into(), version.into());

    Ok(map)
}

/// POST /start_clash
/// 启动clash进程
pub fn start_clash(body: StartBody) -> Result<()> {
    let body_cloned = body.clone();

    let config_dir = body.config_dir.as_str();

    let config_file = body.config_file.as_str();

    let args = vec!["-d", config_dir, "-f", config_file];

    let log = File::create(body.log_file).context("failed to open log")?;
    let child = Command::new(body.bin_path).args(args).stdout(log).spawn()?;

    // 获取进程ID
    let pid = child.id();

    let mut arc = ClashStatus::global().lock();
    arc.info = Some(body_cloned);
    arc.process_id = Some(pid);
    Ok(())
}

/// POST /stop_clash
/// 停止clash进程
pub fn stop_clash() -> Result<()> {
    let mut arc = ClashStatus::global().lock();
    let pid = arc.process_id.take();
    arc.info = None;

    let mut system = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );
    system.refresh_processes(ProcessesToUpdate::All);

    if let Some(pid) = pid {
        if let Some(process) = system.process(sysinfo::Pid::from_u32(pid)) {
            process.kill_with(sysinfo::Signal::Term);

            let mut attempts = 0;
            while attempts < 5 {
                std::thread::sleep(std::time::Duration::from_millis(100));

                let mut new_system = sysinfo::System::new();
                new_system.refresh_processes(ProcessesToUpdate::All);

                let process_exists = new_system.process(sysinfo::Pid::from_u32(pid)).is_some();
                if !process_exists {
                    return Ok(());
                }
                attempts += 1;
            }

            process.kill();
        }
    }
    Ok(())
}

/// GET /get_clash
/// 获取clash当前执行信息
pub fn get_clash() -> Result<StartBody> {
    let arc = ClashStatus::global().lock();

    match arc.info.clone() {
        Some(info) => Ok(info),
        None => bail!("clash not executed"),
    }
}
