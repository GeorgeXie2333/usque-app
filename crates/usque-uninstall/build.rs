use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let strings = root.join("../../packaging/windows/setup/strings.json");
    println!("cargo:rerun-if-changed={}", strings.display());
    println!("cargo:rerun-if-changed=usque-uninstall.manifest");
    let source = fs::read_to_string(&strings).expect("read shared setup translations");
    let locales: serde_json::Value =
        serde_json::from_str(&source).expect("setup translations JSON");
    let mut generated = String::from(
        "pub fn setup_text(locale: &str, key: &str) -> &'static str {\n match (locale, key) {\n",
    );
    for (locale, entries) in locales.as_object().expect("locale map") {
        for (key, value) in entries.as_object().expect("translation map") {
            generated.push_str(&format!(
                "({locale:?}, {key:?}) => {:?},\n",
                value.as_str().expect("translation text")
            ));
        }
    }
    generated.push_str(
        "_ if locale != \"en-US\" => setup_text(\"en-US\", key),\n_ => \"Usque\",\n}\n}\n",
    );
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").expect("output directory")).join("setup_strings.rs"),
        generated,
    )
    .expect("write compiled setup translations");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    winresource::WindowsResource::new()
        .set("ProductName", "Usque")
        .set("FileDescription", "Usque uninstall")
        .set("OriginalFilename", "usque-uninstall.exe")
        .set_manifest_file("usque-uninstall.manifest")
        .set_icon("../../apps/usque_gui/windows/runner/resources/app_icon.ico")
        .compile()
        .expect("compile uninstaller asInvoker resource");
}
