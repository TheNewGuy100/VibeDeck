//! Opt-in native trial. Parses a full script but emits only an n-transform and
//! lexical dependencies. Never evaluates the full player or logs source/values.
use oxc_allocator::Allocator;
use oxc_ast::{ast::*, AstKind};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType, Span};

pub struct TrialScript {
    pub timestamp: u32,
    pub transform: Result<String, &'static str>,
    pub timestamp_properties: usize,
    pub url_entries: usize,
    pub n_inputs: usize,
    pub transform_calls: usize,
    /// Fixed-order numeric counts: functions, >=3 parameters,
    /// default-pattern pair, direct alr call, direct constructor, both markers.
    pub recognition_counts: [usize; 6],
    /// Calls in selected entry: n argument; static/computed/identifier callee;
    /// n-get; n-get assignment/declarator; n-first/n-second argument.
    pub n_shape_counts: [usize; 9],
    /// Whole-script, entry-independent counts: get("n"); set("n",_); get(identifier);
    /// set(identifier,_); string literal "n"; set("n",x) where x is a call whose
    /// callee is computed member / identifier / static member.
    pub global_n_counts: [usize; 8],
    /// Diagnostic only (diagnose_native_script), via bindings initialised to the
    /// literal "n": declarations; get(key); set(key,_); set(key,call) with callee
    /// computed member / identifier / static member.
    pub alias_n_counts: [usize; 6],
    /// Diagnostic only: every string literal "n" classified by its AST parent.
    /// Call arg index 0 with callee identifier / static member / computed member;
    /// index 1 with the same three callees; index >=2 or other callee; new argument;
    /// object property value; object property key; binary operand; assignment right;
    /// array element; anything else.
    pub literal_n_contexts: [usize; 14],
}

fn literal_n_contexts(program: &Program<'_>) -> [usize; 14] {
    let built = SemanticBuilder::new().build(program);
    let semantic = built.semantic;
    let mut counts = [0usize; 14];
    for node in semantic.nodes().iter() {
        let AstKind::StringLiteral(lit) = node.kind() else {
            continue;
        };
        if lit.value != "n" {
            continue;
        }
        let parent = semantic.nodes().parent_node(node.id());
        let parent_kind = match parent.kind() {
            AstKind::Argument(_) => semantic.nodes().parent_kind(parent.id()),
            other => other,
        };
        let slot = match parent_kind {
            AstKind::CallExpression(c) => {
                let idx = c.arguments.iter().position(|a| a.span() == lit.span);
                let shape = match &c.callee {
                    Expression::Identifier(_) => Some(0),
                    Expression::StaticMemberExpression(_) => Some(1),
                    Expression::ComputedMemberExpression(_) => Some(2),
                    _ => None,
                };
                match (idx, shape) {
                    (Some(0), Some(s)) => s,
                    (Some(1), Some(s)) => 3 + s,
                    _ => 6,
                }
            }
            AstKind::NewExpression(_) => 7,
            AstKind::ObjectProperty(p) => {
                if p.key.span() == lit.span {
                    9
                } else {
                    8
                }
            }
            AstKind::BinaryExpression(_) => 10,
            AstKind::AssignmentExpression(_) => 11,
            AstKind::ArrayExpression(_) => 12,
            _ => 13,
        };
        counts[slot] += 1;
    }
    counts
}

