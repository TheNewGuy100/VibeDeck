//! Structural recognition only. A candidate is NOT an executable program:
//! dependency/scope resolution and binding to a player revision are not implemented.
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, AssignmentTarget, BindingPatternKind, Expression, Statement, VariableDeclarator,
};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_span::SourceType;

const SOURCE_LIMIT: usize = 256 * 1024;
const CANDIDATE_LIMIT: usize = 64 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum ExtractionError {
    SourceLimit,
    Parse,
    Missing,
    Ambiguous,
    CandidateLimit,
}

/// Intentionally no Debug/Serialize: source may eventually contain provider data.
/// Not accepted by the evaluator as a transform or connected to any HTTP path.
pub struct Candidate<'s> {
    function_source: &'s str,
}

impl<'s> Candidate<'s> {
    /// Raw code for offline analysis only; never log or treat as resolved code.
    pub fn function_source(&self) -> &'s str {
        self.function_source
    }
}

/// Parses bounded JavaScript script input, never executes it. Any parser error
/// rejects the entire source, even if a matching function was recovered.
/// Source length is bounded; parser/visitor heap and CPU are NOT hard-limited.
pub fn recognize_candidate(source: &str) -> Result<Candidate<'_>, ExtractionError> {
    if source.len() > SOURCE_LIMIT {
        return Err(ExtractionError::SourceLimit);
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(ExtractionError::Parse);
    }
    let mut finder = Finder {
        matches: 0,
        span: None,
    };
    finder.visit_program(&parsed.program);
    match finder.matches {
        0 => Err(ExtractionError::Missing),
        1 => {
            let span = finder.span.ok_or(ExtractionError::Missing)?;
            let function_source = source.get(span).ok_or(ExtractionError::Parse)?;
            if function_source.len() > CANDIDATE_LIMIT {
                return Err(ExtractionError::CandidateLimit);
            }
            Ok(Candidate { function_source })
        }
        _ => Err(ExtractionError::Ambiguous),
    }
}

struct Finder {
    matches: usize,
    span: Option<std::ops::Range<usize>>,
}

