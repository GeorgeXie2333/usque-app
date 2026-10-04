//! Separate, non-shipping visual preview. No argument can enable a real uninstall.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn preview_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Vec<String>, String> {
    let mut result = vec![
        "--preview".to_owned(),
        "--preview-locale=zh-CN".to_owned(),
        "--preview-theme=dark".to_owned(),
    ];
    for argument in arguments {
        if argument == "--preview"
            || argument.starts_with("--preview=")
            || argument.starts_with("--preview-locale=")
            || argument.starts_with("--preview-theme=")
        {
            result.push(argument);
        } else {
            return Err(
                "This preview only accepts preview scenario, language, and theme options."
                    .to_owned(),
            );
        }
    }
    Ok(result)
}

fn main() {
    let result = preview_arguments(std::env::args().skip(1))
        .map_err(usque_uninstall::UninstallError::Detail)
        .and_then(usque_uninstall::run);
    let code = match result {
        Ok(code) => code,
        Err(error) => {
            usque_uninstall::emit_error(&error, true);
            1
        }
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_binary_cannot_accept_live_modes_or_product_codes() {
        for argument in [
            "--quiet",
            "--product-code={076CF387-E447-4666-9153-2DA16049A390}",
            "--remove-user-data",
            "--stage-quiet=1",
        ] {
            assert!(preview_arguments([argument.to_owned()]).is_err());
        }
        let arguments = preview_arguments(Vec::new()).unwrap();
        assert!(matches!(
            usque_uninstall::Cli::parse(arguments).unwrap().mode,
            usque_uninstall::Mode::Preview(_)
        ));
    }
}
