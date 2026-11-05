//! Offline experiment for an explicitly identified inline config script, NOT an
//! HTML extractor or evidence that a particular config key exists in the service.
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, Expression, ObjectPropertyKind, PropertyKey, PropertyKind, Statement,
};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_span::SourceType;

const SOURCE_LIMIT: usize = 256 * 1024;
// Diagnostics only. Does NOT expand discovery eligibility or executor limits.
pub(crate) const DIAGNOSTIC_SOURCE_LIMIT: usize = 1024 * 1024;

/// Sanitized syntax inventory, not declaration eligibility or execution evidence.
/// Counts include nested/conditional/dead code. No source, offsets or values escape.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct JsUrlStructure {
    pub static_data_properties: usize,
    pub literal_strings: usize,
    pub other_values: usize,
    pub accessors_or_methods: usize,
    pub computed_literal_keys: usize,
}

/// Inspect one caller-selected JavaScript source, never HTML. Unknown computed
/// keys/aliases and property assignments are not resolved. A zero count is not
/// proof of absence at runtime. Parser resources have no absolute heap/CPU cap.
pub fn classify_js_url_properties(source: &str) -> Result<JsUrlStructure, BootstrapError> {
    if source.len() > DIAGNOSTIC_SOURCE_LIMIT {
        return Err(BootstrapError::SourceLimit);
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(BootstrapError::Parse);
    }
    let mut inventory = JsUrlStructure::default();
    inventory.visit_program(&parsed.program);
    Ok(inventory)
}

impl<'a> Visit<'a> for JsUrlStructure {
    fn visit_object_property(&mut self, property: &oxc_ast::ast::ObjectProperty<'a>) {
        let named = match &property.key {
            PropertyKey::StaticIdentifier(id) => !property.computed && id.name == "jsUrl",
            PropertyKey::StringLiteral(value) => value.value == "jsUrl",
            _ => false,
        };
        if named {
            if property.computed {
                self.computed_literal_keys += 1;
            } else if property.method || property.kind != PropertyKind::Init {
                self.accessors_or_methods += 1;
            } else {
                self.static_data_properties += 1;
                if matches!(property.value, Expression::StringLiteral(_)) {
                    self.literal_strings += 1;
                } else {
                    self.other_values += 1;
                }
            }
        }
        walk::walk_object_property(self, property);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum BootstrapError {
    SourceLimit,
    Parse,
    Unsupported,
    Missing,
    Ambiguous,
    Path,
}

/// No Debug/Serialize: retain source identity and offsets, not a persistent revision.
pub struct BootstrapScript<'s> {
    source: &'s str,
    path: String,
    declaration: std::ops::Range<usize>,
}

impl<'s> BootstrapScript<'s> {
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn declaration(&self) -> &'s str {
        &self.source[self.declaration.clone()]
    }
}

