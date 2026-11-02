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
    // Created when missing because cargo can only watch a file that exists: an
    // absent one rebuilds this crate on every build, and not watching it would
    // leave the first real .env unnoticed behind a fresh build.
    let path = Path::new("../../.env");
    let Ok(text) = dotenv::read_or_create(path) else {
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
