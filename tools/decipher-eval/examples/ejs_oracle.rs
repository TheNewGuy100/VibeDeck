//! Lab oracle: runs the yt-dlp-ejs solver inside a fresh QuickJS runtime (no network, files,
//! processes or other host bindings: the engine has none) and compares its output for SYNTHETIC
//! `n` inputs with the candidate operation sequence found by static reading of the player.
//!
//!   ejs_oracle <solver_dir> --selftest       plumbing check against a tiny synthetic player
//!   ejs_oracle <solver_dir> [--id <cache id>] runs against the cached player (see player_lab)
//!
//! The host reads only the two solver bundles and the cached player; the runtime never touches
//! the file system. Limits are far above the app executor's on purpose: this is a lab tool, not
//! the app. Prints counts, lengths and the synthetic inputs/outputs, never player source.
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vibedeck_decipher_eval::ejs_solver::{clean, read_bundle, run_solver_threaded};
use vibedeck_decipher_eval::player_cache;

// Synthetic challenge strings: random-looking, 16-21 chars, never provider data.
const TEST_INPUTS: [&str; 6] = [
    "aB3dE5gH7jK9mN1pQ",
    "Zx8Yw7Vu6Ts5Rq4Pon",
    "q1w2e3r4t5y6u7i8o9p0",
    "AbCdEfGhIjKlMnOpQrStU",
    "0123456789abcdefg",
    "_-_-AAAA-BBBB_CCCC-",
];

#[derive(Clone, Copy)]
enum Op {
    Swap(usize),
    Reverse,
    Splice(usize),
}

// Resolved statically from the player's flattened dispatcher (d = 2471); see docs/investigation.md.
const CANDIDATE: [Op; 8] = [
    Op::Swap(1),
    Op::Reverse,
    Op::Swap(53),
    Op::Swap(9),
    Op::Reverse,
    Op::Splice(3),
    Op::Reverse,
    Op::Splice(3),
];

fn apply(ops: &[Op], input: &str) -> String {
    let mut e: Vec<char> = input.chars().collect();
    for op in ops {
        match *op {
            Op::Swap(b) if !e.is_empty() => {
                let i = b % e.len();
                e.swap(0, i);
            }
            Op::Swap(_) => {}
            Op::Reverse => e.reverse(),
            Op::Splice(b) => {
                let n = b.min(e.len());
                e.drain(0..n);
            }
        }
    }
    e.into_iter().collect()
}

// Expected result of the synthetic player below: reverse, then drop the first two characters.
const SELFTEST_OPS: [Op; 2] = [Op::Reverse, Op::Splice(2)];
const SELFTEST_PLAYER: &str = "var _yt_player={};(function(g){var window=this;\
var U=class{constructor(a){this.a=a;this.q={}}set(k,v){this.q[k]=v}get(k){return this.q[k]||null}\
mat(){if(this.q.n){var e=this.q.n.split(\"\");e.reverse();e.splice(0,2);this.q.n=e.join(\"\")}}};\
var F=function(a,b=\"\",c=\"\"){a=new U(a);a.set(\"alr\",\"yes\");return a};\
g.F=F;})(_yt_player);";

/// Parses a JSON array of plain strings like `["abc","def"]` without a JSON dependency. Entries
/// are limited to `[A-Za-z0-9_-]{8,64}`, at most 64 of them, de-duplicated in order.
fn parse_challenges(text: &str) -> Result<Vec<String>, &'static str> {
    // PowerShell pipes text with a UTF-8 BOM.
    let trimmed = text.trim_start_matches('\u{feff}').trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|t| t.strip_suffix(']'))
        .ok_or("invalid_challenge_list")?;
    let mut out: Vec<String> = Vec::new();
    for item in inner.split(',').map(str::trim).filter(|i| !i.is_empty()) {
        let value = item
            .strip_prefix('"')
            .and_then(|t| t.strip_suffix('"'))
            .ok_or("invalid_challenge_list")?;
        let ok = (8..=64).contains(&value.len())
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if !ok {
            return Err("invalid_challenge_list");
        }
        if !out.iter().any(|v| v == value) {
            out.push(value.to_string());
        }
    }
    if out.is_empty() || out.len() > 64 {
        return Err("invalid_challenge_list");
    }
    Ok(out)
}

