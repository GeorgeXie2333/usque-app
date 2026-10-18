#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(windows)]
fn main() {
    let exit_code = match windows::run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    };
    std::process::exit(exit_code);
}

#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = linux::run() {
        eprintln!("{error}");
        std::process::exit(error.exit_code());
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn main() {
    eprintln!("usque-update is available only on Windows");
    std::process::exit(1);
}
