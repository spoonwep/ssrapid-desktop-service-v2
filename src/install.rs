#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn main() {
    panic!("This program is not intended to run on this platform.");
}

#[cfg(not(windows))]
use anyhow::Error;
#[cfg(target_os = "macos")]
fn main() -> Result<(), Error> {
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;

    let service_binary_path = std::env::current_exe()
        .unwrap()
        .with_file_name("clash-verge-service");
    let target_binary_path = "/Library/PrivilegedHelperTools/io.github.clashverge.helper";
    if !service_binary_path.exists() {
        eprintln!("The clash-verge-service binary not found.");
        std::process::exit(2);
    }

    std::fs::copy(service_binary_path, target_binary_path).expect("Unable to copy service file");

    let plist_file = "/Library/LaunchDaemons/io.github.clashverge.helper.plist";
    let plist_file = Path::new(plist_file);

    let plist_file_content = include_str!("io.github.clashverge.helper.plist");
    let mut file = File::create(plist_file).expect("Failed to create file for writing.");
    file.write_all(plist_file_content.as_bytes())
        .expect("Unable to write plist file");
    std::process::Command::new("chmod")
        .arg("644")
        .arg(plist_file)
        .output()
        .expect("Failed to chmod");
    std::process::Command::new("chown")
        .arg("root:wheel")
        .arg(plist_file)
        .output()
        .expect("Failed to chown");
    std::process::Command::new("chmod")
        .arg("544")
        .arg(target_binary_path)
        .output()
        .expect("Failed to chmod");
    std::process::Command::new("chown")
        .arg("root:wheel")
        .arg(target_binary_path)
        .output()
        .expect("Failed to chown");
    // Unload before load the service.
    std::process::Command::new("launchctl")
        .arg("unload")
        .arg(plist_file)
        .output()
        .expect("Failed to unload service.");
    // Load the service.
    std::process::Command::new("launchctl")
        .arg("load")
        .arg(plist_file)
        .output()
        .expect("Failed to load service.");
    // Start the service.
    std::process::Command::new("launchctl")
        .arg("start")
        .arg("io.github.clashverge.helper")
        .output()
        .expect("Failed to load service.");
    Ok(())
}
#[cfg(target_os = "linux")]
fn main() -> Result<(), Error> {
    const SERVICE_NAME: &str = "clash-verge-service";
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;

    let service_binary_path = std::env::current_exe()
        .unwrap()
        .with_file_name("clash-verge-service");
    if !service_binary_path.exists() {
        eprintln!("The clash-verge-service binary not found.");
        std::process::exit(2);
    }

    // Peek the status of the service.
    let status_code = std::process::Command::new("systemctl")
        .arg("status")
        .arg(format!("{}.service", SERVICE_NAME))
        .arg("--no-pager")
        .output()
        .expect("Failed to execute 'systemctl status' command.")
        .status
        .code();

    /*
     * https://www.freedesktop.org/software/systemd/man/latest/systemctl.html#Exit%20status
     */
    match status_code {
        Some(code) => match code {
            0 => return Ok(()),
            1 | 2 | 3 => {
                std::process::Command::new("systemctl")
                    .arg("start")
                    .arg(format!("{}.service", SERVICE_NAME))
                    .output()
                    .expect("Failed to execute 'systemctl start' command.");
                return Ok(());
            }
            4 => {}
            _ => {
                panic!("Unexpected status code from systemctl status")
            }
        },
        None => {
            panic!("systemctl was improperly terminated.");
        }
    }

    let unit_file = format!("/etc/systemd/system/{}.service", SERVICE_NAME);
    let unit_file = Path::new(&unit_file);

    let unit_file_content = format!(
        include_str!("systemd_service_unit.tmpl"),
        service_binary_path.to_str().unwrap()
    );
    let mut file = File::create(unit_file).expect("Failed to create file for writing.");
    file.write_all(unit_file_content.as_bytes())
        .expect("Unable to write unit file");

    // Reload unit files and start service.
    std::process::Command::new("systemctl")
        .arg("daemon-reload")
        .output()
        .and_then(|_| {
            std::process::Command::new("systemctl")
                .arg("enable")
                .arg(SERVICE_NAME)
                .arg("--now")
                .output()
        })
        .expect("Failed to start service.");
    Ok(())
}

/// install and start the service
#[cfg(windows)]
fn main() -> windows_service::Result<()> {
    use std::ffi::{OsStr, OsString};
    use windows_service::{
        service::{
            ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceState,
            ServiceType,
        },
        service_manager::{ServiceManager, ServiceManagerAccess},
    };

    let manager_access = ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

    let service_access = ServiceAccess::QUERY_STATUS | ServiceAccess::START;
    if let Ok(service) = service_manager.open_service("clash_verge_service", service_access) {
        if let Ok(status) = service.query_status() {
            match status.current_state {
                ServiceState::StopPending
                | ServiceState::Stopped
                | ServiceState::PausePending
                | ServiceState::Paused => {
                    service.start(&Vec::<&OsStr>::new())?;
                }
                _ => {}
            };

            return Ok(());
        }
    }

    let service_binary_path = std::env::current_exe()
        .unwrap()
        .with_file_name("clash-verge-service.exe");

    if !service_binary_path.exists() {
        eprintln!("clash-verge-service.exe not found");
        std::process::exit(2);
    }

    let service_info = ServiceInfo {
        name: OsString::from("clash_verge_service"),
        display_name: OsString::from("Clash Verge Service"),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: service_binary_path,
        launch_arguments: vec![],
        dependencies: vec![],
        account_name: None, // run as System
        account_password: None,
    };

    let start_access = ServiceAccess::CHANGE_CONFIG | ServiceAccess::START;
    let service = service_manager.create_service(&service_info, start_access)?;

    service.set_description("Clash Verge Service helps to launch clash core")?;
    service.start(&Vec::<&OsStr>::new())?;

    Ok(())
}
