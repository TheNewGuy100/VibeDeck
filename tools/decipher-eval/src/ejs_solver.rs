//! Runs the yt-dlp-ejs solver bundles inside a fresh QuickJS runtime. The engine has no network,
//! file, process or module bindings. Debug/lab use only: the limits are far above the app
//! executor's on purpose, and one solve takes about a minute on a full player.
//!
//! Solver logic: yt-dlp/ejs (Unlicense) bundling meriyah (ISC) and astring (MIT).
//! Errors are fixed or single-line sanitized codes; player source is never echoed.
use rquickjs::{Context, Function, Runtime, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

const MEMORY_LIMIT: usize = 1024 * 1024 * 1024;
const STACK_LIMIT: usize = 256 * 1024 * 1024;
const THREAD_STACK: usize = 512 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(300);
const BUNDLE_LIMIT: u64 = 1024 * 1024;

pub fn clean(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .take(160)
        .collect()
}

fn exception_text(ctx: &rquickjs::Ctx<'_>) -> String {
    let value = ctx.catch();
    if let Some(s) = value.as_string() {
        return clean(&s.to_string().unwrap_or_default());
    }
    if let Some(o) = value.as_object() {
        if let Ok(message) = o.get::<_, String>("message") {
            return clean(&message);
        }
    }
    "non_string_exception".to_string()
}

pub fn read_bundle(dir: &std::path::Path, name: &str) -> Result<String, String> {
    let path = dir.join(name);
    let meta = std::fs::metadata(&path).map_err(|_| format!("{name}_missing"))?;
    if meta.len() > BUNDLE_LIMIT {
        return Err(format!("{name}_too_large"));
    }
    std::fs::read_to_string(&path).map_err(|_| format!("{name}_unreadable"))
}

/// Tab-separated lines: `OK\t<in>\t<out>` per challenge, or one `ERR\t<message>`.
pub fn run_solver(
    lib: &str,
    core: &str,
    player: &str,
    challenge_list: &[String],
) -> Result<String, String> {
    let runtime = Runtime::new().map_err(|_| "runtime_setup".to_string())?;
    runtime.set_memory_limit(MEMORY_LIMIT);
    runtime.set_max_stack_size(STACK_LIMIT);
    let expired = Arc::new(AtomicBool::new(false));
    let flag = expired.clone();
    let deadline = Instant::now() + DEADLINE;
    runtime.set_interrupt_handler(Some(Box::new(move || {
        let stop = Instant::now() >= deadline;
        if stop {
            flag.store(true, Ordering::Relaxed);
        }
        stop
    })));
    let context = Context::full(&runtime).map_err(|_| "context_setup".to_string())?;
    // Callers validate every entry to plain [A-Za-z0-9_-], so no escaping is needed.
    let challenges = format!(
        "[{}]",
        challenge_list
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(",")
    );
    let out = context.with(|ctx| -> Result<String, String> {
        for (what, source) in [("lib", lib), ("globals", "Object.assign(globalThis, lib);"), ("core", core)] {
            if ctx.eval::<Value, _>(source).is_err() {
                return Err(format!("{what}_failed: {}", exception_text(&ctx)));
            }
        }
        let harness: Function = ctx
            .eval(
                "(function(player, challenges){\
                   const r = jsc({type:'player', player:player, requests:[{type:'n', challenges:JSON.parse(challenges)}], output_preprocessed:false});\
                   const resp = r.responses[0];\
                   if (resp.type !== 'result') return 'ERR\\t' + String(resp.error).split('\\n')[0];\
                   return Object.entries(resp.data).map(([k, v]) => 'OK\\t' + k + '\\t' + v).join('\\n');\
                 })",
            )
            .map_err(|_| format!("harness_failed: {}", exception_text(&ctx)))?;
        match harness.call::<_, String>((player, challenges.as_str())) {
            Ok(text) => Ok(text),
            Err(_) => Err(format!("solver_threw: {}", exception_text(&ctx))),
        }
    });
    if expired.load(Ordering::Relaxed) {
        return Err("deadline_exceeded".to_string());
    }
    out
}

/// Same as `run_solver` on a thread with a stack big enough for the solver's recursion.
pub fn run_solver_threaded(
    lib: String,
    core: String,
    player: String,
    challenges: Vec<String>,
) -> Result<String, String> {
    std::thread::Builder::new()
        .stack_size(THREAD_STACK)
        .spawn(move || run_solver(&lib, &core, &player, &challenges))
        .map_err(|_| "solver_thread_spawn_failed".to_string())?
        .join()
        .unwrap_or_else(|_| Err("solver_thread_panicked".to_string()))
}

/// Solves one `n` challenge from the solver directory and the player source. Returns only the
/// transformed value; every failure is a short code.
pub fn solve_one(
    solver_dir: &std::path::Path,
    player: &str,
    challenge: &str,
) -> Result<String, String> {
    let plain = |s: &str| {
        (8..=64).contains(&s.len())
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    if !plain(challenge) {
        return Err("invalid_challenge".to_string());
    }
    let lib = read_bundle(solver_dir, "lib.min.js")?;
    let core = read_bundle(solver_dir, "core.min.js")?;
    let text = run_solver_threaded(lib, core, player.to_string(), vec![challenge.to_string()])?;
    if let Some(error) = text.strip_prefix("ERR\t") {
        return Err(format!("solver_error: {}", clean(error)));
    }
    let output = text
        .lines()
        .find_map(|line| {
            let mut parts = line.splitn(3, '\t');
            match (parts.next(), parts.next(), parts.next()) {
                (Some("OK"), Some(input), Some(output)) if input == challenge => Some(output),
                _ => None,
            }
        })
        .ok_or_else(|| "solver_no_output".to_string())?;
    if output.is_empty()
        || output.len() > 128
        || !output
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err("solver_output_rejected".to_string());
    }
    Ok(output.to_string())
}

// ---- two-stage use: prepare a player once per revision, then answer challenges quickly ----

/// Challenge text accepted for `n` and `sig`: URL-safe characters only, so it can sit inside a
/// JS string literal without escaping.
fn challenge_ok(s: &str) -> bool {
    (1..=512).contains(&s.len())
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'%' | b'=' | b'.' | b'~' | b'+' | b'/')
        })
}