fn alias_n_counts(program: &Program<'_>) -> [usize; 6] {
    let built = SemanticBuilder::new().build(program);
    let semantic = built.semantic;
    let scoping = semantic.scoping();
    let mut keys = Vec::new();
    for node in semantic.nodes().iter() {
        if let AstKind::VariableDeclarator(d) = node.kind() {
            if let (BindingPatternKind::BindingIdentifier(id), Some(Expression::StringLiteral(s))) =
                (&d.id.kind, &d.init)
            {
                if s.value == "n" {
                    if let Some(sym) = id.symbol_id.get() {
                        keys.push(sym);
                    }
                }
            }
        }
    }
    let mut counts = [0usize; 6];
    counts[0] = keys.len();
    let is_key = |arg: Option<&Argument<'_>>| match arg {
        Some(Argument::Identifier(id)) => id
            .reference_id
            .get()
            .and_then(|r| scoping.get_reference(r).symbol_id())
            .is_some_and(|s| keys.contains(&s)),
        _ => false,
    };
    for node in semantic.nodes().iter() {
        let AstKind::CallExpression(c) = node.kind() else {
            continue;
        };
        let Expression::StaticMemberExpression(m) = &c.callee else {
            continue;
        };
        if !is_key(c.arguments.first()) {
            continue;
        }
        match m.property.name.as_str() {
            "get" => counts[1] += 1,
            "set" => {
                counts[2] += 1;
                if let Some(Argument::CallExpression(v)) = c.arguments.get(1) {
                    match &v.callee {
                        Expression::ComputedMemberExpression(_) => counts[3] += 1,
                        Expression::Identifier(_) => counts[4] += 1,
                        Expression::StaticMemberExpression(_) => counts[5] += 1,
                        _ => (),
                    }
                }
            }
            _ => (),
        }
    }
    counts
}

fn n_get(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::CallExpression(c) if matches!(&c.callee, Expression::StaticMemberExpression(m) if m.property.name == "get") && matches!(c.arguments.first(), Some(Argument::StringLiteral(s)) if s.value == "n"))
}

pub fn inspect_native_script(source: &str) -> Result<TrialScript, &'static str> {
    inspect_inner(source, true)
}

/// One-line JSON of counts and fixed codes for a player script: never source, names or values.
/// On a parse failure it adds numeric input-shape hints to tell transport damage from a real
/// parse problem.
pub fn diagnostic_json(source: &str) -> String {
    let source = source.trim_start_matches('\u{feff}');
    match diagnose_native_script(source) {
        Ok(s) => format!(
            "{{\"scriptBytes\":{},\"timestampProperties\":{},\"urlEntries\":{},\"nInputs\":{},\"transformCalls\":{},\"transformOk\":{},\"recognitionCounts\":{:?},\"nShapeCounts\":{:?},\"globalNCounts\":{:?},\"aliasNCounts\":{:?},\"literalNContexts\":{:?}}}",
            source.len(),
            s.timestamp_properties,
            s.url_entries,
            s.n_inputs,
            s.transform_calls,
            s.transform.is_ok(),
            s.recognition_counts,
            s.n_shape_counts,
            s.global_n_counts,
            s.alias_n_counts,
            s.literal_n_contexts
        ),
        Err(code) => {
            let trimmed = source.trim();
            let first = trimmed.chars().next().map_or(0, |c| c as u32);
            let last = trimmed.chars().last().map_or(0, |c| c as u32);
            format!(
                "{{\"error\":\"{code}\",\"chars\":{},\"bytes\":{},\"nonAscii\":{},\"questionMarks\":{},\"replacementChars\":{},\"firstCharCode\":{first},\"lastCharCode\":{last}}}",
                source.chars().count(),
                source.len(),
                source.chars().filter(|c| !c.is_ascii()).count(),
                source.chars().filter(|c| *c == '?').count(),
                source.chars().filter(|c| *c == '\u{fffd}').count(),
            )
        }
    }
}

/// Offline diagnostic only: same counts, but an ambiguous signatureTimestamp does
/// not abort (timestamp is then the smallest literal, or 0). Never use for a request.
pub fn diagnose_native_script(source: &str) -> Result<TrialScript, &'static str> {
    inspect_inner(source, false)
}