fn report(
    player_bytes: usize,
    ops: &[Op],
    solver_text: Result<String, String>,
    elapsed: Duration,
) -> bool {
    let text = match solver_text {
        Ok(t) => t,
        Err(message) => {
            println!(
                "{{\"solver\":\"error\",\"message\":\"{}\",\"playerBytes\":{player_bytes},\"elapsedMs\":{}}}",
                clean(&message).replace('"', "'"),
                elapsed.as_millis()
            );
            return false;
        }
    };
    if let Some(error) = text.strip_prefix("ERR\t") {
        println!(
            "{{\"solver\":\"returned_error\",\"message\":\"{}\",\"playerBytes\":{player_bytes},\"elapsedMs\":{}}}",
            clean(error).replace('"', "'"),
            elapsed.as_millis()
        );
        return false;
    }
    let mut rows = Vec::new();
    let mut all_match = true;
    for line in text.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some("OK"), Some(input), Some(output)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let candidate = apply(ops, input);
        let matches = candidate == output;
        all_match &= matches;
        rows.push(format!(
            "{{\"in\":\"{input}\",\"solver\":\"{}\",\"candidate\":\"{candidate}\",\"match\":{matches}}}",
            clean(output).replace('"', "'")
        ));
    }
    println!(
        "{{\"solver\":\"ok\",\"playerBytes\":{player_bytes},\"elapsedMs\":{},\"rows\":[{}],\"allMatch\":{}}}",
        elapsed.as_millis(),
        rows.join(","),
        all_match && !rows.is_empty()
    );
    all_match && !rows.is_empty()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(dir) = args.first() else {
        println!(
            "{{\"error\":\"usage: ejs_oracle <solver_dir> [--selftest | --stdin | --id <id>]\"}}"
        );
        std::process::exit(2);
    };
    let dir = std::path::PathBuf::from(dir);
    let (lib, core) = match (
        read_bundle(&dir, "lib.min.js"),
        read_bundle(&dir, "core.min.js"),
    ) {
        (Ok(l), Ok(c)) => (l, c),
        (Err(e), _) | (_, Err(e)) => {
            println!("{{\"error\":\"{e}\"}}");
            std::process::exit(2);
        }
    };
    let selftest = args.iter().any(|a| a == "--selftest");
    let stdin_mode = args.iter().any(|a| a == "--stdin");
    if selftest && stdin_mode {
        println!("{{\"error\":\"selftest_and_stdin_are_exclusive\"}}");
        std::process::exit(2);
    }
    // --stdin: a JSON array of challenge strings in, [[in,out],...] out. Values stay on this
    // machine (stdin/stdout); nothing is logged or written to disk.
    let challenges: Vec<String> = if stdin_mode {
        let mut text = String::new();
        let parsed = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
            .map_err(|_| "stdin_not_utf8")
            .and_then(|_| parse_challenges(&text));
        match parsed {
            Ok(list) => list,
            Err(code) => {
                println!("{{\"error\":\"{code}\"}}");
                std::process::exit(2);
            }
        }
    } else {
        TEST_INPUTS.iter().map(|s| s.to_string()).collect()
    };
    let (player, ops): (String, &[Op]) = if selftest {
        (SELFTEST_PLAYER.to_string(), &SELFTEST_OPS)
    } else {
        let id = args
            .iter()
            .position(|a| a == "--id")
            .and_then(|i| args.get(i + 1))
            .cloned();
        let project = std::env::current_dir().unwrap_or_default();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let loaded = player_cache::default_base(&project)
            .and_then(|base| player_cache::load(&base, id.as_deref(), now));
        match loaded {
            Ok((_, source)) => (source, &CANDIDATE),
            Err(code) => {
                println!("{{\"error\":\"{code}\"}}");
                std::process::exit(2);
            }
        }
    };
    let bytes = player.len();
    let started = Instant::now();
    let for_thread = challenges.clone();
    let result = run_solver_threaded(lib, core, player, for_thread);
    if stdin_mode {
        std::process::exit(print_pairs(result));
    }
    let ok = report(bytes, ops, result, started.elapsed());
    std::process::exit(if ok { 0 } else { 1 });
}

