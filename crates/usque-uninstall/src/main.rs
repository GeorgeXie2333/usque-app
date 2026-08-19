#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let show_dialog = !arguments.iter().any(|argument| argument == "--dry-run");
    let code = match usque_uninstall::run(arguments) {
        Ok(code) => code,
        Err(error) => {
            usque_uninstall::emit_error(&error, show_dialog);
            1
        }
    };
    std::process::exit(code);
}
