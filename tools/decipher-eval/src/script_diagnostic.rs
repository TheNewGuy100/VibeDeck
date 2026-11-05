//! Acquisition hint only: AST declaration provenance is not runtime config identity.
//! No execution, HTTP, serialization, or source/value-bearing diagnostics.
use html5ever::{parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{NodeData, RcDom};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, Expression, ObjectExpression, ObjectPropertyKind, PropertyKey, PropertyKind,
    Statement,
};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_span::SourceType;

// Deliberately independent from discovery/executor limits. Parser-only checkpoint.
const SCRIPT_BYTES: usize = 4 * 1024 * 1024;
pub struct MusicScriptDeclaration<'a> {
    _bootstrap: &'a str,
    path: String,
}
impl MusicScriptDeclaration<'_> {
    pub fn path(&self) -> &str {
        &self.path
    }
}

pub fn select_music_script(html: &str) -> Result<MusicScriptDeclaration<'_>, &'static str> {
    let report = crate::bootstrap_html::inspect_bootstrap_html(html)
        .map_err(|_| "script_diagnostic_html_rejected")?;
    if report.oversized_scripts != 0
        || report.script_count_limited
        || report.javascript_parse_failures != 0
        || report.unsupported_scripts != 0
        || report.unsupported_config_members != 0
        || report.static_js_url_properties != 1
        || report.literal_string_properties != 1
        || report.computed_literal_keys != 0
        || report.accessors_or_methods != 0
        || report.music_watch_properties != 1
        || report.canonical_string_paths != 1
    {
        return Err("script_diagnostic_declaration_rejected");
    }
    let dom = parse_document(RcDom::default(), Default::default()).one(html);
    let mut stack = vec![dom.document.clone()];
    let mut selected = None;
    while let Some(node) = stack.pop() {
        if let NodeData::Element { name, attrs, .. } = &node.data {
            if name.ns.as_ref() != "http://www.w3.org/1999/xhtml"
                || matches!(name.local.as_ref(), "template" | "noscript")
            {
                continue;
            }
            if name.local.as_ref() == "script" {
                let attrs = attrs.borrow();
                if attrs.iter().any(|a| a.name.local.as_ref() == "src") {
                    continue;
                }
                let source: String = node
                    .children
                    .borrow()
                    .iter()
                    .filter_map(|child| {
                        if let NodeData::Text { contents } = &child.data {
                            Some(contents.borrow().to_string())
                        } else {
                            None
                        }
                    })
                    .collect();
                let allocator = Allocator::default();
                let parsed = Parser::new(&allocator, &source, SourceType::cjs()).parse();
                for statement in &parsed.program.body {
                    let Statement::ExpressionStatement(s) = statement else {
                        continue;
                    };
                    let Expression::CallExpression(call) = &s.expression else {
                        continue;
                    };
                    let Expression::StaticMemberExpression(member) = &call.callee else {
                        continue;
                    };
                    if call.optional
                        || member.optional
                        || member.property.name != "set"
                        || !matches!(&member.object, Expression::Identifier(id) if id.name == "ytcfg")
                        || call.arguments.len() != 1
                    {
                        continue;
                    }
                    let Argument::ObjectExpression(object) = &call.arguments[0] else {
                        continue;
                    };
                    let Some(Expression::ObjectExpression(configs)) =
                        field(object, "WEB_PLAYER_CONTEXT_CONFIGS")?
                    else {
                        continue;
                    };
                    let Some(Expression::ObjectExpression(watch)) =
                        field(configs, "WEB_PLAYER_CONTEXT_CONFIG_ID_MUSIC_WATCH")?
                    else {
                        continue;
                    };
                    let Some(Expression::StringLiteral(value)) = field(watch, "jsUrl")? else {
                        continue;
                    };
                    if selected.replace(value.value.to_string()).is_some() {
                        return Err("script_diagnostic_declaration_rejected");
                    }
                }
                continue;
            }
        }
        stack.extend(node.children.borrow().iter().rev().cloned());
    }
    Ok(MusicScriptDeclaration {
        _bootstrap: html,
        path: selected.ok_or("script_diagnostic_declaration_rejected")?,
    })
}

