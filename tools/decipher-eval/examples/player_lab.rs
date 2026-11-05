//! Local analysis bench for a public player script, cached outside the project.
//!
//!   player_lab ingest        reads the script from stdin and caches it (24 h, 3 revisions)
//!   player_lab counts [id]   counts-only projection of the cached script (latest by default)
//!   player_lab list          cached revisions: id, size, age (no content)
//!   player_lab purge         removes every cached script
//!
//! Prints numbers and fixed codes only. The cache lives under %LOCALAPPDATA%\VibeDeck\player-lab
//! and is refused if it would fall inside the current directory (the project).
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};
use vibedeck_decipher_eval::native_transform::diagnostic_json;
use vibedeck_decipher_eval::player_cache;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn fail(code: &str) {
    println!("{{\"error\":\"{code}\"}}");
    std::process::exit(2);
}

fn main() {
    let project = std::env::current_dir().unwrap_or_default();
    let base = match player_cache::default_base(&project) {
        Ok(base) => base,
        Err(code) => return fail(code),
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("ingest") => {
            let mut source = String::new();
            if std::io::stdin().read_to_string(&mut source).is_err() {
                return fail("stdin_not_utf8");
            }
            // The Windows clipboard turns \n into \r\n; normalize so the identity of a revision
            // does not depend on how it was copied.
            let normalized = source.contains("\r\n");
            let source = source.replace("\r\n", "\n");
            match player_cache::ingest(&base, &source, now()) {
                Ok(entry) => println!(
                    "{{\"ingested\":\"{}\",\"bytes\":{},\"crlfNormalized\":{normalized}}}",
                    entry.id, entry.bytes
                ),
                Err(code) => fail(code),
            }
        }
        Some("counts") => match player_cache::load(&base, args.get(1).map(String::as_str), now()) {
            Ok((entry, source)) => {
                let json = diagnostic_json(&source);
                println!("{{\"id\":\"{}\",\"result\":{json}}}", entry.id);
            }
            Err(code) => fail(code),
        },
        Some("list") => {
            let now = now();
            let items: Vec<String> = player_cache::list(&base, now)
                .into_iter()
                .map(|e| {
                    format!(
                        "{{\"id\":\"{}\",\"bytes\":{},\"ageSecs\":{}}}",
                        e.id,
                        e.bytes,
                        now.saturating_sub(e.ingested_unix)
                    )
                })
                .collect();
            println!("[{}]", items.join(","));
        }
        Some("purge") => println!("{{\"purged\":{}}}", player_cache::purge_all(&base)),
        _ => fail("usage_ingest_counts_list_purge"),
    }
}
