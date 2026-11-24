use std::path::Path;

/// The page zerochat serves when it was built before the web UI: it says how to build the UI.
const PLACEHOLDER: &str = r#"<!doctype html>
<title>zerochat</title>
<p>The zerochat web UI has not been built.
Run <code>npm --prefix web ci && npm --prefix web run build</code>, then rebuild zerochat.</p>
"#;

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    if !dist.join("index.html").is_file() {
        std::fs::create_dir_all(&dist).expect("create web/dist");
        std::fs::write(dist.join("index.html"), PLACEHOLDER).expect("write placeholder");
    }
}
