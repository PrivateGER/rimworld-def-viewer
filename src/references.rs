use crate::model::{DefElement, DefinitionReference, DefinitionSummary, RimWorldDef};
use std::collections::HashMap;

pub fn build_reference_mappings(definitions: &mut [RimWorldDef]) {
    println!("\nBuilding reference mappings...");

    let mut definitions_by_name: HashMap<String, Vec<usize>> = HashMap::new();
    let mut definitions_by_inheritance_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, definition) in definitions.iter().enumerate() {
        if let Some(def_name) = &definition.def_name {
            definitions_by_name
                .entry(def_name.clone())
                .or_default()
                .push(index);
        }
        if let Some(inheritance_name) = &definition.inheritance_name {
            definitions_by_inheritance_name
                .entry(inheritance_name.clone())
                .or_default()
                .push(index);
        }
    }

    let mut outgoing = vec![Vec::new(); definitions.len()];
    let mut incoming = vec![Vec::new(); definitions.len()];
    let mut code_references = vec![Vec::new(); definitions.len()];
    let mut reference_count = 0;

    for source_index in 0..definitions.len() {
        let (reference_names, source_code_references) =
            extract_references(&definitions[source_index].elements);
        code_references[source_index] = source_code_references;

        for reference_name in reference_names {
            if add_reference(
                source_index,
                &reference_name,
                definitions,
                &definitions_by_name,
                &mut outgoing,
                &mut incoming,
            ) {
                reference_count += 1;
            }
        }
    }

    for source_index in 0..definitions.len() {
        if let Some(parent_name) = definitions[source_index].parent_name.clone()
            && add_reference(
                source_index,
                &parent_name,
                definitions,
                &definitions_by_inheritance_name,
                &mut outgoing,
                &mut incoming,
            )
        {
            reference_count += 1;
        }
    }

    for index in 0..definitions.len() {
        outgoing[index].sort_by(|left, right| left.name.cmp(&right.name));
        incoming[index].sort_by(|left, right| left.id.cmp(&right.id));
        definitions[index].references_out = std::mem::take(&mut outgoing[index]);
        definitions[index].references_in = std::mem::take(&mut incoming[index]);
        definitions[index].code_references = std::mem::take(&mut code_references[index]);
    }

    println!(
        "  ✓ Reference mappings built: {} references found",
        reference_count
    );
}

fn add_reference(
    source_index: usize,
    reference_name: &str,
    definitions: &[RimWorldDef],
    definitions_by_name: &HashMap<String, Vec<usize>>,
    outgoing: &mut [Vec<DefinitionReference>],
    incoming: &mut [Vec<DefinitionSummary>],
) -> bool {
    if outgoing[source_index]
        .iter()
        .any(|reference| reference.name == reference_name)
    {
        return false;
    }

    let Some(matching_indices) = definitions_by_name.get(reference_name) else {
        return false;
    };
    let target_indices: Vec<usize> = matching_indices
        .iter()
        .copied()
        .filter(|target_index| *target_index != source_index)
        .collect();
    if target_indices.is_empty() {
        return false;
    }

    let mut targets: Vec<DefinitionSummary> = target_indices
        .iter()
        .map(|target_index| definition_summary(&definitions[*target_index]))
        .collect();
    targets.sort_by(|left, right| {
        left.def_type
            .cmp(&right.def_type)
            .then_with(|| left.id.cmp(&right.id))
    });

    if let [target_index] = target_indices.as_slice() {
        let source = definition_summary(&definitions[source_index]);
        if !incoming[*target_index]
            .iter()
            .any(|candidate| candidate.id == source.id)
        {
            incoming[*target_index].push(source);
        }
    }

    outgoing[source_index].push(DefinitionReference {
        name: reference_name.to_string(),
        targets,
    });
    true
}

fn definition_summary(definition: &RimWorldDef) -> DefinitionSummary {
    DefinitionSummary {
        id: definition.id.clone(),
        def_name: definition.def_name.clone(),
        inheritance_name: definition.inheritance_name.clone(),
        def_type: definition.def_type.clone(),
        file_path: definition.file_path.clone(),
    }
}

fn extract_references(elements: &[DefElement]) -> (Vec<String>, Vec<String>) {
    let mut references = Vec::new();
    let mut code_references = Vec::new();

    extract_references_recursive(elements, &mut references, &mut code_references);

    references.sort();
    references.dedup();
    code_references.sort();
    code_references.dedup();

    (references, code_references)
}

