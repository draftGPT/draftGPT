use std::path::Path;

/// In the placeholder page, so a later build can tell it from the real one.
const PLACEHOLDER_MARK: &str = "<meta name=\"draftgpt-placeholder\">";

fn main() {
    // `tauri::generate_context!` embeds ui/desktop/dist at compile time and
    // fails if it's missing. On a fresh clone (or a Rust-only CI) the UI isn't
    // built yet, so debug builds get a placeholder page that says how to build
    // it. A release build must ship the real page, so it fails instead.
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/desktop/dist");
    let index = dist.join("index.html");
    let real = std::fs::read_to_string(&index).is_ok_and(|html| !html.contains(PLACEHOLDER_MARK));
    if !real {
        assert!(
            std::env::var("PROFILE").as_deref() != Ok("release"),
            "ui/desktop/dist has no built page: run `npm install` and `npm run build -w @draftgpt/desktop` in ui/ first"
        );
        println!("cargo:warning=ui/desktop/dist has no built page; the panel shows a placeholder");
        if !index.exists() {
            std::fs::create_dir_all(&dist).expect("create ui/desktop/dist");
            let page = format!(
                "<!doctype html><meta charset=utf-8>{PLACEHOLDER_MARK}<title>draftGPT</title>\
                 <body style=\"font-family:sans-serif;background:#0b0f17;color:#e6e9f0;padding:16px\">\
                 <p>The panel UI isn't built. Run <code>npm install</code> and \
                 <code>npm run build -w @draftgpt/desktop</code> in <code>ui/</code>, then rebuild.</p>"
            );
            std::fs::write(&index, page).expect("write placeholder index.html");
        }
    }
    println!("cargo:rerun-if-changed=../../ui/desktop/dist/index.html");
    tauri_build::build();
}