fn inspect_inner(source: &str, strict_timestamp: bool) -> Result<TrialScript, &'static str> {
    if source.len() > 4 * 1024 * 1024 {
        return Err("trial_script_limit");
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err("trial_script_parse");
    }
    #[derive(Default)]
    struct Inventory {
        timestamps: Vec<u32>,
        properties: usize,
        entries: Vec<Span>,
        counts: [usize; 6],
        global_n: [usize; 8],
    }
    impl<'a> Visit<'a> for Inventory {
        fn visit_string_literal(&mut self, s: &StringLiteral<'a>) {
            if s.value == "n" {
                self.global_n[4] += 1;
            }
        }
        fn visit_call_expression(&mut self, c: &CallExpression<'a>) {
            if let Expression::StaticMemberExpression(m) = &c.callee {
                let first_n = matches!(c.arguments.first(), Some(Argument::StringLiteral(s)) if s.value == "n");
                let first_id = matches!(c.arguments.first(), Some(Argument::Identifier(_)));
                match m.property.name.as_str() {
                    "get" => {
                        self.global_n[0] += usize::from(first_n);
                        self.global_n[2] += usize::from(first_id);
                    }
                    "set" => {
                        self.global_n[1] += usize::from(first_n);
                        self.global_n[3] += usize::from(first_id);
                        if let (true, Some(Argument::CallExpression(v))) =
                            (first_n, c.arguments.get(1))
                        {
                            match &v.callee {
                                Expression::ComputedMemberExpression(_) => self.global_n[5] += 1,
                                Expression::Identifier(_) => self.global_n[6] += 1,
                                Expression::StaticMemberExpression(_) => self.global_n[7] += 1,
                                _ => (),
                            }
                        }
                    }
                    _ => (),
                }
            }
            walk::walk_call_expression(self, c);
        }
        fn visit_object_property(&mut self, p: &ObjectProperty<'a>) {
            if !p.computed
                && matches!(&p.key, PropertyKey::StaticIdentifier(k) if k.name == "signatureTimestamp")
            {
                self.properties += 1;
                if let Expression::NumericLiteral(n) = &p.value {
                    if n.value >= 1.0 && n.value <= u32::MAX as f64 && n.value.fract() == 0.0 {
                        self.timestamps.push(n.value as u32);
                    }
                }
            }
            walk::walk_object_property(self, p);
        }
        fn visit_function(&mut self, f: &Function<'a>, flags: oxc_semantic::ScopeFlags) {
            {
                self.counts[0] += 1;
                if f.params.items.len() >= 3 {
                    self.counts[1] += 1;
                }
                if let Some(body) = &f.body {
                    let alr = body.statements.iter().any(|s| matches!(s, Statement::ExpressionStatement(s) if matches!(&s.expression, Expression::CallExpression(c) if c.arguments.len()==2 && matches!(&c.arguments[0], Argument::StringLiteral(s) if s.value=="alr") && matches!(&c.arguments[1], Argument::StringLiteral(s) if s.value=="yes"))));
                    let ctor = body.statements.iter().any(|s| matches!(s, Statement::ExpressionStatement(s) if matches!(&s.expression, Expression::AssignmentExpression(a) if matches!(&a.right, Expression::NewExpression(_)))));
                    self.counts[3] += usize::from(alr);
                    self.counts[4] += usize::from(ctor);
                    self.counts[5] += usize::from(alr && ctor);
                }
                if f.params.items.len() >= 3
                    && matches!(
                        &f.params.items[1].pattern.kind,
                        BindingPatternKind::AssignmentPattern(_)
                    )
                    && matches!(
                        &f.params.items[2].pattern.kind,
                        BindingPatternKind::AssignmentPattern(_)
                    )
                {
                    self.counts[2] += 1;
                    if let Some(body) = &f.body {
                        let alr = body.statements.iter().any(|s| matches!(s, Statement::ExpressionStatement(s) if matches!(&s.expression, Expression::CallExpression(c) if c.arguments.len()==2 && matches!(&c.arguments[0], Argument::StringLiteral(s) if s.value=="alr") && matches!(&c.arguments[1], Argument::StringLiteral(s) if s.value=="yes"))));
                        let ctor = body.statements.iter().any(|s| matches!(s, Statement::ExpressionStatement(s) if matches!(&s.expression, Expression::AssignmentExpression(a) if matches!(&a.right, Expression::NewExpression(_)))));
                        if alr && ctor {
                            self.entries.push(f.span);
                        }
                    }
                }
            }
            walk::walk_function(self, f, flags);
        }
    }
    let mut inventory = Inventory::default();
    inventory.visit_program(&parsed.program);
    inventory.timestamps.sort_unstable();
    inventory.timestamps.dedup();
    if strict_timestamp && inventory.timestamps.len() != 1 {
        return Err("trial_timestamp_not_unique");
    }
    let mut result = TrialScript {
        timestamp: inventory.timestamps.first().copied().unwrap_or(0),
        transform: Err("trial_url_entry_not_unique"),
        timestamp_properties: inventory.properties,
        url_entries: inventory.entries.len(),
        n_inputs: 0,
        transform_calls: 0,
        recognition_counts: inventory.counts,
        n_shape_counts: [0; 9],
        global_n_counts: inventory.global_n,
        alias_n_counts: if strict_timestamp {
            [0; 6]
        } else {
            alias_n_counts(&parsed.program)
        },
        literal_n_contexts: if strict_timestamp {
            [0; 14]
        } else {
            literal_n_contexts(&parsed.program)
        },
    };
    if inventory.entries.len() != 1 {
        return Ok(result);
    }
    let entry = inventory.entries[0];
    let built = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .build(&parsed.program);
    if !built.errors.is_empty() {
        return Err("trial_script_semantic");
    }
    let semantic = built.semantic;
    let scoping = semantic.scoping();
    let inside = |span: Span| span.start >= entry.start && span.end <= entry.end;
    let mut inputs = Vec::new();
    for node in semantic.nodes().iter() {
        if !inside(node.kind().span()) {
            continue;
        }
        if let AstKind::CallExpression(c) = node.kind() {
            if c.arguments
                .iter()
                .any(|a| matches!(a, Argument::StringLiteral(s) if s.value == "n"))
            {
                result.n_shape_counts[0] += 1;
                match &c.callee {
                    Expression::StaticMemberExpression(_) => result.n_shape_counts[1] += 1,
                    Expression::ComputedMemberExpression(_) => result.n_shape_counts[2] += 1,
                    Expression::Identifier(_) => result.n_shape_counts[3] += 1,
                    _ => (),
                }
                if matches!(c.arguments.first(), Some(Argument::StringLiteral(s)) if s.value == "n")
                {
                    result.n_shape_counts[7] += 1;
                }
                if matches!(c.arguments.get(1), Some(Argument::StringLiteral(s)) if s.value == "n")
                {
                    result.n_shape_counts[8] += 1;
                }
            }
            if matches!(&c.callee, Expression::StaticMemberExpression(m) if m.property.name == "get")
                && matches!(c.arguments.first(), Some(Argument::StringLiteral(s)) if s.value == "n")
            {
                result.n_shape_counts[4] += 1;
            }
        }
        let symbol = match node.kind() {
            AstKind::AssignmentExpression(a) if n_get(&a.right) => {
                result.n_shape_counts[5] += 1;
                if let AssignmentTarget::AssignmentTargetIdentifier(id) = &a.left {
                    id.reference_id
                        .get()
                        .and_then(|r| scoping.get_reference(r).symbol_id())
                } else {
                    None
                }
            }
            AstKind::VariableDeclarator(d) if d.init.as_ref().is_some_and(n_get) => {
                result.n_shape_counts[6] += 1;
                if let BindingPatternKind::BindingIdentifier(id) = &d.id.kind {
                    id.symbol_id.get()
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(symbol) = symbol {
            if !inputs.contains(&symbol) {
                inputs.push(symbol);
            }
        }
    }
    result.n_inputs = inputs.len();
    let mut targets = Vec::new();
    for node in semantic.nodes().iter() {
        if !inside(node.kind().span()) {
            continue;
        }
        let AstKind::CallExpression(c) = node.kind() else {
            continue;
        };
        if c.arguments.len() != 1 {
            continue;
        }
        let Argument::Identifier(arg) = &c.arguments[0] else {
            continue;
        };
        let arg = arg
            .reference_id
            .get()
            .and_then(|r| scoping.get_reference(r).symbol_id());
        if !arg.is_some_and(|s| inputs.contains(&s)) {
            continue;
        }
        // `f(n)` or `table[<integer literal>](n)`; the latter is how obfuscated
        // players index an array of functions.
        let (callee, index) = match &c.callee {
            Expression::Identifier(id) => (id, None),
            Expression::ComputedMemberExpression(m) => {
                let (Expression::Identifier(id), Expression::NumericLiteral(i)) =
                    (&m.object, &m.expression)
                else {
                    continue;
                };
                if i.value < 0.0 || i.value > 65535.0 || i.value.fract() != 0.0 {
                    continue;
                }
                (id, Some(i.value as u32))
            }
            _ => continue,
        };
        if let Some(symbol) = callee
            .reference_id
            .get()
            .and_then(|r| scoping.get_reference(r).symbol_id())
        {
            if !targets.contains(&(symbol, index)) {
                targets.push((symbol, index));
            }
        }
    }
    result.transform_calls = targets.len();
    result.transform = if targets.len() != 1 {
        Err("trial_n_call_not_unique")
    } else {
        build_native_transform(source, &semantic, targets[0].0, targets[0].1)
    };
    Ok(result)
}

fn initializer_supported(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_) | Expression::Identifier(_) | Expression::StringLiteral(_) | Expression::NumericLiteral(_) | Expression::BooleanLiteral(_) | Expression::NullLiteral(_) | Expression::RegExpLiteral(_) => true,
        Expression::ArrayExpression(a)=>a.elements.iter().all(|e|match e { ArrayExpressionElement::Elision(_)=>true, ArrayExpressionElement::SpreadElement(_)=>false,_=>e.as_expression().is_some_and(initializer_supported)}),
        Expression::ObjectExpression(o)=>o.properties.iter().all(|p| matches!(p,ObjectPropertyKind::ObjectProperty(p) if !p.computed && p.kind==PropertyKind::Init && initializer_supported(&p.value))),
        Expression::UnaryExpression(e)=>initializer_supported(&e.argument),
        Expression::BinaryExpression(e)=>initializer_supported(&e.left)&&initializer_supported(&e.right),
        _=>false
    }
}