impl<'a> Visit<'a> for Finder {
    fn visit_variable_declarator(&mut self, declaration: &VariableDeclarator<'a>) {
        if let Some(Expression::FunctionExpression(function)) = &declaration.init {
            let params = &function.params;
            let supported = matches!(
                declaration.id.kind,
                BindingPatternKind::BindingIdentifier(_)
            ) && !function.r#async
                && !function.generator
                && params.rest.is_none()
                && params.items.len() == 3;
            if supported {
                let first = &params.items[0].pattern.kind;
                let second = &params.items[1].pattern.kind;
                let third = &params.items[2].pattern.kind;
                if let (
                    BindingPatternKind::BindingIdentifier(url),
                    BindingPatternKind::AssignmentPattern(sig_name),
                    BindingPatternKind::AssignmentPattern(sig_value),
                ) = (first, second, third)
                {
                    if matches!(sig_name.left.kind, BindingPatternKind::BindingIdentifier(_))
                        && matches!(
                            sig_value.left.kind,
                            BindingPatternKind::BindingIdentifier(_)
                        )
                    {
                        if let Some(body) = &function.body {
                            let mut constructs_url = false;
                            let mut sets_alr = false;
                            for statement in &body.statements {
                                let Statement::ExpressionStatement(statement) = statement else {
                                    continue;
                                };
                                match &statement.expression {
                                    Expression::AssignmentExpression(assignment) => {
                                        if assignment.operator.as_str() == "=" {
                                            if let (
                                                AssignmentTarget::AssignmentTargetIdentifier(
                                                    target,
                                                ),
                                                Expression::NewExpression(new),
                                            ) = (&assignment.left, &assignment.right)
                                            {
                                                constructs_url |= target.name == url.name
                                                    && matches!(
                                                        new.callee,
                                                        Expression::StaticMemberExpression(_)
                                                            | Expression::ComputedMemberExpression(
                                                                _
                                                            )
                                                    );
                                            }
                                        }
                                    }
                                    Expression::CallExpression(call) => {
                                        if let Expression::StaticMemberExpression(member) =
                                            &call.callee
                                        {
                                            if member.property.name == "set"
                                                && !call.optional
                                                && call.arguments.len() == 2
                                            {
                                                if let (
                                                    Argument::StringLiteral(name),
                                                    Argument::StringLiteral(value),
                                                ) = (&call.arguments[0], &call.arguments[1])
                                                {
                                                    sets_alr |=
                                                        name.value == "alr" && value.value == "yes";
                                                }
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            if constructs_url && sets_alr {
                                self.matches = self.matches.saturating_add(1);
                                self.span =
                                    Some(function.span.start as usize..function.span.end as usize);
                            }
                        }
                    }
                }
            }
        }
        walk::walk_variable_declarator(self, declaration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FUNCTION: &str =
        "function(u,sp='',s=''){u=new helpers.URL(u);u.set('alr','yes');return u;}";

    #[test]
    fn recognizes_one_and_preserves_exact_source() {
        let source = format!("/*synthetic*/var transform={FUNCTION};");
        let candidate =
            recognize_candidate(&source).unwrap_or_else(|_| panic!("expected candidate"));
        assert_eq!(candidate.function_source(), FUNCTION);
    }

    #[test]
    fn finds_nested_declaration_without_executing_container() {
        let source =
            format!("(function(){{throw Error('must-not-execute');var f={FUNCTION};}})();");
        assert!(recognize_candidate(&source).is_ok());
    }

    #[test]
    fn ignores_markers_in_strings_comments_and_regex() {
        let source = format!("// var a={FUNCTION};\nconst text=\"u=new h.URL(u);u.set('alr','yes')\";const re=/alr/;");
        assert_eq!(
            recognize_candidate(&source).err(),
            Some(ExtractionError::Missing)
        );
    }

    #[test]
    fn rejects_absent_or_ambiguous() {
        assert_eq!(
            recognize_candidate("const a=1").err(),
            Some(ExtractionError::Missing)
        );
        assert_eq!(
            recognize_candidate(&format!("var a={FUNCTION},b={FUNCTION};")).err(),
            Some(ExtractionError::Ambiguous)
        );
    }

    #[test]
    fn rejects_partial_parse_even_with_candidate() {
        assert_eq!(
            recognize_candidate(&format!("var f={FUNCTION};let = ;")).err(),
            Some(ExtractionError::Parse)
        );
    }

    #[test]
    fn rejects_unsupported_parameter_and_function_shapes() {
        for function in [
            "function(u,sp,s){u=new h.URL(u);u.set('alr','yes')}",
            "function(u,sp='',s='',...rest){u=new h.URL(u);u.set('alr','yes')}",
            "async function(u,sp='',s=''){u=new h.URL(u);u.set('alr','yes')}",
            "function*(u,sp='',s=''){u=new h.URL(u);u.set('alr','yes')}",
            "(u,sp='',s='')=>{u=new h.URL(u);u.set('alr','yes')}",
        ] {
            assert_eq!(
                recognize_candidate(&format!("var f={function};")).err(),
                Some(ExtractionError::Missing)
            );
        }
    }

    #[test]
    fn requires_direct_statements_and_matching_assignment() {
        for body in [
            "other=new h.URL(u);u.set('alr','yes')",
            "u=new URL(u);u.set('alr','yes')",
            "u=new h.URL(u);u.set('alr','no')",
            "u=new h.URL(u);u.get('alr','yes')",
            "if(true){u=new h.URL(u);u.set('alr','yes')}",
        ] {
            assert_eq!(
                recognize_candidate(&format!("var f=function(u,sp='',s=''){{{body}}};")).err(),
                Some(ExtractionError::Missing)
            );
        }
    }

    #[test]
    fn bounds_source_and_candidate_bytes() {
        assert_eq!(
            recognize_candidate(&" ".repeat(SOURCE_LIMIT + 1)).err(),
            Some(ExtractionError::SourceLimit)
        );
        let body = format!(
            "var f=function(u,sp='',s=''){{u=new h.URL(u);u.set('alr','yes');/*{}*/}}",
            "x".repeat(CANDIDATE_LIMIT)
        );
        assert_eq!(
            recognize_candidate(&body).err(),
            Some(ExtractionError::CandidateLimit)
        );
    }

    #[test]
    fn unicode_prefix_does_not_shift_byte_span() {
        let source = format!("const label='á🎵'; var f={FUNCTION};");
        assert_eq!(
            recognize_candidate(&source)
                .unwrap_or_else(|_| panic!("expected candidate"))
                .function_source(),
            FUNCTION
        );
    }
}
