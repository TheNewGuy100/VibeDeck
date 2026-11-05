//! Offline executor experiment, not a decipher or a security sandbox.
//! No app dependency, IPC, HTTP, module loader, host bindings or provider fixtures.

pub mod bootstrap;
pub mod bootstrap_context;
pub mod bootstrap_html;
pub mod defaults;
pub mod ejs_solver;
pub mod extraction;
pub mod native_transform;
pub mod player_cache;
pub mod reconstruction;
pub mod resolution;
pub mod script_diagnostic;
pub mod structure;

use rquickjs::{Context, Function, Runtime, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

const SOURCE_LIMIT: usize = 256 * 1024;
const OUTPUT_LIMIT: usize = 16 * 1024;
const MEMORY_LIMIT: usize = 8 * 1024 * 1024;
const STACK_LIMIT: usize = 128 * 1024;
const DEADLINE: Duration = Duration::from_millis(100);

/// Fixed categories only; never include source, exception text or output values.
#[derive(Debug, PartialEq, Eq)]
pub enum EvalError {
    SourceLimit,
    InputLimit,
    Setup,
    Execution,
    Interrupted,
    OutputType,
    OutputLimit,
    OutputRejected,
}

/// Evaluates synthetic source in a fresh runtime. No string coercion or job pump.
/// Interrupts are cooperative, not a hard deadline for parsing/native operations.
pub fn evaluate_synthetic(source: &str) -> Result<String, EvalError> {
    evaluate_bounded(source, None)
}

/// Synthetic function-valued source only. Arguments enter through the engine's
/// string conversion, NEVER through JavaScript source interpolation. No defaults
/// are omitted: the three string arguments are always supplied explicitly.
/// Does not extract/approve remote code or certify a transformation algorithm.
pub fn invoke_synthetic(source: &str, arguments: [&str; 3]) -> Result<String, EvalError> {
    if arguments.iter().any(|value| value.len() > OUTPUT_LIMIT) {
        return Err(EvalError::InputLimit);
    }
    evaluate_bounded(source, Some(arguments))
}

fn evaluate_bounded(source: &str, arguments: Option<[&str; 3]>) -> Result<String, EvalError> {
    if source.len() > SOURCE_LIMIT {
        return Err(EvalError::SourceLimit);
    }
    let runtime = Runtime::new().map_err(|_| EvalError::Setup)?;
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
    let context = Context::full(&runtime).map_err(|_| EvalError::Setup)?;
    let result = context.with(|ctx| {
        let value: Value = match arguments {
            Some([first, second, third]) => {
                let function: Function = ctx.eval(source).map_err(|_| EvalError::Execution)?;
                function
                    .call((first, second, third))
                    .map_err(|_| EvalError::Execution)?
            }
            None => ctx.eval(source).map_err(|_| EvalError::Execution)?,
        };
        let string = value.into_string().ok_or(EvalError::OutputType)?;
        // Conversion storage stays engine-owned/heap-limited. Check byte length
        // before copying to a Rust allocation; never invoke user toString/getters.
        let bytes = string.to_cstring().map_err(|_| EvalError::Execution)?;
        if bytes.len() > OUTPUT_LIMIT {
            return Err(EvalError::OutputLimit);
        }
        let text = bytes.as_str();
        if text.is_empty() || text.starts_with("enhanced_except_") {
            return Err(EvalError::OutputRejected);
        }
        Ok(text.to_owned())
    });
    if expired.load(Ordering::Relaxed) || Instant::now() >= deadline {
        Err(EvalError::Interrupted)
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invoke_arguments_are_data() {
        for input in [
            "quotes'\";throw 1;//",
            "line\n\0break",
            "á🎵",
            "\\u2028",
            "",
        ] {
            assert_eq!(
                invoke_synthetic(
                    "(function(a,b,c){return a+'|'+b+'|'+c})",
                    [input, "second", "third"]
                ),
                Ok(format!("{input}|second|third"))
            );
        }
    }

    #[test]
    fn typed_invocation_is_bounded() {
        assert_eq!(
            invoke_synthetic(
                "(function(a){return a})",
                [&"x".repeat(OUTPUT_LIMIT + 1), "", ""]
            ),
            Err(EvalError::InputLimit)
        );
        assert_eq!(
            invoke_synthetic(
                "(function(a){return a})",
                [&"x".repeat(OUTPUT_LIMIT), "", ""]
            ),
            Ok("x".repeat(OUTPUT_LIMIT))
        );
        assert_eq!(
            invoke_synthetic("(function(){while(true){}})", ["", "", ""]),
            Err(EvalError::Interrupted)
        );
        assert_eq!(
            invoke_synthetic("(function(){return {toString(){throw 1}}})", ["", "", ""]),
            Err(EvalError::OutputType)
        );
        assert_eq!(
            invoke_synthetic("(function(){return Promise.resolve('x')})", ["", "", ""]),
            Err(EvalError::OutputType)
        );
        assert_eq!(
            invoke_synthetic("(function(){return 'enhanced_except_test'})", ["", "", ""]),
            Err(EvalError::OutputRejected)
        );
        assert_eq!(
            invoke_synthetic("(function(){return 'ok'})", ["", "", ""]),
            Ok("ok".into())
        );
    }

    #[test]
    fn deterministic_and_unicode() {
        assert_eq!(
            evaluate_synthetic("'abc'.split('').reverse().join('')"),
            Ok("cba".into())
        );
        assert_eq!(evaluate_synthetic("'á🎵'"), Ok("á🎵".into()));
    }

    #[test]
    fn no_host_bindings() {
        assert_eq!(evaluate_synthetic("[typeof fetch,typeof process,typeof require,typeof std,typeof os,typeof document,typeof window,typeof setTimeout].join(',')"), Ok(["undefined"; 8].join(",")));
    }

    #[test]
    fn infinite_loop_is_interrupted() {
        assert_eq!(
            evaluate_synthetic("while(true){}"),
            Err(EvalError::Interrupted)
        );
    }

    #[test]
    fn catch_cannot_resume_after_interrupt() {
        assert_eq!(
            evaluate_synthetic("try { while(true){} } catch (_) { 'escaped' }"),
            Err(EvalError::Interrupted)
        );
    }

    #[test]
    fn source_is_bounded_before_evaluation() {
        assert_eq!(
            evaluate_synthetic(&" ".repeat(SOURCE_LIMIT + 1)),
            Err(EvalError::SourceLimit)
        );
    }

    #[test]
    fn output_byte_limit_and_boundary() {
        assert_eq!(
            evaluate_synthetic("'x'.repeat(16384)").unwrap().len(),
            OUTPUT_LIMIT
        );
        assert_eq!(
            evaluate_synthetic("'x'.repeat(16385)"),
            Err(EvalError::OutputLimit)
        );
        assert_eq!(
            evaluate_synthetic("'á'.repeat(8193)"),
            Err(EvalError::OutputLimit)
        );
    }

    #[test]
    fn invalid_outputs_are_not_coerced() {
        for script in [
            "42",
            "null",
            "undefined",
            "({toString(){while(true){}}})",
            "Promise.resolve('x')",
        ] {
            assert_eq!(evaluate_synthetic(script), Err(EvalError::OutputType));
        }
        for script in ["''", "'enhanced_except_synthetic'"] {
            assert_eq!(evaluate_synthetic(script), Err(EvalError::OutputRejected));
        }
    }

    #[test]
    fn exceptions_and_syntax_errors_are_sanitized() {
        for script in ["throw new Error('private-synthetic-text')", "let = ;"] {
            assert_eq!(evaluate_synthetic(script), Err(EvalError::Execution));
        }
    }

    #[test]
    fn allocation_growth_fails_without_deadline() {
        assert_eq!(
            evaluate_synthetic("let a=[];while(true)a.push(new ArrayBuffer(1024*1024));"),
            Err(EvalError::Execution)
        );
    }

    #[test]
    fn recursion_fails_without_deadline() {
        assert_eq!(
            evaluate_synthetic("function f(){return f()}f()"),
            Err(EvalError::Execution)
        );
    }

    #[test]
    fn globals_do_not_cross_calls() {
        assert_eq!(
            evaluate_synthetic("globalThis.marker='synthetic';'ok'"),
            Ok("ok".into())
        );
        assert_eq!(evaluate_synthetic("typeof marker"), Ok("undefined".into()));
        assert_eq!(evaluate_synthetic("'fresh'"), Ok("fresh".into()));
    }
}
