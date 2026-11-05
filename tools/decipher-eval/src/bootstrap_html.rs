//! Offline HTML-to-JavaScript syntax inventory. Never executes or resolves a URL.
use html5ever::{parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{NodeData, RcDom};

use crate::bootstrap::classify_js_url_properties;
use crate::bootstrap_context::inspect_js_url_context;

const HTML_LIMIT: usize = 4 * 1024 * 1024;
const SCRIPT_LIMIT: usize = crate::bootstrap::DIAGNOSTIC_SOURCE_LIMIT;
const NODE_LIMIT: usize = 100_000;
const SCRIPT_COUNT_LIMIT: usize = 128;

#[derive(Debug, PartialEq, Eq)]
pub enum HtmlInspectionError {
    SourceLimit,
    HtmlParse,
    NodeLimit,
    ScriptSourceLimit,
    ScriptCountLimit,
}

/// Fixed counts only. No source, attribute values, URLs or revision identifiers.
/// A property count is NOT a selectable candidate or proof of execution/binding.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct HtmlScriptInventory {
    pub inline_classic_scripts: usize,
    pub external_scripts: usize,
    pub unsupported_scripts: usize,
    pub excluded_subtrees: usize,
    pub javascript_parse_failures: usize,
    pub static_js_url_properties: usize,
    pub literal_string_properties: usize,
    pub other_values: usize,
    pub accessors_or_methods: usize,
    pub computed_literal_keys: usize,
    pub scripts_with_static_properties: usize,
    pub direct_config_properties: usize,
    pub music_watch_properties: usize,
    pub canonical_string_paths: usize,
    pub unsupported_config_members: usize,
    pub oversized_scripts: usize,
    pub oversized_scripts_with_lexical_key: usize,
    pub largest_inline_script_bytes: usize,
    pub script_count_limited: bool,
}

