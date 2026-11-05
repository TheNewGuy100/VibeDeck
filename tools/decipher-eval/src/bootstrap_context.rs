//! Syntax location only. Does not attest ytcfg identity, execution or player binding.
use crate::bootstrap::BootstrapError;
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, Expression, ObjectExpression, ObjectPropertyKind, PropertyKey, PropertyKind,
    Statement,
};
use oxc_parser::Parser;
use oxc_span::SourceType;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ConfigLocation {
    pub direct_config_properties: usize,
    pub music_watch_properties: usize,
    pub canonical_string_paths: usize,
    pub unsupported_object_members: usize,
}

pub fn inspect_js_url_context(source: &str) -> Result<ConfigLocation, BootstrapError> {
    if source.len() > crate::bootstrap::DIAGNOSTIC_SOURCE_LIMIT {
        return Err(BootstrapError::SourceLimit);
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(BootstrapError::Parse);
    }
    let mut result = ConfigLocation::default();
    for statement in &parsed.program.body {
        let Statement::ExpressionStatement(statement) = statement else {
            continue;
        };
        let Expression::CallExpression(call) = &statement.expression else {
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
        if let Argument::ObjectExpression(object) = &call.arguments[0] {
            inspect_object(object, &mut Vec::new(), &mut result);
        }
    }
    Ok(result)
}

fn inspect_object(
    object: &ObjectExpression<'_>,
    parents: &mut Vec<String>,
    result: &mut ConfigLocation,
) {
    for member in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = member else {
            result.unsupported_object_members += 1;
            continue;
        };
        if property.computed
            || property.method
            || property.shorthand
            || property.kind != PropertyKind::Init
        {
            result.unsupported_object_members += 1;
            continue;
        }
        let name = match &property.key {
            PropertyKey::StaticIdentifier(id) => id.name.as_str(),
            PropertyKey::StringLiteral(value) => value.value.as_str(),
            _ => {
                result.unsupported_object_members += 1;
                continue;
            }
        };
        if name == "jsUrl" {
            result.direct_config_properties += 1;
            if parents.iter().map(String::as_str).eq([
                "WEB_PLAYER_CONTEXT_CONFIGS",
                "WEB_PLAYER_CONTEXT_CONFIG_ID_MUSIC_WATCH",
            ]) {
                result.music_watch_properties += 1;
            }
            if let Expression::StringLiteral(value) = &property.value {
                let path = value.value.as_str();
                if path.len() <= 512
                    && path.starts_with("/s/player/")
                    && path.ends_with("/base.js")
                    && path.split('/').count() >= 6
                    && path.split('/').skip(1).all(|s| {
                        !s.is_empty()
                            && s != "."
                            && s != ".."
                            && s.bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
                    })
                {
                    result.canonical_string_paths += 1;
                }
            }
        }
        if let Expression::ObjectExpression(nested) = &property.value {
            parents.push(name.to_owned());
            inspect_object(nested, parents, result);
            parents.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizes_only_exact_direct_syntax_location_without_exporting_values() {
        let source = "ytcfg.set({WEB_PLAYER_CONTEXT_CONFIGS:{WEB_PLAYER_CONTEXT_CONFIG_ID_MUSIC_WATCH:{jsUrl:'/s/player/synthetic-private/x/base.js'}}});";
        let report = inspect_js_url_context(source).unwrap();
        assert_eq!(report.direct_config_properties, 1);
        assert_eq!(report.music_watch_properties, 1);
        assert_eq!(report.canonical_string_paths, 1);
        assert!(!format!("{report:?}").contains("synthetic-private"));
        for wrapped in [
            format!("if(false){{{source}}}"),
            format!("function unused(){{{source}}}"),
            source.replace("ytcfg.set", "other.set"),
        ] {
            assert_eq!(
                inspect_js_url_context(&wrapped).unwrap(),
                ConfigLocation::default()
            );
        }
    }
    #[test]
    fn duplicates_and_unsupported_members_are_not_silently_accepted() {
        let result = inspect_js_url_context(
            "ytcfg.set({jsUrl:'a',jsUrl:'b',...x,get other(){throw 1},['jsUrl']:'c'});",
        )
        .unwrap();
        assert_eq!(result.direct_config_properties, 2);
        assert_eq!(result.canonical_string_paths, 0);
        assert_eq!(result.unsupported_object_members, 3);
        assert_eq!(result.music_watch_properties, 0);
    }
}
