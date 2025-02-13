use super::data::*;
use anyhow::{bail, Context, Result};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs::File;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Default)]
pub struct ClashStatus {
    pub info: Option<StartBody>,
    pub process_id: Option<u32>,
    pub is_running: Arc<AtomicBool>,
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
    // 先检查是否已有进程在运行
    let mut status = ClashStatus::global().lock();
    if status.is_running.load(Ordering::SeqCst) {
        // 如果有进程在运行，先尝试停止它
        if let Some(old_pid) = status.process_id {
            drop(status); // 释放锁，避免死锁
            log::warn!("Found existing process {}, attempting to stop it first", old_pid);
            let _ = stop_clash();
            // 等待一段时间确保进程完全终止
            std::thread::sleep(std::time::Duration::from_millis(100));
            status = ClashStatus::global().lock();
        }
    }

    let body_cloned = body.clone();
    let config_dir = body.config_dir.as_str();
    let config_file = body.config_file.as_str();
    let args = vec!["-d", config_dir, "-f", config_file];

    let log = File::create(&body.log_file).context("failed to open log")?;

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;

        unsafe {
            let child = Command::new(&body.bin_path)
                .args(&args)
                .stdout(log)
                .pre_exec(|| {
                    libc::setpgid(0, 0);
                    Ok(())
                })
                .spawn()?;

            let pid = child.id();
            status.info = Some(body_cloned);
            status.process_id = Some(pid);
            status.is_running.store(true, Ordering::SeqCst);
            log::info!("Started new process with PID {}", pid);
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let child = Command::new(&body.bin_path)
            .args(&args)
            .stdout(log)
            .spawn()?;

        let pid = child.id();
        status.info = Some(body_cloned);
        status.process_id = Some(pid);
        status.is_running.store(true, Ordering::SeqCst);
        log::info!("Started new process with PID {}", pid);
    }

    Ok(())
}

/// POST /stop_clash
/// 停止clash进程
pub fn stop_clash() -> Result<()> {
    let mut status = ClashStatus::global().lock();
    
    if !status.is_running.load(Ordering::SeqCst) {
        log::info!("No running process found");
        return Ok(());
    }

    if let Some(pid) = status.process_id.take() {
        log::info!("Stopping process {}", pid);
        
        #[cfg(target_os = "linux")]
        unsafe {
            // 1. 发送 SIGTERM
            if libc::kill(pid as i32, libc::SIGTERM) == 0 {
                // 2. 等待进程退出（最多1秒）
                let mut attempts = 0;
                while attempts < 10 {
                    match libc::kill(pid as i32, 0) {
                        0 => {
                            std::thread::sleep(std::time::Duration::from_millis(100));
                            attempts += 1;
                        }
                        _ => {
                            log::info!("Process {} terminated successfully", pid);
                            break;
                        }
                    }
                }

                // 3. 如果还没退出，发送 SIGKILL
                if attempts >= 10 {
                    log::warn!("Process {} did not respond to SIGTERM, sending SIGKILL", pid);
                    libc::kill(pid as i32, libc::SIGKILL);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};
            let mut system = System::new_with_specifics(
                RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
            );
            system.refresh_processes(ProcessesToUpdate::All);

            if let Some(process) = system.process(sysinfo::Pid::from_u32(pid)) {
                process.kill();
                log::info!("Sent kill signal to process {}", pid);
                // 等待进程退出
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }

    // 清理状态
    status.info = None;
    status.is_running.store(false, Ordering::SeqCst);
    log::info!("Process status cleared");

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

// 在服务启动时调用这个函数
#[cfg(target_os = "linux")]
pub fn init_signal_handler() {
    use nix::sys::signal::{self, SigAction, SigHandler, Signal};
    unsafe {
        // 设置 SIGCHLD 处理函数，自动回收子进程
        let sa = SigAction::new(
            SigHandler::Handler(handle_sigchld),
            signal::SaFlags::empty(),
            signal::SigSet::empty(),
        );
        let _ = signal::sigaction(Signal::SIGCHLD, &sa);
    }
}

#[cfg(target_os = "linux")]
extern "C" fn handle_sigchld(_: i32) {
    unsafe {
        // 循环等待所有已终止的子进程
        while libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) > 0 {}
    }
}
