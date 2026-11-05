//! Scope-preserving reconstruction PLAN. No code generation or execution.
use oxc_allocator::Allocator;
use oxc_ast::AstKind;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType, Span};

use crate::extraction::recognize_candidate;
use crate::resolution::{resolve_candidate, ResolutionError};

#[derive(Debug, PartialEq, Eq)]
pub enum PlanError {
    Resolution(ResolutionError),
    Semantic,
}

/// Positions are UTF-8 byte offsets in the input, not reusable scope identities.
/// A plan intentionally has no Debug/Serialize and cannot be passed to an evaluator.
pub struct ReconstructionPlan<'s> {
    source: &'s str,
    entry: Span,
    groups: Vec<ScopeGroup>,
}

/// Lexical ancestry is root-first. Declaration order is original source order,
/// NOT dependency-first: sorting dependencies cannot establish initialization timing.
pub struct ScopeGroup {
    ancestry: Vec<Span>,
    declarations: Vec<Span>,
    contains_entry: bool,
}

impl ScopeGroup {
    pub fn ancestry_ranges(&self) -> impl Iterator<Item = std::ops::Range<usize>> + '_ {
        self.ancestry
            .iter()
            .map(|s| s.start as usize..s.end as usize)
    }

    pub fn contains_entry(&self) -> bool {
        self.contains_entry
    }
}

impl<'s> ReconstructionPlan<'s> {
    pub fn groups(&self) -> &[ScopeGroup] {
        &self.groups
    }

    pub fn entry_source(&self) -> &'s str {
        &self.source[self.entry.start as usize..self.entry.end as usize]
    }

    pub fn declaration_sources(
        &self,
        group_index: usize,
    ) -> Option<impl Iterator<Item = &'s str> + '_> {
        self.groups.get(group_index).map(|group| {
            group
                .declarations
                .iter()
                .map(|s| &self.source[s.start as usize..s.end as usize])
        })
    }
}