/// Caller supplies a config key justified by separate evidence. Accept only direct
/// ytcfg.set({...}) statements with static data properties. Reject other statements
/// rather than silently omit mutations/conditional execution. Never execute source.
/// This recognizes a declaration, not the provider's binding to a later player POST.
/// Parser heap/CPU are not hard-limited by the source size or QuickJS limits.
pub fn discover_bootstrap_script<'s>(
    source: &'s str,
    key: &str,
) -> Result<BootstrapScript<'s>, BootstrapError> {
    if source.len() > SOURCE_LIMIT {
        return Err(BootstrapError::SourceLimit);
    }
    if key.is_empty() || key.len() > 128 {
        return Err(BootstrapError::Unsupported);
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(BootstrapError::Parse);
    }
    let mut found = None;
    for statement in &parsed.program.body {
        let Statement::ExpressionStatement(statement) = statement else {
            return Err(BootstrapError::Unsupported);
        };
        let Expression::CallExpression(call) = &statement.expression else {
            return Err(BootstrapError::Unsupported);
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Err(BootstrapError::Unsupported);
        };
        if call.optional
            || member.optional
            || member.property.name != "set"
            || !matches!(&member.object, Expression::Identifier(id) if id.name == "ytcfg")
            || call.arguments.len() != 1
        {
            return Err(BootstrapError::Unsupported);
        }
        let Argument::ObjectExpression(object) = &call.arguments[0] else {
            return Err(BootstrapError::Unsupported);
        };
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                return Err(BootstrapError::Unsupported);
            };
            if property.computed
                || property.method
                || property.shorthand
                || property.kind != PropertyKind::Init
            {
                return Err(BootstrapError::Unsupported);
            }
            let name = match &property.key {
                PropertyKey::StaticIdentifier(id) => id.name.as_str(),
                PropertyKey::StringLiteral(value) => value.value.as_str(),
                _ => return Err(BootstrapError::Unsupported),
            };
            if !matches!(
                property.value,
                Expression::StringLiteral(_)
                    | Expression::NumericLiteral(_)
                    | Expression::BooleanLiteral(_)
                    | Expression::NullLiteral(_)
            ) {
                return Err(BootstrapError::Unsupported);
            }
            if name != key {
                continue;
            }
            if found.is_some() {
                return Err(BootstrapError::Ambiguous);
            }
            let Expression::StringLiteral(value) = &property.value else {
                return Err(BootstrapError::Unsupported);
            };
            let path = value.value.as_str();
            // Same conservative path subset as native acquisition. No host guessing,
            // percent escapes, query/fragment, protocol-relative URL or dot segments.
            if path.len() > 512
                || !path.starts_with("/s/player/")
                || !path.ends_with("/base.js")
                || path.split('/').count() < 6
                || path.split('/').skip(1).any(|segment| {
                    segment.is_empty()
                        || segment == "."
                        || segment == ".."
                        || !segment
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
                })
            {
                return Err(BootstrapError::Path);
            }
            found = Some(BootstrapScript {
                source,
                path: path.to_owned(),
                declaration: property.span.start as usize..property.span.end as usize,
            });
        }
    }
    found.ok_or(BootstrapError::Missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structural_inventory_ignores_lexical_decoys() {
        let source = r#"// {jsUrl:'comment'}
            const text = "{jsUrl:'string'}";
            const regex = /jsUrl:'regex'/;
            const value = {"js\u0055rl":'synthetic-private'};"#;
        let result = classify_js_url_properties(source).unwrap();
        assert_eq!(result.static_data_properties, 1);
        assert_eq!(result.literal_strings, 1);
        assert!(!format!("{result:?}").contains("synthetic-private"));
    }

    #[test]
    fn structural_inventory_distinguishes_nonliteral_and_computed_forms() {
        let result = classify_js_url_properties(
            "const x={jsUrl:null,jsUrl,jsUrl:compute(),['jsUrl']:'x',get jsUrl(){throw 1},jsUrl(){}};"
        ).unwrap();
        assert_eq!(result.static_data_properties, 3);
        assert_eq!(result.other_values, 3);
        assert_eq!(result.computed_literal_keys, 1);
        assert_eq!(result.accessors_or_methods, 2);
    }

    #[test]
    fn structural_inventory_preserves_ambiguity_without_claiming_execution() {
        let result = classify_js_url_properties(
            "if(false){const x={jsUrl:'a'}} function unused(){return {nested:{jsUrl:'b'}}}",
        )
        .unwrap();
        assert_eq!(result.static_data_properties, 2);
        assert_eq!(result.literal_strings, 2);
        assert_eq!(
            classify_js_url_properties("x.jsUrl='x';").unwrap(),
            JsUrlStructure::default()
        );
        assert_eq!(
            classify_js_url_properties("const x={").err(),
            Some(BootstrapError::Parse)
        );
        assert_eq!(
            classify_js_url_properties(&"x".repeat(DIAGNOSTIC_SOURCE_LIMIT + 1)).err(),
            Some(BootstrapError::SourceLimit)
        );
    }
    // Hypothetical fixture key, NOT observed provider data or a confirmed contract.
    const KEY: &str = "PLAYER_JS_URL";
    const PATH: &str = "/s/player/synthetic/player_ias.vflset/en_US/base.js";
    fn declaration() -> String {
        format!("ytcfg.set({{\"{KEY}\":\"{PATH}\"}});")
    }

    #[test]
    fn literal_declaration_preserves_unicode_offsets_and_decodes_js_escapes() {
        let source = format!(
            "/* 🎵 */{}",
            declaration().replace("/s/player", "\\/s\\/player")
        );
        let candidate = discover_bootstrap_script(&source, KEY).unwrap();
        assert_eq!(candidate.path(), PATH);
        assert!(candidate.declaration().starts_with("\"PLAYER_JS_URL\":"));
        assert!(candidate.declaration().contains("\\/s\\/player"));
    }

    #[test]
    fn duplicate_declarations_are_ambiguous_even_when_equal() {
        let source = format!("{}{}", declaration(), declaration());
        assert_eq!(
            discover_bootstrap_script(&source, KEY).err(),
            Some(BootstrapError::Ambiguous)
        );
        let source = format!("ytcfg.set({{\"{KEY}\":\"{PATH}\",\"{KEY}\":\"{PATH}\"}});");
        assert_eq!(
            discover_bootstrap_script(&source, KEY).err(),
            Some(BootstrapError::Ambiguous)
        );
    }

    #[test]
    fn rejects_nonliteral_conditional_shadowed_or_mutated_config() {
        for source in [
            format!("if(false){{{}}}", declaration()),
            format!("function unused(){{{}}}", declaration()),
            format!("const ytcfg={{set(){{}}}};{}", declaration()),
            format!("{}ytcfg.set=evil;", declaration()),
            format!("ytcfg.set({{['{KEY}']:'{PATH}'}});"),
            format!("ytcfg.set({{get {KEY}(){{throw 1}}}});"),
            format!("ytcfg.set({{{KEY}:compute()}});"),
            format!("ytcfg.set({{...other,{KEY}:'{PATH}'}});"),
            format!("ytcfg.set({{other:compute(),{KEY}:'{PATH}'}});"),
        ] {
            assert_eq!(
                discover_bootstrap_script(&source, KEY).err(),
                Some(BootstrapError::Unsupported)
            );
        }
    }

    #[test]
    fn missing_parse_limits_and_invalid_paths_fail_closed() {
        assert_eq!(
            discover_bootstrap_script("// ytcfg.set({PLAYER_JS_URL:'fake'})", KEY).err(),
            Some(BootstrapError::Missing)
        );
        assert_eq!(
            discover_bootstrap_script("ytcfg.set({unrelated:'value'});", KEY).err(),
            Some(BootstrapError::Missing)
        );
        assert_eq!(
            discover_bootstrap_script(&(declaration() + "let = ;"), KEY).err(),
            Some(BootstrapError::Parse)
        );
        assert_eq!(
            discover_bootstrap_script(&"x".repeat(SOURCE_LIMIT + 1), KEY).err(),
            Some(BootstrapError::SourceLimit)
        );
        for path in [
            "//evil.invalid/base.js",
            "/s/player/../x/base.js",
            "/s/player/x/y/base.js?x=1",
            "/s/player/%2e%2e/x/base.js",
        ] {
            let source = format!("ytcfg.set({{{KEY}:'{path}'}});");
            assert_eq!(
                discover_bootstrap_script(&source, KEY).err(),
                Some(BootstrapError::Path)
            );
        }
    }
}
