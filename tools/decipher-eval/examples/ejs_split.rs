//! Lab measurement: how long does the yt-dlp-ejs solver spend preparing a player (parse + rewrite)
//! versus answering `n`/`sig` challenges once the prepared player exists? Uses the cached player
//! and SYNTHETIC challenges only; prints durations, sizes and output lengths, never player source.
//!
//!   ejs_split <solver_dir> [--id <cache id>]
use rquickjs::{Context, Function, Runtime, Value};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use vibedeck_decipher_eval::ejs_solver::{clean, read_bundle};
use vibedeck_decipher_eval::player_cache;

const N_IN: &str = "aB3dE5gH7jK9mN1pQ";
const SIG_IN: &str = "AOq0QJ8wRgIhAKx1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9t0uvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghij";

fn run(lib: String, core: String, player: String) -> Result<String, String> {
    let runtime = Runtime::new().map_err(|_| "runtime_setup".to_string())?;
    runtime.set_memory_limit(1024 * 1024 * 1024);
    runtime.set_max_stack_size(256 * 1024 * 1024);
    let context = Context::full(&runtime).map_err(|_| "context_setup".to_string())?;
    context.with(|ctx| -> Result<String, String> {
        for (what, source) in [("lib", lib.as_str()), ("globals", "Object.assign(globalThis, lib);"), ("core", core.as_str())] {
            let t = Instant::now();
            ctx.eval::<Value, _>(source).map_err(|_| format!("{what}_failed: {}", clean(&format!("{:?}", ctx.catch()))))?;
            println!("load {what}: {} ms", t.elapsed().as_millis());
        }
        // Stage 1: prepare the player and keep the rewritten script.
        let prepare: Function = ctx
            .eval("(function(p){const r=jsc({type:'player',player:p,requests:[],output_preprocessed:true});return r.preprocessed_player;})")
            .map_err(|_| "prepare_harness_failed".to_string())?;
        let t = Instant::now();
        let prepared: String = prepare.call((player.as_str(),)).map_err(|_| format!("prepare_threw: {}", clean(&format!("{:?}", ctx.catch()))))?;
        println!("prepare: {} ms, prepared script {} bytes (source {} bytes)", t.elapsed().as_millis(), prepared.len(), player.len());
        // Stage 2: answer challenges from the prepared script, several times.
        let solve: Function = ctx
            .eval(format!(
                "(function(pp){{const r=jsc({{type:'preprocessed',preprocessed_player:pp,requests:[{{type:'n',challenges:['{N_IN}']}},{{type:'sig',challenges:['{SIG_IN}']}}]}});\
                 return r.responses.map(x=>x.type==='result'?('OK:'+Object.values(x.data)[0].length):('ERR:'+String(x.error).split('\\n')[0])).join('|');}})"
            ))
            .map_err(|_| "solve_harness_failed".to_string())?;
        let mut last = String::new();
        for round in 1..=3 {
            let t = Instant::now();
            last = solve.call((prepared.as_str(),)).map_err(|_| format!("solve_threw: {}", clean(&format!("{:?}", ctx.catch()))))?;
            println!("solve round {round}: {} ms", t.elapsed().as_millis());
        }
        Ok(last)
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(dir) = args.first() else {
        println!("{{\"error\":\"usage: ejs_split <solver_dir> [--id <id>]\"}}");
        std::process::exit(2);
    };
    let id = args.iter().position(|a| a == "--id").and_then(|i| args.get(i + 1)).cloned();
    let project = std::env::current_dir().unwrap_or_default();
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let source = match player_cache::default_base(&project).and_then(|b| player_cache::load(&b, id.as_deref(), now)) {
        Ok((_, s)) => s,
        Err(code) => {
            println!("{{\"error\":\"{code}\"}}");
            std::process::exit(2);
        }
    };
    let dir = std::path::Path::new(dir);
    let (lib, core) = match (read_bundle(dir, "lib.min.js"), read_bundle(dir, "core.min.js")) {
        (Ok(l), Ok(c)) => (l, c),
        (Err(e), _) | (_, Err(e)) => {
            println!("{{\"error\":\"{e}\"}}");
            std::process::exit(2);
        }
    };
    let result = std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(move || run(lib, core, source))
        .map(|h| h.join().unwrap_or_else(|_| Err("thread_panicked".into())))
        .unwrap_or_else(|_| Err("thread_spawn_failed".into()));
    match result {
        Ok(answer) => println!("answers (lengths only): {answer}"),
        Err(code) => println!("{{\"error\":\"{}\"}}", clean(&code)),
    }
}
