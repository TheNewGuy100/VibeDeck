//! Conservative lexical dependency inventory, NOT executable code generation.
//! Original spans retain their scopes; never concatenate them into one scope.
use oxc_allocator::Allocator;
use oxc_ast::{ast::Expression, AstKind};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType, Span};

use crate::extraction::{recognize_candidate, ExtractionError};

const DEPENDENCY_LIMIT: usize = 64;

#[derive(Debug, PartialEq, Eq)]
pub enum ResolutionError {
    Extraction(ExtractionError),
    Semantic,
    Unresolved,
    Unsupported,
    Mutated,
    Cycle,
    DependencyLimit,
}

/// No Debug/Serialize and no evaluator integration. Includes transitive dependencies
/// in dependency-first order, with their original lexical scope left intact.
pub struct Resolution<'s> {
    source: &'s str,
    dependencies: Vec<Span>,
}

impl<'s> Resolution<'s> {
    pub fn dependency_sources(&self) -> impl Iterator<Item = &'s str> + '_ {
        self.dependencies
            .iter()
            .map(|span| &self.source[span.start as usize..span.end as usize])
    }
}

/// No global allowlist yet: all unbound names fail closed. Only external simple
/// variable bindings initialized by functions or primitive literals are supported.
/// Calls, objects/getters, aliases, destructuring and captured parameters fail.
/// This inventories bindings, not effects, initialization timing or URL semantics.
pub fn resolve_candidate(source: &str) -> Result<Resolution<'_>, ResolutionError> {
    let candidate = recognize_candidate(source).map_err(ResolutionError::Extraction)?;
    let start = candidate.function_source().as_ptr() as usize - source.as_ptr() as usize;
    let root = Span::new(
        start as u32,
        (start + candidate.function_source().len()) as u32,
    );
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    let built = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .build(&parsed.program);
    if !built.errors.is_empty() {
        return Err(ResolutionError::Semantic);
    }
    let semantic = built.semantic;
    let scoping = semantic.scoping();
    let mut output = Vec::new();
    let mut active = Vec::new();
    let mut done = Vec::new();
    // Explicit DFS keeps dependency depth off the Rust call stack.
    let mut work = vec![(root, false)];
    while let Some((region, exit)) = work.pop() {
        if exit {
            active.pop();
            done.push(region);
            if region != root {
                output.push(region);
            }
            continue;
        }
        if done.contains(&region) {
            continue;
        }
        if active.contains(&region) {
            return Err(ResolutionError::Cycle);
        }
        if active.len() + done.len() >= DEPENDENCY_LIMIT {
            return Err(ResolutionError::DependencyLimit);
        }
        active.push(region);
        work.push((region, true));
        let mut dependencies = Vec::new();
        for node in semantic.nodes().iter() {
            let span = node.kind().span();
            if span.start < region.start || span.end > region.end {
                continue;
            }
            match node.kind() {
                AstKind::ThisExpression(_)
                | AstKind::Super(_)
                | AstKind::WithStatement(_)
                | AstKind::ImportExpression(_)
                | AstKind::MetaProperty(_) => return Err(ResolutionError::Unsupported),
                AstKind::IdentifierReference(id) => {
                    // Reject eval even when locally bound; dynamic scopes are unsupported.
                    if id.name == "eval" {
                        return Err(ResolutionError::Unsupported);
                    }
                    let reference = scoping
                        .get_reference(id.reference_id.get().ok_or(ResolutionError::Semantic)?);
                    let symbol = reference.symbol_id().ok_or(ResolutionError::Unresolved)?;
                    let binding = scoping.symbol_span(symbol);
                    if binding.start >= region.start && binding.end <= region.end {
                        continue;
                    }
                    if !scoping.symbol_redeclarations(symbol).is_empty() {
                        return Err(ResolutionError::Unsupported);
                    }
                    if semantic.symbol_references(symbol).any(|r| r.is_write()) {
                        return Err(ResolutionError::Mutated);
                    }
                    let AstKind::VariableDeclarator(declaration) =
                        semantic.symbol_declaration(symbol).kind()
                    else {
                        return Err(ResolutionError::Unsupported);
                    };
                    if !matches!(
                        declaration.id.kind,
                        oxc_ast::ast::BindingPatternKind::BindingIdentifier(_)
                    ) {
                        return Err(ResolutionError::Unsupported);
                    }
                    if !matches!(
                        declaration.init,
                        Some(
                            Expression::FunctionExpression(_)
                                | Expression::StringLiteral(_)
                                | Expression::NumericLiteral(_)
                                | Expression::BooleanLiteral(_)
                                | Expression::NullLiteral(_)
                        )
                    ) {
                        return Err(ResolutionError::Unsupported);
                    }
                    let dependency = declaration.span;
                    if !dependencies.contains(&dependency) {
                        dependencies.push(dependency);
                    }
                }
                _ => {}
            }
        }
        for dependency in dependencies.into_iter().rev() {
            work.push((dependency, false));
        }
    }
    Ok(Resolution {
        source,
        dependencies: output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn script(prefix: &str, tail: &str) -> String {
        format!(
            "{prefix};var f=function(u,sp='',s=''){{u=new h.URL(u);u.set('alr','yes');{tail}}};"
        )
    }
    fn error(source: &str) -> ResolutionError {
        resolve_candidate(source).err().expect("rejected")
    }

    #[test]
    fn transitive_dependencies_are_ordered_and_deduplicated() {
        let source = script("const k=2;const h=function(){return k}", "h();return h");
        let result = resolve_candidate(&source).ok().expect("resolved");
        assert_eq!(
            result.dependency_sources().collect::<Vec<_>>(),
            ["k=2", "h=function(){return k}"]
        );
    }
    #[test]
    fn shadowed_local_is_not_external() {
        let source = script(
            "const h=function(){return 1};const hidden=missing",
            "{let hidden=3;h(hidden)}",
        );
        assert_eq!(
            resolve_candidate(&source)
                .ok()
                .expect("resolved")
                .dependency_sources()
                .count(),
            1
        );
    }
    #[test]
    fn nested_scope_keeps_original_binding_identity() {
        let source = format!(
            "const h=missing;(function(){{{}}})();",
            script("const h=function(){return 1}", "return h")
        );
        assert!(resolve_candidate(&source).is_ok());
    }
    #[test]
    fn unresolved_transitive_reference_is_rejected() {
        assert_eq!(
            error(&script("const h=function(){return missing}", "")),
            ResolutionError::Unresolved
        );
    }
    #[test]
    fn captured_container_parameter_is_rejected() {
        let source = format!("(function(h){{{}}})(null)", script("", ""));
        assert_eq!(error(&source), ResolutionError::Unsupported);
    }
    #[test]
    fn initializer_effects_and_aliases_are_rejected_without_execution() {
        for init in [
            "danger()",
            "{get URL(){throw 1}}",
            "other",
            "(()=>{throw 1})()",
        ] {
            assert_eq!(
                error(&script(&format!("const h={init}"), "")),
                ResolutionError::Unsupported
            );
        }
    }
    #[test]
    fn writes_even_outside_candidate_are_rejected() {
        assert_eq!(
            error(&script("let h=function(){};h=0", "")),
            ResolutionError::Mutated
        );
    }
    #[test]
    fn cycles_are_rejected() {
        assert_eq!(
            error(&script(
                "const h=function(){return j};const j=function(){return h}",
                ""
            )),
            ResolutionError::Cycle
        );
    }
    #[test]
    fn dynamic_environment_is_rejected() {
        for tail in ["return this", "eval('x')", "return import('x')"] {
            assert_eq!(
                error(&script("const h=function(){}", tail)),
                ResolutionError::Unsupported
            );
        }
    }

    #[test]
    fn dependency_budget_is_bounded() {
        let mut prefix = "const h=function(){return d0};".to_owned();
        for i in 0..70 {
            prefix.push_str(&format!("const d{i}=function(){{return d{}}};", i + 1));
        }
        prefix.push_str("const d70=1");
        assert_eq!(
            error(&script(&prefix, "")),
            ResolutionError::DependencyLimit
        );
    }

    #[test]
    fn computed_properties_and_defaults_have_real_references() {
        assert_eq!(
            error(&script("const h=function(){}", "return h[unknown]")),
            ResolutionError::Unresolved
        );
        let source = script("const h=function(){}", "").replace("sp=''", "sp=unknown");
        assert_eq!(error(&source), ResolutionError::Unresolved);
    }
}