fn build_native_transform(
    source: &str,
    semantic: &oxc_semantic::Semantic<'_>,
    target: oxc_semantic::SymbolId,
    index: Option<u32>,
) -> Result<String, &'static str> {
    let scoping = semantic.scoping();
    let target_scope = scoping.symbol_scope_id(target);
    let ancestors: Vec<_> = scoping.scope_ancestors(target_scope).collect();
    let mut work = vec![target];
    let mut done = Vec::new();
    let mut snippets = Vec::new();
    while let Some(symbol) = work.pop() {
        if done.contains(&symbol) {
            continue;
        }
        done.push(symbol);
        if done.len() > 128 {
            return Err("trial_dependency_limit");
        }
        if !ancestors.contains(&scoping.symbol_scope_id(symbol))
            || !scoping.symbol_redeclarations(symbol).is_empty()
        {
            return Err("trial_dependency_scope");
        }
        let node = semantic.symbol_declaration(symbol);
        let span = node.kind().span();
        let code = match node.kind() {
            AstKind::VariableDeclarator(d) => {
                if !matches!(&d.id.kind, BindingPatternKind::BindingIdentifier(_))
                    || !d.init.as_ref().is_some_and(initializer_supported)
                {
                    return Err("trial_initializer_unsupported");
                }
                // var-only declarations; do not move lexical const/let across scopes.
                let parent = semantic.nodes().parent_kind(node.id());
                if !matches!(parent,AstKind::VariableDeclaration(v) if v.kind.is_var()) {
                    return Err("trial_lexical_declaration_unsupported");
                }
                format!("var {};", &source[span.start as usize..span.end as usize])
            }
            AstKind::Function(f) if f.is_function_declaration() => {
                source[span.start as usize..span.end as usize].to_string()
            }
            _ => return Err("trial_dependency_declaration"),
        };
        for r in semantic.symbol_references(symbol) {
            let s = semantic.nodes().get_node(r.node_id()).kind().span();
            if r.is_write() && !(s.start >= span.start && s.end <= span.end) {
                return Err("trial_dependency_mutated");
            }
        }
        for n in semantic.nodes().iter() {
            let s = n.kind().span();
            if s.start < span.start || s.end > span.end {
                continue;
            }
            if let AstKind::IdentifierReference(id) = n.kind() {
                let reference =
                    scoping.get_reference(id.reference_id.get().ok_or("trial_reference_missing")?);
                if let Some(dep) = reference.symbol_id() {
                    let binding = scoping.symbol_span(dep);
                    if binding.start < span.start || binding.end > span.end {
                        work.push(dep);
                    }
                } else if !matches!(
                    id.name.as_str(),
                    "Math"
                        | "String"
                        | "Array"
                        | "Object"
                        | "Number"
                        | "Boolean"
                        | "RegExp"
                        | "Date"
                        | "JSON"
                        | "parseInt"
                        | "parseFloat"
                        | "isNaN"
                        | "NaN"
                        | "Infinity"
                        | "undefined"
                        | "decodeURIComponent"
                        | "encodeURIComponent"
                        | "Error"
                        | "TypeError"
                        | "Uint8Array"
                        | "Int8Array"
                        | "Uint32Array"
                        | "Int32Array"
                        | "ArrayBuffer"
                ) {
                    return Err("trial_unresolved_dependency");
                }
            }
        }
        snippets.push((scoping.symbol_scope_id(symbol), span.start, code));
    }
    snippets.sort_by_key(|(_, start, _)| *start);
    let mut result = match index {
        Some(i) => format!("return {}[{i}];", scoping.symbol_name(target)),
        None => format!("return {};", scoping.symbol_name(target)),
    };
    for scope in ancestors {
        let body = snippets
            .iter()
            .filter(|(s, _, _)| *s == scope)
            .map(|(_, _, s)| s.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        result = format!("return (function(){{\n{body}\n{result}\n}})();");
    }
    let result = format!("(function(){{{result}}})()");
    if result.len() > 256 * 1024 {
        return Err("trial_extracted_source_limit");
    }
    Ok(result)
}

