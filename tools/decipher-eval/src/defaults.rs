//! Parameter prerequisite, NOT effect certification of function bodies.
use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPatternKind, Expression, FormalParameters};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::structure::{check_reconstruction_structure, StructureError};

#[derive(Debug, PartialEq, Eq)]
pub enum DefaultsError {
    Structure(StructureError),
    Parameters,
}

/// Checks ALL parameter lists, including nested/unselected functions. Only simple
/// bindings and primitive literal defaults are supported. No evaluation, coercion,
/// generated code, evaluator integration or claim about calls/body effects.
pub fn check_reconstruction_defaults(source: &str) -> Result<(), DefaultsError> {
    check_reconstruction_structure(source).map_err(DefaultsError::Structure)?;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    let mut checker = ParameterCheck { rejected: false };
    checker.visit_program(&parsed.program);
    if checker.rejected {
        Err(DefaultsError::Parameters)
    } else {
        Ok(())
    }
}

struct ParameterCheck {
    rejected: bool,
}

impl<'a> Visit<'a> for ParameterCheck {
    fn visit_formal_parameters(&mut self, parameters: &FormalParameters<'a>) {
        self.rejected |= parameters.rest.is_some();
        for parameter in &parameters.items {
            self.rejected |= match &parameter.pattern.kind {
                BindingPatternKind::BindingIdentifier(_) => false,
                BindingPatternKind::AssignmentPattern(pattern) => {
                    !matches!(pattern.left.kind, BindingPatternKind::BindingIdentifier(_))
                        || !matches!(
                            pattern.right,
                            Expression::StringLiteral(_)
                                | Expression::NumericLiteral(_)
                                | Expression::BooleanLiteral(_)
                                | Expression::NullLiteral(_)
                        )
                }
                _ => true,
            };
        }
        walk::walk_formal_parameters(self, parameters);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ENTRY: &str = "const f=function(u,sp='',s=''){const h={URL:function(v){return {set:function(a,b){return a+b}}}};u=new h.URL(u);u.set('alr','yes');return 'ok'};";

    #[test]
    fn accepts_primitive_defaults_without_running_bodies() {
        for parameters in ["x='a'", "x=1", "x=true", "x=null", "x"] {
            let source = format!("{ENTRY}const unused=function({parameters}){{throw 1}};");
            assert_eq!(check_reconstruction_defaults(&source), Ok(()));
        }
    }

    #[test]
    fn rejects_the_previously_observed_default_call() {
        let source = ENTRY.replace("sp=''", "sp=(function(){throw 'synthetic'})()");
        assert_eq!(check_reconstruction_structure(&source), Ok(()));
        assert_eq!(
            check_reconstruction_defaults(&source),
            Err(DefaultsError::Parameters)
        );
    }

    #[test]
    fn checks_nested_functions_arrows_and_methods() {
        for local in [
            "const nested=function(x=(()=>{throw 1})()){};",
            "const nested=(x=(()=>{throw 1})())=>x;",
            "const nested={method(x=(()=>{throw 1})()){return x}};",
        ] {
            let source = ENTRY.replace("return 'ok'", &format!("{local}return 'ok'"));
            assert_eq!(
                check_reconstruction_defaults(&source),
                Err(DefaultsError::Parameters)
            );
        }
    }

    #[test]
    fn rejects_destructuring_rest_and_nonliteral_defaults() {
        for parameters in [
            "{x}",
            "[x]",
            "...x",
            "x={}",
            "x=[]",
            "x=1+2",
            "x=(()=>1)",
            "{x}={}",
        ] {
            let source = format!("{ENTRY}const unused=function({parameters}){{}};");
            assert_eq!(
                check_reconstruction_defaults(&source),
                Err(DefaultsError::Parameters)
            );
        }
    }
}
