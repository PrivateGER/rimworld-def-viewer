use crate::model::{DefElement, RimWorldDef};
use std::collections::HashMap;

pub fn build_reference_mappings(definitions: &mut [RimWorldDef]) {
    println!("\nBuilding reference mappings...");

    let mut definitions_by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, definition) in definitions.iter().enumerate() {
        if let Some(def_name) = &definition.def_name {
            definitions_by_name
                .entry(def_name.clone())
                .or_default()
                .push(index);
        }
    }

    let mut reference_count = 0;
    for index in 0..definitions.len() {
        let def_name = definitions[index].def_name.clone();
        let source_name = def_name
            .clone()
            .unwrap_or_else(|| definitions[index].id.clone());
        let (references, code_references) = extract_references(&definitions[index].elements);

        let valid_references: Vec<String> = references
            .into_iter()
            .filter(|reference_name| {
                definitions_by_name.contains_key(reference_name)
                    && def_name.as_ref() != Some(reference_name)
            })
            .collect();

        reference_count += valid_references.len();
        definitions[index].references_out = valid_references.clone();
        definitions[index].code_references = code_references;

        for reference_name in valid_references {
            if let Some(reference_indices) = definitions_by_name.get(&reference_name) {
                for &reference_index in reference_indices {
                    definitions[reference_index]
                        .references_in
                        .push(source_name.clone());
                }
            }
        }
    }

    for index in 0..definitions.len() {
        let Some(parent_name) = definitions[index].parent_name.clone() else {
            continue;
        };
        let child_name = definitions[index]
            .def_name
            .clone()
            .unwrap_or_else(|| definitions[index].id.clone());
        if definitions[index].def_name.as_ref() == Some(&parent_name) {
            continue;
        }
        let Some(parent_indices) = definitions_by_name.get(&parent_name) else {
            continue;
        };

        if !definitions[index].references_out.contains(&parent_name) {
            definitions[index].references_out.push(parent_name);
            definitions[index].references_out.sort();
            reference_count += 1;
        }

        for &parent_index in parent_indices {
            if !definitions[parent_index]
                .references_in
                .contains(&child_name)
            {
                definitions[parent_index]
                    .references_in
                    .push(child_name.clone());
            }
        }
    }

    println!(
        "  ✓ Reference mappings built: {} references found",
        reference_count
    );
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

    #[test]
    fn builds_outgoing_incoming_and_code_references() {
        let reference_element = DefElement {
            name: "targetDef".to_string(),
            attributes: HashMap::from([("Class".to_string(), "Example.Component".to_string())]),
            content: Some("Target".to_string()),
            children: Vec::new(),
            depth: 0,
        };
        let mut definitions = vec![
            definition("Source", vec![reference_element]),
            definition("Target", Vec::new()),
        ];

        build_reference_mappings(&mut definitions);

        assert_eq!(definitions[0].references_out, ["Target"]);
        assert_eq!(definitions[0].code_references, ["Example.Component"]);
        assert_eq!(definitions[1].references_in, ["Source"]);
    }

    #[test]
    fn represents_parent_relationships_in_both_directions() {
        let parent = definition("Parent", Vec::new());
        let mut child = definition("Child", Vec::new());
        child.parent_name = Some("Parent".to_string());
        let mut definitions = vec![parent, child];

        build_reference_mappings(&mut definitions);

        assert_eq!(definitions[1].references_out, ["Parent"]);
        assert_eq!(definitions[0].references_in, ["Child"]);
    }
}
