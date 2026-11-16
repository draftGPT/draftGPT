use std::path::Path;

fn main() {
    // `tauri::generate_context!` embeds ui/desktop/dist at compile time and
    // fails if it's missing. On a fresh clone (or a Rust-only CI) the UI isn't
    // built yet, so put a placeholder page there that says how to build it.
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/desktop/dist");
    if !dist.join("index.html").exists() {
        // A release build must ship the real page.
        assert!(
            std::env::var("PROFILE").as_deref() != Ok("release"),
            "ui/desktop/dist is missing: run `npm install` and `npm run build -w @draftgpt/desktop` in ui/ first"
        );
        println!("cargo:warning=ui/desktop/dist is missing; the panel shows a placeholder page");
        std::fs::create_dir_all(&dist).expect("create ui/desktop/dist");
        std::fs::write(
            dist.join("index.html"),
            "<!doctype html><meta charset=utf-8><title>draftGPT</title>\
             <body style=\"font-family:sans-serif;background:#0b0f17;color:#e6e9f0;padding:16px\">\
             <p>The panel UI isn't built. Run <code>npm install</code> and \
             <code>npm run build -w @draftgpt/desktop</code> in <code>ui/</code>, then rebuild.</p>",
        )
        .expect("write placeholder index.html");
    }
    println!("cargo:rerun-if-changed=../../ui/desktop/dist/index.html");
    tauri_build::build();
}