fn with_solver<T: Send + 'static>(
    solver_dir: &std::path::Path,
    job: impl FnOnce(&rquickjs::Ctx<'_>) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let lib = read_bundle(solver_dir, "lib.min.js")?;
    let core = read_bundle(solver_dir, "core.min.js")?;
    std::thread::Builder::new()
        .stack_size(THREAD_STACK)
        .spawn(move || -> Result<T, String> {
            let runtime = Runtime::new().map_err(|_| "runtime_setup".to_string())?;
            runtime.set_memory_limit(MEMORY_LIMIT);
            runtime.set_max_stack_size(STACK_LIMIT);
            let expired = Arc::new(AtomicBool::new(false));
            let flag = expired.clone();
            let deadline = Instant::now() + DEADLINE;
            runtime.set_interrupt_handler(Some(Box::new(move || {
                let stop = Instant::now() >= deadline;
                if stop {
                    flag.store(true, Ordering::Relaxed);
                }
                stop
            })));
            let context = Context::full(&runtime).map_err(|_| "context_setup".to_string())?;
            let out = context.with(|ctx| -> Result<T, String> {
                for (what, source) in [("lib", lib.as_str()), ("globals", "Object.assign(globalThis, lib);"), ("core", core.as_str())] {
                    if ctx.eval::<Value, _>(source).is_err() {
                        return Err(format!("{what}_failed: {}", exception_text(&ctx)));
                    }
                }
                job(&ctx)
            });
            if expired.load(Ordering::Relaxed) {
                return Err("deadline_exceeded".to_string());
            }
            out
        })
        .map_err(|_| "solver_thread_spawn_failed".to_string())?
        .join()
        .unwrap_or_else(|_| Err("solver_thread_panicked".to_string()))
}

/// Parses and rewrites a player once (about a minute in a debug build). The returned script is
/// what `solve_prepared` needs; it depends only on the player revision and can be cached.
pub fn prepare_player(solver_dir: &std::path::Path, player: &str) -> Result<String, String> {
    let player = player.to_string();
    with_solver(solver_dir, move |ctx| {
        let harness: Function = ctx
            .eval("(function(p){const r=jsc({type:'player',player:p,requests:[],output_preprocessed:true});return r.preprocessed_player;})")
            .map_err(|_| format!("harness_failed: {}", exception_text(ctx)))?;
        harness
            .call::<_, String>((player.as_str(),))
            .map_err(|_| format!("prepare_threw: {}", exception_text(ctx)))
    })
}

