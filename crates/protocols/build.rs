//! Lets local builds take the Google OAuth client from the workspace `.env`,
//! read by `option_env!` in `oauth.rs`. Real environment variables win, which
//! is how CI passes its secrets.

use std::path::Path;

#[path = "src/dotenv.rs"]
mod dotenv;

const KEYS: [&str; 2] = ["BGMAIL_GOOGLE_CLIENT_ID", "BGMAIL_GOOGLE_CLIENT_SECRET"];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/dotenv.rs");
    for key in KEYS {
        println!("cargo:rerun-if-env-changed={key}");
    }
    // Watched only when present: a missing watched file makes cargo rebuild
    // this crate on every build. A first .env needs `touch crates/protocols/build.rs`.
    let path = Path::new("../../.env");
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    println!("cargo:rerun-if-changed={}", path.display());
    for key in KEYS {
        if std::env::var_os(key).is_none() {
            if let Some(value) = dotenv::lookup(&text, key) {
                println!("cargo:rustc-env={key}={value}");
            }
        }
    }
}