fn extract_references_recursive(
    elements: &[DefElement],
    references: &mut Vec<String>,
    code_references: &mut Vec<String>,
) {
    for element in elements {
        if element.name != "defName" && element.name != "li" {
            references.push(element.name.clone());
        }

        if let Some(content) = &element.content
            && element.name != "defName"
        {
            references.push(content.clone());
        }

        for (key, value) in &element.attributes {
            if key == "Class" {
                code_references.push(value.clone());
            } else {
                references.push(value.clone());
            }
        }

        extract_references_recursive(&element.children, references, code_references);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DefStats;

    fn definition(name: &str, elements: Vec<DefElement>) -> RimWorldDef {
        RimWorldDef {
            id: format!("Data/Core/Defs/{name}.xml#0"),
            def_name: Some(name.to_string()),
            inheritance_name: None,
            def_type: "ThingDef".to_string(),
            label: None,
            description: None,
            parent_name: None,
            is_abstract: false,
            elements,
            file_path: format!("Data/Core/Defs/{name}.xml"),
            tags: Vec::new(),
            stats: Some(DefStats {
                element_count: 1,
                max_depth: 1,
                has_complex_structure: false,
            }),
            references_out: Vec::new(),
            references_in: Vec::new(),
            code_references: Vec::new(),
            raw_xml: String::new(),
            extension: "Core".to_string(),
        }
    }

    fn reference_element(target: &str) -> DefElement {
        DefElement {
            name: "targetDef".to_string(),
            attributes: HashMap::from([("Class".to_string(), "Example.Component".to_string())]),
            content: Some(target.to_string()),
            comments: Vec::new(),
            children: Vec::new(),
            depth: 0,
        }
    }

    #[test]
    fn builds_outgoing_incoming_and_code_references() {
        let mut definitions = vec![
            definition("Source", vec![reference_element("Target")]),
            definition("Target", Vec::new()),
        ];

        build_reference_mappings(&mut definitions);

        assert_eq!(definitions[0].references_out.len(), 1);
        assert_eq!(definitions[0].references_out[0].name, "Target");
        assert_eq!(
            definitions[0].references_out[0].targets[0].id,
            definitions[1].id
        );
        assert_eq!(definitions[0].code_references, ["Example.Component"]);
        assert_eq!(definitions[1].references_in.len(), 1);
        assert_eq!(definitions[1].references_in[0].id, definitions[0].id);
    }

    #[test]
    fn represents_parent_relationships_in_both_directions() {
        let mut parent = definition("UnusedDefName", Vec::new());
        parent.def_name = None;
        parent.inheritance_name = Some("Parent".to_string());
        let mut child = definition("Child", Vec::new());
        child.parent_name = Some("Parent".to_string());
        let mut definitions = vec![parent, child];

        build_reference_mappings(&mut definitions);

        assert_eq!(definitions[1].references_out.len(), 1);
        assert_eq!(definitions[1].references_out[0].name, "Parent");
        assert_eq!(
            definitions[1].references_out[0].targets[0].id,
            definitions[0].id
        );
        assert_eq!(definitions[0].references_in[0].id, definitions[1].id);
    }

    #[test]
    fn preserves_ambiguous_candidates_without_asserting_incoming_edges() {
        let source = definition("Source", vec![reference_element("Shared")]);
        let mut first = definition("Shared", Vec::new());
        let mut second = definition("Shared", Vec::new());
        first.id = "Data/Core/Defs/First.xml#0".to_string();
        second.id = "Data/Core/Defs/Second.xml#0".to_string();
        second.def_type = "PawnKindDef".to_string();
        let mut definitions = vec![source, first, second];

        build_reference_mappings(&mut definitions);

        let reference = &definitions[0].references_out[0];
        assert_eq!(reference.name, "Shared");
        assert_eq!(reference.targets.len(), 2);
        assert!(definitions[1].references_in.is_empty());
        assert!(definitions[2].references_in.is_empty());
    }

    #[test]
    fn uses_definition_names_only_for_content_references() {
        let source = definition(
            "Source",
            vec![
                reference_element("ConcreteName"),
                reference_element("TemplateName"),
            ],
        );
        let mut target = definition("ConcreteName", Vec::new());
        target.inheritance_name = Some("TemplateName".to_string());
        let mut definitions = vec![source, target];

        build_reference_mappings(&mut definitions);

        assert_eq!(definitions[0].references_out.len(), 1);
        assert_eq!(definitions[0].references_out[0].name, "ConcreteName");
        assert_eq!(definitions[1].references_in.len(), 1);
    }
}