/// JSON pairs for the page-side comparison plus how many pairs were dropped. Anything that is
/// not plain base64url-like is dropped and counted, so a surprising solver output can never
/// break or inject into the JSON.
fn pairs_json(text: &str) -> Result<(String, usize), String> {
    if let Some(error) = text.strip_prefix("ERR\t") {
        return Err(clean(error).replace('"', "'"));
    }
    let plain = |s: &str| {
        !s.is_empty()
            && s.len() <= 128
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    let mut pairs = Vec::new();
    let mut dropped = 0;
    for line in text.lines() {
        let mut parts = line.splitn(3, '\t');
        if let (Some("OK"), Some(input), Some(output)) = (parts.next(), parts.next(), parts.next())
        {
            if plain(input) && plain(output) {
                pairs.push(format!("[\"{input}\",\"{output}\"]"));
            } else {
                dropped += 1;
            }
        }
    }
    Ok((format!("[{}]", pairs.join(",")), dropped))
}

fn print_pairs(result: Result<String, String>) -> i32 {
    let text = match result {
        Ok(t) => t,
        Err(message) => {
            println!("{{\"error\":\"{}\"}}", clean(&message).replace('"', "'"));
            return 1;
        }
    };
    match pairs_json(&text) {
        Ok((json, dropped)) => {
            if dropped > 0 {
                eprintln!("note: {dropped} pair(s) dropped: the solver output had characters outside A-Z a-z 0-9 _ -");
            }
            println!("{json}");
            0
        }
        Err(message) => {
            println!("{{\"error\":\"{message}\"}}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_plain_challenge_lists() {
        assert_eq!(
            parse_challenges(" [\"abcdefgh12\", \"Zz_-0123456\",\"abcdefgh12\"] ").unwrap(),
            vec!["abcdefgh12".to_string(), "Zz_-0123456".to_string()]
        );
        assert_eq!(
            parse_challenges("\u{feff}[\"abcdefgh12\"]\r\n").unwrap(),
            vec!["abcdefgh12".to_string()]
        );
        for bad in [
            "",
            "[]",
            "abc",
            "[\"short\"]",
            "[\"has space inside\"]",
            "[\"quo\\\"te12345\"]",
            "[abcdefgh12]",
            "[\"abcdefgh12\\u0041\"]",
            "{\"a\":1}",
        ] {
            assert_eq!(
                parse_challenges(bad).unwrap_err(),
                "invalid_challenge_list",
                "{bad}"
            );
        }
        let many = format!(
            "[{}]",
            (0..65)
                .map(|i| format!("\"abcdefgh{i:04}\""))
                .collect::<Vec<_>>()
                .join(",")
        );
        assert!(parse_challenges(&many).is_err());
    }

    #[test]
    fn pairs_json_keeps_plain_pairs_and_counts_what_it_drops() {
        assert_eq!(
            pairs_json("OK\tabcdefgh12\tXYZ-_09").unwrap(),
            (String::from("[[\"abcdefgh12\",\"XYZ-_09\"]]"), 0)
        );
        assert_eq!(
            pairs_json("OK\tabcdefgh12\tbad.out\nOK\tijklmnop34\tfine").unwrap(),
            (String::from("[[\"ijklmnop34\",\"fine\"]]"), 1)
        );
        assert_eq!(pairs_json("ERR\tbad").unwrap_err(), "bad");
        assert_eq!(print_pairs(Err("boom".into())), 1);
    }
}