/// Remote extracted fragment only; typed challenge input, fresh limited runtime.
/// This is not a security sandbox or a proof of semantic equivalence.
pub fn invoke_native_transform(source: &str, input: &str) -> Result<String, &'static str> {
    crate::invoke_synthetic(source, [input, "", ""]).map_err(|_| "trial_transform_execution_failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizes_assigned_entry_without_executing_assignment() {
        let entry = ENTRY.replace("var entry=", "entry=");
        let source = format!(
            "var config={{signatureTimestamp:123}};var trans=function(n){{return n}};{entry}"
        );
        let script = inspect_native_script(&source).unwrap();
        assert_eq!(script.url_entries, 1);
        assert!(script.transform.is_ok());
    }
    #[test]
    fn recognizes_entry_with_additional_parameters() {
        let entry = ENTRY.replace("sp='',s=''", "sp='',s='',extra=0");
        let source = format!(
            "var config={{signatureTimestamp:123}};var trans=function(n){{return n}};{entry}"
        );
        let script = inspect_native_script(&source).unwrap();
        assert_eq!(script.url_entries, 1);
        assert!(script.transform.is_ok());
    }
    const ENTRY:&str="var entry=function(u,sp='',s=''){u=new h.URL(u);u.set('alr','yes');var n=u.get('n');n=trans(n);u.set('n',n);return u};";
    #[test]
    fn extracts_only_closure_not_container_or_unrelated_setup() {
        let source=format!("throw 'must not run';(function(){{var k='!';var trans=function(n){{return n+k}};var config={{signatureTimestamp:123}};{ENTRY}}})();");
        let script = inspect_native_script(&source).unwrap();
        assert_eq!(script.timestamp, 123);
        assert_eq!(script.url_entries, 1);
        assert_eq!(script.n_inputs, 1);
        assert_eq!(script.transform_calls, 1);
        let fragment = script.transform.unwrap();
        assert!(!fragment.contains("must not run"));
        assert_eq!(
            invoke_native_transform(&fragment, "typed'input").unwrap(),
            "typed'input!"
        );
    }
    #[test]
    fn diagnostic_json_is_counts_only_and_marks_parse_failures() {
        let source = "var c={signatureTimestamp:1};function a(u){var n=u.get('SECRETKEY');}";
        let ok = diagnostic_json(&format!("\u{feff}{source}"));
        assert!(ok.starts_with("{\"scriptBytes\":"), "{ok}");
        assert!(ok.contains("\"literalNContexts\":[") && !ok.contains("SECRETKEY"));
        let bad = diagnostic_json("var a = ;");
        assert!(bad.contains("\"error\":\"trial_script_parse\"") && bad.contains("\"nonAscii\":0"));
        assert!(!bad.contains("var a"));
    }
    #[test]
    fn follows_computed_member_callee_into_function_table() {
        let entry = ENTRY.replace("trans(n)", "tbl[0](n)");
        let source = format!(
            "var config={{signatureTimestamp:5}};var k='!';var tbl=[function(n){{return n+k}}];{entry}"
        );
        let script = inspect_native_script(&source).unwrap();
        assert_eq!(script.transform_calls, 1);
        let fragment = script.transform.unwrap();
        assert_eq!(invoke_native_transform(&fragment, "x").unwrap(), "x!");
    }
    #[test]
    fn computed_callee_rejects_dynamic_or_non_integer_index() {
        for callee in ["tbl[i](n)", "tbl['a'](n)", "tbl[0.5](n)", "tbl[-1](n)"] {
            let entry = ENTRY.replace("trans(n)", callee);
            let source = format!(
                "var config={{signatureTimestamp:5}};var i=0;var tbl=[function(n){{return n}}];{entry}"
            );
            let script = inspect_native_script(&source).unwrap();
            assert_eq!(script.transform_calls, 0, "{callee}");
            assert_eq!(script.transform.err(), Some("trial_n_call_not_unique"));
        }
    }
    #[test]
    fn global_n_counts_are_entry_independent() {
        // No alr entry at all: counts must still describe n usage elsewhere.
        let source = "var c={signatureTimestamp:1};\
            function a(u){var n=u.get('n');u.set('n',t[0](n));u.set('n',f(n));u.set('n',o.g(n));}\
            function b(u,k){return u.get(k)+u.set(k,1);}";
        let script = inspect_native_script(source).unwrap();
        assert_eq!(script.url_entries, 0);
        // get("n"), set("n"), get(id), set(id), literal "n", computed/id/static sinks
        assert_eq!(script.global_n_counts, [1, 3, 1, 1, 4, 1, 1, 1]);
    }
    #[test]
    fn alias_counts_follow_bindings_initialised_to_n() {
        let source = "var c={signatureTimestamp:1};var K='n',O='x';\
            function a(u){var v=u.get(K);u.set(K,t[0](v));u.set(K,f(v));u.set(K,o.g(v));u.get(O);u.set(O,1);}\
            function b(K){return u.get(K)}";
        let script = diagnose_native_script(source).unwrap();
        // 1 key binding; get(K) once (the shadowed K in b is a different symbol);
        // set(K,_) x3 with computed / identifier / static sinks.
        assert_eq!(script.alias_n_counts, [1, 1, 3, 1, 1, 1]);
        assert_eq!(
            inspect_native_script(source).unwrap().alias_n_counts,
            [0; 6]
        );
    }
    #[test]
    fn literal_n_contexts_classify_each_occurrence() {
        let source = "var c={signatureTimestamp:1};\
            f('n');o.g('n');o['x']('n');\
            h(u,'n');o.g(u,'n');o['x'](u,'n');h(u,v,'n');\
            new W('n');var p={k:'n','n':1};if(q==='n'){}r='n';var a=['n'];var s=`n`+'m';";
        let script = diagnose_native_script(source).unwrap();
        // idx0 ident/static/computed, idx1 ident/static/computed, other,
        // new, prop value, prop key, binary, assignment, array, other
        assert_eq!(
            script.literal_n_contexts,
            [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0]
        );
    }
    #[test]
    fn distinct_timestamps_and_effectful_initializers_fail() {
        assert!(
            inspect_native_script("var x={signatureTimestamp:1},y={signatureTimestamp:2};")
                .is_err()
        );
        let source = format!("var x={{signatureTimestamp:1}};var trans=danger();{ENTRY}");
        assert_eq!(
            inspect_native_script(&source).unwrap().transform.err(),
            Some("trial_initializer_unsupported")
        );
    }
}