/// Reparse the same bounded input and retain original binding-scope identities.
/// Does not copy enclosing code into an executable wrapper, move declarations,
/// prove effects/TDZ or validate player identity. Only synthetic input is supported.
pub fn plan_reconstruction(source: &str) -> Result<ReconstructionPlan<'_>, PlanError> {
    let resolved = resolve_candidate(source).map_err(PlanError::Resolution)?;
    let candidate = recognize_candidate(source)
        .map_err(|e| PlanError::Resolution(ResolutionError::Extraction(e)))?;
    let offset = candidate.function_source().as_ptr() as usize - source.as_ptr() as usize;
    let entry = Span::new(
        offset as u32,
        (offset + candidate.function_source().len()) as u32,
    );
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    let built = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .build(&parsed.program);
    if !built.errors.is_empty() {
        return Err(PlanError::Semantic);
    }
    let semantic = built.semantic;
    let scoping = semantic.scoping();
    let dependencies: Vec<_> = resolved
        .dependency_sources()
        .map(|text| {
            let start = text.as_ptr() as usize - source.as_ptr() as usize;
            Span::new(start as u32, (start + text.len()) as u32)
        })
        .collect();
    let mut scope_ids = Vec::new();
    let mut groups: Vec<ScopeGroup> = Vec::new();
    let mut found = 0;
    for node in semantic.nodes().iter() {
        let AstKind::VariableDeclarator(declaration) = node.kind() else {
            continue;
        };
        let is_entry = declaration
            .init
            .as_ref()
            .is_some_and(|init| init.span() == entry);
        if !is_entry && !dependencies.contains(&declaration.span) {
            continue;
        }
        let oxc_ast::ast::BindingPatternKind::BindingIdentifier(binding) = &declaration.id.kind
        else {
            return Err(PlanError::Semantic);
        };
        let symbol = binding.symbol_id.get().ok_or(PlanError::Semantic)?;
        let scope = scoping.symbol_scope_id(symbol);
        let index = match scope_ids.iter().position(|id| *id == scope) {
            Some(index) => index,
            None => {
                let mut ancestry: Vec<_> = scoping
                    .scope_ancestors(scope)
                    .map(|id| {
                        semantic
                            .nodes()
                            .get_node(scoping.get_node_id(id))
                            .kind()
                            .span()
                    })
                    .collect();
                ancestry.reverse();
                scope_ids.push(scope);
                groups.push(ScopeGroup {
                    ancestry,
                    declarations: Vec::new(),
                    contains_entry: false,
                });
                groups.len() - 1
            }
        };
        groups[index].declarations.push(declaration.span);
        groups[index].contains_entry |= is_entry;
        found += 1;
    }
    if found != dependencies.len() + 1 {
        return Err(PlanError::Semantic);
    }
    for group in &mut groups {
        group.declarations.sort_by_key(|s| s.start);
    }
    Ok(ReconstructionPlan {
        source,
        entry,
        groups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const ENTRY: &str = "function(u,sp='',s=''){u=new h.URL(u);u.set('alr','yes');return h}";

    #[test]
    fn same_scope_keeps_original_order_not_dependency_order() {
        let source = format!("const h=function(){{return k}};const k=1;const f={ENTRY};");
        let plan = plan_reconstruction(&source).ok().expect("plan");
        assert_eq!(plan.groups().len(), 1);
        let declarations: Vec<_> = plan.declaration_sources(0).unwrap().collect();
        assert!(declarations[0].starts_with("h="));
        assert_eq!(declarations[1], "k=1");
        assert_eq!(plan.entry_source(), ENTRY);
        assert!(plan.groups()[0].contains_entry());
        assert!(plan.declaration_sources(1).is_none());
    }

    #[test]
    fn same_name_bindings_in_distinct_scopes_are_not_flattened() {
        let source = format!("const k=1;const outer=function(){{return k}};(function(){{const k=2;const h=function(){{return outer()+k}};const f={ENTRY};}})();");
        let plan = plan_reconstruction(&source).ok().expect("plan");
        assert_eq!(plan.groups().len(), 2);
        let declarations: Vec<Vec<_>> = (0..2)
            .map(|i| plan.declaration_sources(i).unwrap().collect())
            .collect();
        assert!(declarations.iter().any(|d| d.contains(&"k=1")));
        assert!(declarations.iter().any(|d| d.contains(&"k=2")));
        assert_ne!(
            plan.groups()[0].ancestry_ranges().collect::<Vec<_>>(),
            plan.groups()[1].ancestry_ranges().collect::<Vec<_>>()
        );
    }

    #[test]
    fn does_not_execute_or_copy_unrelated_container_statements() {
        let source = format!(
            "(function(){{throw 'must-not-run';const h=function(){{}};const f={ENTRY};}})();"
        );
        let plan = plan_reconstruction(&source).ok().expect("plan");
        assert_eq!(plan.groups().len(), 1);
        assert!(plan
            .declaration_sources(0)
            .unwrap()
            .all(|text| !text.contains("must-not-run")));
        assert_eq!(plan.groups()[0].ancestry_ranges().count(), 2);
    }

    #[test]
    fn lexical_block_and_function_scopes_remain_distinct() {
        let source = format!("const h=function(){{}};{{const f={ENTRY};}}");
        let plan = plan_reconstruction(&source).ok().expect("plan");
        assert_eq!(plan.groups().len(), 2);
        assert_eq!(
            plan.groups().iter().filter(|g| g.contains_entry()).count(),
            1
        );
    }

    #[test]
    fn unicode_offsets_remain_byte_exact() {
        let source = format!("const label='🎵';const h=function(){{}};const f={ENTRY};");
        assert_eq!(
            plan_reconstruction(&source)
                .ok()
                .expect("plan")
                .entry_source(),
            ENTRY
        );
    }
}
