fn main() {
    println!("cargo:rerun-if-changed=../.env");
    if let Ok(contents) = std::fs::read_to_string("../.env") {
        for line in contents.lines() {
            if let Some(secret) = line.strip_prefix("GOOGLE_CLIENT_SECRET=") {
                println!("cargo:rustc-env=VIBE_GOOGLE_CLIENT_SECRET={secret}");
            }
        }
    }
    tauri_build::build()
}