/// Answers from a prepared script. Returns `(n answers, sig answers)` as `(input, output)` pairs.
#[allow(clippy::type_complexity)]
pub fn solve_prepared(
    solver_dir: &std::path::Path,
    prepared: &str,
    n_challenges: &[String],
    sig_challenges: &[String],
) -> Result<(Vec<(String, String)>, Vec<(String, String)>), String> {
    if n_challenges.is_empty() && sig_challenges.is_empty() {
        return Ok((vec![], vec![]));
    }
    if !n_challenges.iter().chain(sig_challenges).all(|c| challenge_ok(c)) {
        return Err("invalid_challenge".to_string());
    }
    let list = |v: &[String]| format!("[{}]", v.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(","));
    let (ns, sigs) = (list(n_challenges), list(sig_challenges));
    let prepared = prepared.to_string();
    let text = with_solver(solver_dir, move |ctx| {
        let harness: Function = ctx
            .eval(
                "(function(pp, ns, sigs){\
                   const reqs=[]; const kinds=[];\
                   const n=JSON.parse(ns), s=JSON.parse(sigs);\
                   if(n.length){reqs.push({type:'n',challenges:n});kinds.push('n');}\
                   if(s.length){reqs.push({type:'sig',challenges:s});kinds.push('sig');}\
                   const r=jsc({type:'preprocessed',preprocessed_player:pp,requests:reqs});\
                   const lines=[];\
                   r.responses.forEach((resp,i)=>{\
                     if(resp.type!=='result'){lines.push('ERR\\t'+kinds[i]+'\\t'+String(resp.error).split('\\n')[0]);return;}\
                     for(const [k,v] of Object.entries(resp.data)) lines.push('OK\\t'+kinds[i]+'\\t'+k+'\\t'+v);\
                   });\
                   return lines.join('\\n');\
                 })",
            )
            .map_err(|_| format!("harness_failed: {}", exception_text(ctx)))?;
        harness
            .call::<_, String>((prepared.as_str(), ns.as_str(), sigs.as_str()))
            .map_err(|_| format!("solver_threw: {}", exception_text(ctx)))
    })?;
    let (mut n_out, mut sig_out) = (vec![], vec![]);
    for line in text.lines() {
        let parts: Vec<&str> = line.splitn(4, '\t').collect();
        match parts.as_slice() {
            ["ERR", kind, message] => return Err(format!("solver_error_{kind}: {}", clean(message))),
            ["OK", kind, input, output] => {
                if output.is_empty() || output.len() > 512 || !challenge_ok(output) {
                    return Err("solver_output_rejected".to_string());
                }
                let pair = ((*input).to_string(), (*output).to_string());
                if *kind == "n" { n_out.push(pair) } else { sig_out.push(pair) }
            }
            _ => {}
        }
    }
    Ok((n_out, sig_out))
}

#[cfg(test)]
mod prepared_tests {
    use super::*;

    #[test]
    fn challenge_filter_rejects_quotes_backslashes_and_control_characters() {
        assert!(challenge_ok("aB3-_%3D=.~+/"));
        for bad in ["", "a\"b", "a\\b", "a b", "a\nb", "a;b", "é"] {
            assert!(!challenge_ok(bad), "{bad:?}");
        }
        assert!(!challenge_ok(&"a".repeat(513)));
    }

    /// Real solver + tiny synthetic player (n only): the two-stage path must answer n and report
    /// a missing sig function as an error code, not as a harness failure. Skipped without the solver.
    #[test]
    fn prepared_two_stage_answers_n_on_a_synthetic_player() {
        let Some(local) = std::env::var_os("LOCALAPPDATA") else { return };
        let dir = std::path::PathBuf::from(local).join("VibeDeck").join("ejs-solver");
        if !dir.join("core.min.js").exists() {
            return;
        }
        let player = "var _yt_player={};(function(g){var window=this;var U=class{constructor(a){this.a=a;this.q={}}set(k,v){this.q[k]=v}get(k){return this.q[k]||null}mat(){if(this.q.n){var e=this.q.n.split(\"\");e.reverse();e.splice(0,2);this.q.n=e.join(\"\")}}};var F=function(a,b=\"\",c=\"\"){a=new U(a);a.set(\"alr\",\"yes\");return a};g.F=F;})(_yt_player);";
        let prepared = prepare_player(&dir, player).unwrap();
        let (n, sig) = solve_prepared(&dir, &prepared, &["abcdefgh12345678".to_string()], &[]).unwrap();
        // The synthetic player reverses `n` and drops two characters.
        assert_eq!(n, vec![("abcdefgh12345678".to_string(), "654321hgfedcba".to_string())]);
        assert!(sig.is_empty());
    }

    #[test]
    fn solve_with_no_challenges_needs_no_solver() {
        let dir = std::path::Path::new("definitely-missing-dir");
        assert_eq!(solve_prepared(dir, "", &[], &[]), Ok((vec![], vec![])));
    }

    #[test]
    fn solve_refuses_bad_challenges_before_touching_the_solver() {
        let dir = std::path::Path::new("definitely-missing-dir");
        assert_eq!(solve_prepared(dir, "", &["a\"b".to_string()], &[]), Err("invalid_challenge".to_string()));
    }
}