/// Parse with scripting enabled so noscript is not interpreted as active markup.
/// Reject HTML parse errors rather than certify a repaired/ambiguous document.
/// HTML/SVG/MathML/template subtrees are NOT interchangeable script contexts.
/// Limits bound input and accepted traversal, not parser peak heap/CPU or teardown.
pub fn inspect_bootstrap_html(html: &str) -> Result<HtmlScriptInventory, HtmlInspectionError> {
    if html.len() > HTML_LIMIT {
        return Err(HtmlInspectionError::SourceLimit);
    }
    let dom = parse_document(RcDom::default(), Default::default()).one(html);
    if !dom.errors.borrow().is_empty() {
        return Err(HtmlInspectionError::HtmlParse);
    }
    let mut inventory = HtmlScriptInventory::default();
    let mut stack = vec![dom.document.clone()];
    let mut nodes = 0;
    let mut scripts = 0;
    while let Some(node) = stack.pop() {
        nodes += 1;
        if nodes > NODE_LIMIT {
            return Err(HtmlInspectionError::NodeLimit);
        }
        if let NodeData::Element { name, attrs, .. } = &node.data {
            if name.ns.as_ref() != "http://www.w3.org/1999/xhtml"
                || matches!(name.local.as_ref(), "template" | "noscript")
            {
                inventory.excluded_subtrees += 1;
                continue;
            }
            if name.local.as_ref() == "script" {
                scripts += 1;
                if scripts > SCRIPT_COUNT_LIMIT {
                    inventory.script_count_limited = true;
                    break;
                }
                let attrs = attrs.borrow();
                if attrs.iter().any(|a| a.name.local.as_ref() == "src") {
                    inventory.external_scripts += 1;
                    continue;
                }
                let script_type = attrs
                    .iter()
                    .find(|a| a.name.local.as_ref() == "type")
                    .map(|a| a.value.trim().to_ascii_lowercase());
                let language = attrs
                    .iter()
                    .find(|a| a.name.local.as_ref() == "language")
                    .map(|a| a.value.trim().to_ascii_lowercase());
                if !matches!(
                    script_type.as_deref(),
                    None | Some("" | "text/javascript" | "application/javascript")
                ) || language
                    .as_deref()
                    .is_some_and(|l| !matches!(l, "" | "javascript"))
                    || attrs.iter().any(|a| a.name.local.as_ref() == "nomodule")
                {
                    inventory.unsupported_scripts += 1;
                    continue;
                }
                let mut source_bytes = 0;
                let mut lexical_key = false;
                for child in node.children.borrow().iter() {
                    if let NodeData::Text { contents } = &child.data {
                        let contents = contents.borrow();
                        source_bytes += contents.len();
                        lexical_key |=
                            contents.contains("\"jsUrl\"") || contents.contains("'jsUrl'");
                    }
                }
                inventory.inline_classic_scripts += 1;
                inventory.largest_inline_script_bytes =
                    inventory.largest_inline_script_bytes.max(source_bytes);
                if source_bytes > SCRIPT_LIMIT {
                    inventory.oversized_scripts += 1;
                    inventory.oversized_scripts_with_lexical_key += usize::from(lexical_key);
                    continue;
                }
                let mut source = String::with_capacity(source_bytes);
                for child in node.children.borrow().iter() {
                    if let NodeData::Text { contents } = &child.data {
                        source.push_str(&contents.borrow());
                    }
                }
                match classify_js_url_properties(&source) {
                    Ok(properties) => {
                        let context = inspect_js_url_context(&source)
                            .map_err(|_| HtmlInspectionError::HtmlParse)?;
                        inventory.direct_config_properties += context.direct_config_properties;
                        inventory.music_watch_properties += context.music_watch_properties;
                        inventory.canonical_string_paths += context.canonical_string_paths;
                        inventory.unsupported_config_members += context.unsupported_object_members;
                        inventory.static_js_url_properties += properties.static_data_properties;
                        inventory.literal_string_properties += properties.literal_strings;
                        inventory.other_values += properties.other_values;
                        inventory.accessors_or_methods += properties.accessors_or_methods;
                        inventory.computed_literal_keys += properties.computed_literal_keys;
                        inventory.scripts_with_static_properties +=
                            usize::from(properties.static_data_properties > 0);
                    }
                    Err(_) => inventory.javascript_parse_failures += 1,
                }
                continue;
            }
        }
        stack.extend(node.children.borrow().iter().rev().cloned());
    }
    Ok(inventory)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(body: &str) -> String {
        format!(
            "<!doctype html><html><head><title>synthetic</title></head><body>{body}</body></html>"
        )
    }

    #[test]
    fn selects_inline_classic_not_inert_external_or_foreign_sources() {
        let html = page(
            r#"
            <!-- <script>const x={jsUrl:'comment'}</script> -->
            <template><script>const x={jsUrl:'template'}</script></template>
            <noscript><script>const x={jsUrl:'noscript'}</script></noscript>
            <svg><script>const x={jsUrl:'foreign'}</script></svg>
            <textarea><script>const x={jsUrl:'text'}</script></textarea>
            <script src="synthetic.js">const x={jsUrl:'external'}</script>
            <script type="application/json">{"jsUrl":"json"}</script>
            <script type="module">const x={jsUrl:'module'}</script>
            <script nomodule>const x={jsUrl:'legacy'}</script>
            <script TYPE="text/javascript">const x={jsUrl:'synthetic-private'}</script>"#,
        );
        let inventory = inspect_bootstrap_html(&html).unwrap();
        assert_eq!(inventory.inline_classic_scripts, 1);
        assert_eq!(inventory.external_scripts, 1);
        assert_eq!(inventory.unsupported_scripts, 3);
        assert_eq!(inventory.excluded_subtrees, 3);
        assert_eq!(inventory.literal_string_properties, 1);
        assert!(!format!("{inventory:?}").contains("synthetic-private"));
    }

    #[test]
    fn retains_duplicates_and_parse_failures_without_selecting_a_candidate() {
        let html = page("<script>const x={jsUrl:'a',jsUrl:'b'};</script><script>if(false){const x={jsUrl:'c'}}</script><script>const x={</script>");
        let inventory = inspect_bootstrap_html(&html).unwrap();
        assert_eq!(inventory.static_js_url_properties, 3);
        assert_eq!(inventory.scripts_with_static_properties, 2);
        assert_eq!(inventory.javascript_parse_failures, 1);
    }

    #[test]
    fn html_raw_text_does_not_decode_entities_as_javascript_keys() {
        let html = page("<script>const x={'js&#85;rl':'not-jsUrl'};</script>");
        assert_eq!(
            inspect_bootstrap_html(&html)
                .unwrap()
                .static_js_url_properties,
            0
        );
        assert_eq!(
            inspect_bootstrap_html(&page("<script src=''></script>"))
                .unwrap()
                .external_scripts,
            1
        );
    }

    #[test]
    fn rejects_repaired_html_and_resource_limits() {
        assert_eq!(
            inspect_bootstrap_html("<script>x</script>").err(),
            Some(HtmlInspectionError::HtmlParse)
        );
        assert_eq!(
            inspect_bootstrap_html(&page("<script type='a' type='b'></script>")).err(),
            Some(HtmlInspectionError::HtmlParse)
        );
        assert_eq!(
            inspect_bootstrap_html(&"x".repeat(HTML_LIMIT + 1)).err(),
            Some(HtmlInspectionError::SourceLimit)
        );
        assert_eq!(
            inspect_bootstrap_html(&page(&format!(
                "<script>{}</script>",
                " ".repeat(SCRIPT_LIMIT + 1)
            )))
            .unwrap()
            .oversized_scripts,
            1
        );
        assert_eq!(
            inspect_bootstrap_html(&page(&"<script></script>".repeat(SCRIPT_COUNT_LIMIT + 1)))
                .unwrap()
                .script_count_limited,
            true
        );
    }

    #[test]
    fn oversized_script_does_not_erase_other_structural_observations() {
        let large = format!(
            "const x={{'jsUrl':'synthetic-private'}};{}",
            " ".repeat(SCRIPT_LIMIT)
        );
        let html = page(&format!("<script>{large}</script><script>ytcfg.set({{jsUrl:'/s/player/synthetic/x/base.js'}});</script>"));
        let report = inspect_bootstrap_html(&html).unwrap();
        assert_eq!(report.oversized_scripts, 1);
        assert_eq!(report.oversized_scripts_with_lexical_key, 1);
        assert_eq!(report.direct_config_properties, 1);
        assert_eq!(report.canonical_string_paths, 1);
        assert!(report.largest_inline_script_bytes > SCRIPT_LIMIT);
        assert!(!format!("{report:?}").contains("synthetic-private"));
    }

    #[test]
    fn diagnostic_megabyte_limit_does_not_expand_script_discovery_limit() {
        let source = format!(
            "ytcfg.set({{jsUrl:'/s/player/synthetic/x/base.js'}});{}",
            " ".repeat(706114)
        );
        let report = inspect_bootstrap_html(&page(&format!("<script>{source}</script>"))).unwrap();
        assert_eq!(report.oversized_scripts, 0);
        assert_eq!(report.javascript_parse_failures, 0);
        assert_eq!(report.direct_config_properties, 1);
        assert_eq!(
            crate::bootstrap::discover_bootstrap_script(&source, "jsUrl").err(),
            Some(crate::bootstrap::BootstrapError::SourceLimit)
        );
    }
}