fn field<'a, 'b>(
    object: &'b ObjectExpression<'a>,
    key: &str,
) -> Result<Option<&'b Expression<'a>>, &'static str> {
    let mut found = None;
    for member in &object.properties {
        let ObjectPropertyKind::ObjectProperty(p) = member else {
            return Err("script_diagnostic_declaration_rejected");
        };
        if p.computed || p.method || p.shorthand || p.kind != PropertyKind::Init {
            return Err("script_diagnostic_declaration_rejected");
        }
        let name = match &p.key {
            PropertyKey::StaticIdentifier(id) => id.name.as_str(),
            PropertyKey::StringLiteral(s) => s.value.as_str(),
            _ => continue,
        };
        if name == key && found.replace(&p.value).is_some() {
            return Err("script_diagnostic_declaration_rejected");
        }
    }
    Ok(found)
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct TimestampInventory {
    pub properties: usize,
    pub bounded_integer_literals: usize,
}
pub fn inspect_script_timestamp(source: &str) -> Result<TimestampInventory, &'static str> {
    if source.is_empty() || source.len() > SCRIPT_BYTES {
        return Err("script_diagnostic_source_limit");
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err("script_diagnostic_parse_rejected");
    }
    let mut inventory = TimestampInventory::default();
    struct Inspector<'a>(&'a mut TimestampInventory);
    impl<'a> Visit<'a> for Inspector<'_> {
        fn visit_object_property(&mut self, p: &oxc_ast::ast::ObjectProperty<'a>) {
            let key = match &p.key {
                PropertyKey::StaticIdentifier(id) if !p.computed => id.name.as_str(),
                PropertyKey::StringLiteral(s) => s.value.as_str(),
                _ => "",
            };
            if key == "signatureTimestamp" {
                self.0.properties += 1;
                if !p.computed && !p.method && p.kind == PropertyKind::Init {
                    if let Expression::NumericLiteral(n) = &p.value {
                        self.0.bounded_integer_literals += usize::from(
                            n.value >= 1.0 && n.value <= u32::MAX as f64 && n.value.fract() == 0.0,
                        );
                    }
                }
            }
            walk::walk_object_property(self, p);
        }
    }
    Inspector(&mut inventory).visit_program(&parsed.program);
    Ok(inventory)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(script: &str) -> String {
        format!("<!doctype html><html><head><script>{script}</script></head><body></body></html>")
    }
    const CONFIG: &str = "ytcfg.set({WEB_PLAYER_CONTEXT_CONFIGS:{WEB_PLAYER_CONTEXT_CONFIG_ID_MUSIC_WATCH:{jsUrl:'/s/player/synthetic/x/base.js'}}});";
    #[test]
    fn selection_is_exact_and_duplicate_ancestors_fail_closed() {
        let html = page(CONFIG);
        assert_eq!(
            select_music_script(&html).unwrap().path(),
            "/s/player/synthetic/x/base.js"
        );
        for bad in [
            format!("{CONFIG}{CONFIG}"),
            format!("if(false){{{CONFIG}}}"),
            CONFIG.replace("jsUrl:", "['jsUrl']:"),
            CONFIG.replace("jsUrl:", "...x,jsUrl:"),
            CONFIG.replace(
                "/s/player/synthetic/x/base.js",
                "https://invalid.example/base.js",
            ),
            CONFIG.replace(
                "WEB_PLAYER_CONTEXT_CONFIGS:{",
                "WEB_PLAYER_CONTEXT_CONFIGS:{},WEB_PLAYER_CONTEXT_CONFIGS:{",
            ),
        ] {
            assert!(select_music_script(&page(&bad)).is_err());
        }
    }
    #[test]
    fn timestamp_is_only_syntax_and_exports_no_values() {
        let report = inspect_script_timestamp("const x={signatureTimestamp:12345};function dead(){return {signatureTimestamp:other}};const text='signatureTimestamp:999';").unwrap();
        assert_eq!(report.properties, 2);
        assert_eq!(report.bounded_integer_literals, 1);
        assert!(!format!("{report:?}").contains("12345"));
        assert!(inspect_script_timestamp("const x={").is_err());
        assert!(inspect_script_timestamp(&" ".repeat(SCRIPT_BYTES + 1)).is_err());
    }
}
