//! `generate` writes vectors.json and circuits/lib/src/test_vectors.nr from a
//! fresh render; `check` (the default, and what CI runs) exits 1 when either
//! committed file differs from that render.

use std::process::ExitCode;

use stellpoker_test_vectors::{generate, render_all, repo_root, stale_files};

fn main() -> ExitCode {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "check".to_string());
    let root = repo_root();
    match mode.as_str() {
        "generate" => {
            for rendered in render_all(&generate()) {
                let path = root.join(rendered.relative_path);
                std::fs::write(&path, rendered.content).expect("write vector file");
                println!("wrote {}", rendered.relative_path);
            }
            ExitCode::SUCCESS
        }
        "check" => {
            let stale = stale_files(&root);
            if stale.is_empty() {
                println!("shared test vectors are current");
                return ExitCode::SUCCESS;
            }
            for (path, reason) in &stale {
                eprintln!("stale: {path} ({reason})");
            }
            eprintln!("regenerate and commit: cargo run -p stellpoker-test-vectors -- generate");
            ExitCode::FAILURE
        }
        other => {
            eprintln!("unknown mode {other:?}; use `generate` or `check`");
            ExitCode::FAILURE
        }
    }
}
