#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod windows;

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

#[cfg(not(windows))]
fn main() {
    eprintln!("usque-update is available only on Windows");
    std::process::exit(1);
}
