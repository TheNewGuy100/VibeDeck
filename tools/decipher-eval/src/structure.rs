//! First structural prerequisite only. Success is NOT execution eligibility.
//! No code generation, evaluator integration or container execution.
use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPatternKind, Expression, Statement, VariableDeclarationKind};
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::reconstruction::{plan_reconstruction, PlanError};

#[derive(Debug, PartialEq, Eq)]
pub enum StructureError {
    Plan(PlanError),
    Container,
    Declaration,
    Initializer,
}

/// Synthetic input only. Requires the entire program to contain only direct const
/// declarations (plus directives). This deliberately rejects otherwise unrelated
/// statements instead of silently removing their effects. Function bodies/defaults
/// remain unclassified; passing this check must never authorize their execution.
/// Source/parser/semantic limits are those of the existing experimental planner.
pub fn check_reconstruction_structure(source: &str) -> Result<(), StructureError> {
    let _plan = plan_reconstruction(source).map_err(StructureError::Plan)?;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    for statement in &parsed.program.body {
        let Statement::VariableDeclaration(declaration) = statement else {
            return Err(StructureError::Container);
        };
        if declaration.kind != VariableDeclarationKind::Const {
            return Err(StructureError::Declaration);
        }
        for declarator in &declaration.declarations {
            if !matches!(declarator.id.kind, BindingPatternKind::BindingIdentifier(_)) {
                return Err(StructureError::Declaration);
            }
            if !matches!(
                declarator.init,
                Some(
                    Expression::FunctionExpression(_)
                        | Expression::StringLiteral(_)
                        | Expression::NumericLiteral(_)
                        | Expression::BooleanLiteral(_)
                        | Expression::NullLiteral(_)
                )
            ) {
                return Err(StructureError::Initializer);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const ENTRY: &str =
        "const f=function(u,sp='',s=''){u=new h.URL(u);u.set('alr','yes');return h};";
    // Local stub, NOT browser URL semantics or a provider decipher algorithm.
    // The constructor explicitly returns an object; no this/global/host binding.
    const FUNCTIONAL_SOURCE: &str = "const f=function(u,sp='',s=''){
        const h={URL:function(value){return {
            value:value,
            marker:'',
            set:function(name,value){return name+':'+value}
        }}};
        u=new h.URL(u);
        u.set('alr','yes');
        return u.value.split('').reverse().join('')+':'+sp+':'+s;
    };";

    #[test]
    fn functional_synthetic_candidate_has_observable_output() {
        assert_eq!(check_reconstruction_structure(FUNCTIONAL_SOURCE), Ok(()));
        // Explicit fixed synthetic invocation. Never interpolate remote input.
        assert_eq!(
            crate::evaluate_synthetic(&format!("{FUNCTIONAL_SOURCE}f('abc','sig','test')")),
            Ok("cba:sig:test".into())
        );
        assert_eq!(
            crate::evaluate_synthetic(&format!("{FUNCTIONAL_SOURCE}f('abc')")),
            Ok("cba::".into())
        );
    }

    #[test]
    fn structural_success_does_not_certify_default_effects() {
        let source = FUNCTIONAL_SOURCE.replace("sp=''", "sp=(function(){throw 'synthetic'})()");
        assert_eq!(check_reconstruction_structure(&source), Ok(()));
        assert_eq!(
            crate::evaluate_synthetic(&format!("{source}f('abc')")),
            Err(crate::EvalError::Execution)
        );
        // Supplying the argument skips the default; both paths need classification.
        assert_eq!(
            crate::evaluate_synthetic(&format!("{source}f('abc','sig','test')")),
            Ok("cba:sig:test".into())
        );
    }

    #[test]
    fn accepts_direct_const_structure_without_execution() {
        let source = format!("const h=function(){{throw 'not-executed'}};{ENTRY}");
        assert_eq!(check_reconstruction_structure(&source), Ok(()));
        assert_eq!(
            check_reconstruction_structure(&format!("'use strict';{source}")),
            Ok(())
        );
    }

    #[test]
    fn rejects_conditional_var_even_with_outer_binding_scope() {
        let source = format!("if(false){{var h=function(){{}}}}{ENTRY}");
        assert_eq!(
            check_reconstruction_structure(&source),
            Err(StructureError::Container)
        );
    }

    #[test]
    fn rejects_blocks_and_iife_containers() {
        for source in [
            format!("{{const h=function(){{}};{ENTRY}}}"),
            format!("(function(){{const h=function(){{}};{ENTRY}}})();"),
        ] {
            assert_eq!(
                check_reconstruction_structure(&source),
                Err(StructureError::Container)
            );
        }
    }

    #[test]
    fn rejects_var_and_let_entry_declarations() {
        for kind in ["var", "let"] {
            let source = format!(
                "const h=function(){{}};{}",
                ENTRY.replacen("const", kind, 1)
            );
            assert_eq!(
                check_reconstruction_structure(&source),
                Err(StructureError::Declaration)
            );
        }
    }

    #[test]
    fn rejects_unrelated_effects_property_mutation_and_escape() {
        for effect in ["throw 1;", "h.URL=function(){};", "consume(h);", "f=0;"] {
            let source = format!("const h=function(){{}};{ENTRY}{effect}");
            assert_eq!(
                check_reconstruction_structure(&source),
                Err(StructureError::Container)
            );
        }
    }

    #[test]
    fn rejects_unselected_initializer_effects_and_destructuring() {
        for (extra, expected) in [
            ("const unused=danger();", StructureError::Initializer),
            ("const unused={};", StructureError::Initializer),
            ("const {unused}=value;", StructureError::Declaration),
        ] {
            let source = format!("const h=function(){{}};{ENTRY}{extra}");
            assert_eq!(check_reconstruction_structure(&source), Err(expected));
        }
    }
}
