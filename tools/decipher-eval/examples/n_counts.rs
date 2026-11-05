//! Offline counts-only projection of a player script read from stdin.
//! Prints numbers and fixed codes only; never echoes source, names or values.
//! Pipe from memory (e.g. clipboard) or use `player_lab` to keep a local cache outside the project.
use std::io::Read;
use vibedeck_decipher_eval::native_transform::diagnostic_json;

fn main() {
    let mut source = String::new();
    if std::io::stdin().read_to_string(&mut source).is_err() {
        println!("{{\"error\":\"stdin_not_utf8\"}}");
        return;
    }
    println!("{}", diagnostic_json(&source));
}
