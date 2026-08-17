use crate::model::{DefElement, DefinitionReference, DefinitionSummary, RimWorldDef};
use crate::schema::{CustomLoader, CustomLoaderRule, ManagedType, ReferenceSchema};
use std::collections::{HashMap, HashSet};

pub fn build_reference_mappings(definitions: &mut [RimWorldDef], schema: Option<&ReferenceSchema>) {
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

    let mut code_references = vec![Vec::new(); definitions.len()];
    let mut reference_count = 0;
    let mut graph = ReferenceGraph::new(definitions, schema);

    for source_index in 0..definitions.len() {
        let analysis = analyze_definition(&definitions[source_index], schema);
        code_references[source_index] = analysis.code_references;

        for (reference_name, expected_types) in analysis.references {
            if graph.add_reference(
                source_index,
                &reference_name,
                expected_types.as_ref(),
                &definitions_by_name,
            ) {
                reference_count += 1;
            }
        }
    }

    for (source_index, definition) in definitions.iter().enumerate() {
        if let Some(parent_name) = definition.parent_name.clone()
            && graph.add_reference(
                source_index,
                &parent_name,
                None,
                &definitions_by_inheritance_name,
            )
        {
            reference_count += 1;
        }
    }

    let (mut outgoing, mut incoming) = (graph.outgoing, graph.incoming);
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

struct ReferenceGraph<'a> {
    definitions: &'a [RimWorldDef],
    schema: Option<&'a ReferenceSchema>,
    outgoing: Vec<Vec<DefinitionReference>>,
    incoming: Vec<Vec<DefinitionSummary>>,
}

impl<'a> ReferenceGraph<'a> {
    fn new(definitions: &'a [RimWorldDef], schema: Option<&'a ReferenceSchema>) -> Self {
        Self {
            definitions,
            schema,
            outgoing: vec![Vec::new(); definitions.len()],
            incoming: vec![Vec::new(); definitions.len()],
        }
    }

    fn add_reference(
        &mut self,
        source_index: usize,
        reference_name: &str,
        expected_types: Option<&HashSet<String>>,
        definitions_by_name: &HashMap<String, Vec<usize>>,
    ) -> bool {
        if self.outgoing[source_index]
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
            .filter(|target_index| {
                let (Some(expected_types), Some(schema)) = (expected_types, self.schema) else {
                    return true;
                };
                definition_runtime_type(&self.definitions[*target_index], schema).is_some_and(
                    |target_type| {
                        expected_types
                            .iter()
                            .any(|expected_type| schema.is_assignable(&target_type, expected_type))
                    },
                )
            })
            .collect();
        if target_indices.is_empty() {
            return false;
        }

        let mut targets: Vec<DefinitionSummary> = target_indices
            .iter()
            .map(|target_index| definition_summary(&self.definitions[*target_index]))
            .collect();
        targets.sort_by(|left, right| {
            left.def_type
                .cmp(&right.def_type)
                .then_with(|| left.id.cmp(&right.id))
        });

        if let [target_index] = target_indices.as_slice() {
            let source = definition_summary(&self.definitions[source_index]);
            if !self.incoming[*target_index]
                .iter()
                .any(|candidate| candidate.id == source.id)
            {
                self.incoming[*target_index].push(source);
            }
        }

        self.outgoing[source_index].push(DefinitionReference {
            name: reference_name.to_string(),
            targets,
        });
        true
    }
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

#[derive(Debug, Default)]
struct ReferenceAnalysis {
    references: HashMap<String, Option<HashSet<String>>>,
    code_references: Vec<String>,
}

fn analyze_definition(
    definition: &RimWorldDef,
    schema: Option<&ReferenceSchema>,
) -> ReferenceAnalysis {
    let mut analysis = ReferenceAnalysis::default();
    if let Some(class_name) = &definition.class_name {
        analysis.code_references.push(class_name.clone());
    }
    collect_code_references(&definition.elements, &mut analysis.code_references);

    if let Some(schema) = schema
        && let Some(root_type) = definition_runtime_type(definition, schema)
    {
        analyze_complex_elements(
            &definition.elements,
            &root_type,
            schema,
            &mut analysis.references,
        );
    } else {
        collect_heuristic_elements(&definition.elements, &mut analysis.references);
    }

    analysis.code_references.sort();
    analysis.code_references.dedup();
    analysis
}

fn definition_runtime_type(definition: &RimWorldDef, schema: &ReferenceSchema) -> Option<String> {
    let declared_type = schema.resolve_type(&definition.def_type, None);
    definition
        .class_name
        .as_deref()
        .and_then(|class_name| schema.resolve_type(class_name, declared_type.as_deref()))
        .or(declared_type)
}

fn analyze_complex_elements(
    elements: &[DefElement],
    type_name: &str,
    schema: &ReferenceSchema,
    references: &mut HashMap<String, Option<HashSet<String>>>,
) {
    if schema.custom_loader(type_name) != CustomLoader::None {
        collect_heuristic_elements(elements, references);
        return;
    }

    for element in elements {
        let Some(field) = schema.find_field(type_name, &element.name) else {
            collect_heuristic_element(element, references);
            continue;
        };
        analyze_value(element, &field.value_type, schema, references);
    }
}

fn analyze_value(
    element: &DefElement,
    value_type: &ManagedType,
    schema: &ReferenceSchema,
    references: &mut HashMap<String, Option<HashSet<String>>>,
) {
    match value_type {
        ManagedType::Named(declared_type) => {
            if schema.is_def_type(declared_type) {
                add_typed_reference(references, &element_text(element), declared_type);
                return;
            }

            let effective_type = element
                .attributes
                .get("Class")
                .and_then(|class_name| schema.resolve_type(class_name, Some(declared_type)))
                .unwrap_or_else(|| declared_type.clone());
            if !schema.contains_type(&effective_type) {
                if !element.children.is_empty() {
                    collect_heuristic_element(element, references);
                }
            } else {
                match schema.custom_loader(&effective_type) {
                    CustomLoader::None => analyze_complex_elements(
                        &element.children,
                        &effective_type,
                        schema,
                        references,
                    ),
                    CustomLoader::Known(rule) => {
                        analyze_custom_loader(element, rule, schema, references)
                    }
                    CustomLoader::Unknown => collect_heuristic_element(element, references),
                }
            }
        }
        ManagedType::List(item_type) | ManagedType::Array(item_type) => {
            for item in &element.children {
                analyze_value(item, item_type, schema, references);
            }
        }
        ManagedType::Dictionary(key_type, value_type) => {
            for item in &element.children {
                let key = item.children.iter().find(|child| child.name == "key");
                let value = item.children.iter().find(|child| child.name == "value");
                if let (Some(key), Some(value)) = (key, value) {
                    analyze_value(key, key_type, schema, references);
                    analyze_value(value, value_type, schema, references);
                } else {
                    collect_heuristic_element(item, references);
                }
            }
        }
        ManagedType::Primitive => {}
        ManagedType::Unknown => collect_heuristic_element(element, references),
    }
}

fn analyze_custom_loader(
    element: &DefElement,
    rule: CustomLoaderRule,
    schema: &ReferenceSchema,
    references: &mut HashMap<String, Option<HashSet<String>>>,
) {
    match rule {
        CustomLoaderRule::ElementName(expected_type) => {
            add_typed_reference(references, &element.name, expected_type);
        }
        CustomLoaderRule::ElementTextTypeFromName => {
            if let Some(expected_type) = schema.resolve_type(&element.name, None)
                && schema.is_def_type(&expected_type)
            {
                add_typed_reference(references, &element_text(element), &expected_type);
            }
        }
        CustomLoaderRule::ThingDefCount => {
            if element.name == "li" {
                add_typed_reference(references, &element_text(element), "Verse.ThingDef");
            } else {
                add_typed_reference(references, &element.name, "Verse.ThingDef");
                for child in element
                    .children
                    .iter()
                    .filter(|child| child.name == "thingDef" || child.name == "stuff")
                {
                    add_typed_reference(references, &element_text(child), "Verse.ThingDef");
                }
            }
        }
        CustomLoaderRule::NoReferences => {}
    }
}

fn add_typed_reference(
    references: &mut HashMap<String, Option<HashSet<String>>>,
    name: &str,
    expected_type: &str,
) {
    if name.is_empty() {
        return;
    }
    if let Some(types) = references
        .entry(name.to_string())
        .or_insert_with(|| Some(HashSet::new()))
        .as_mut()
    {
        types.insert(expected_type.to_string());
    }
}

fn add_heuristic_reference(references: &mut HashMap<String, Option<HashSet<String>>>, name: &str) {
    if !name.is_empty() {
        references.insert(name.to_string(), None);
    }
}

fn collect_heuristic_elements(
    elements: &[DefElement],
    references: &mut HashMap<String, Option<HashSet<String>>>,
) {
    for element in elements {
        collect_heuristic_element(element, references);
    }
}

fn collect_heuristic_element(
    element: &DefElement,
    references: &mut HashMap<String, Option<HashSet<String>>>,
) {
    if element.name != "defName" && element.name != "li" {
        add_heuristic_reference(references, &element.name);
    }

    if let Some(content) = &element.content
        && element.name != "defName"
    {
        add_heuristic_reference(references, content);
    }

    for (key, value) in &element.attributes {
        if key != "Class" {
            add_heuristic_reference(references, value);
        }
    }

    collect_heuristic_elements(&element.children, references);
}

fn collect_code_references(elements: &[DefElement], code_references: &mut Vec<String>) {
    for element in elements {
        if let Some(class_name) = element.attributes.get("Class") {
            code_references.push(class_name.clone());
        }
        collect_code_references(&element.children, code_references);
    }
}

fn element_text(element: &DefElement) -> String {
    let mut text = element.content.clone().unwrap_or_default();
    for child in &element.children {
        text.push_str(&element_text(child));
    }
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DefStats;
    use crate::schema::{ManagedField, ManagedTypeInfo};

    fn definition(name: &str, elements: Vec<DefElement>) -> RimWorldDef {
        RimWorldDef {
            id: format!("Data/Core/Defs/{name}.xml#0"),
            def_name: Some(name.to_string()),
            inheritance_name: None,
            class_name: None,
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

    fn element(name: &str, content: Option<&str>, children: Vec<DefElement>) -> DefElement {
        DefElement {
            name: name.to_string(),
            attributes: HashMap::new(),
            content: content.map(str::to_string),
            comments: Vec::new(),
            children,
            depth: 0,
        }
    }

    fn typed_schema() -> ReferenceSchema {
        ReferenceSchema::from_types(HashMap::from([
            ("Verse.Def".to_string(), ManagedTypeInfo::default()),
            (
                "Verse.ThingDef".to_string(),
                ManagedTypeInfo {
                    base_type: Some("Verse.Def".to_string()),
                    fields: vec![
                        ManagedField {
                            name: "tickerType".to_string(),
                            aliases: Vec::new(),
                            value_type: ManagedType::Primitive,
                        },
                        ManagedField {
                            name: "thingCategories".to_string(),
                            aliases: Vec::new(),
                            value_type: ManagedType::List(Box::new(ManagedType::Named(
                                "Verse.ThingCategoryDef".to_string(),
                            ))),
                        },
                        ManagedField {
                            name: "comps".to_string(),
                            aliases: Vec::new(),
                            value_type: ManagedType::List(Box::new(ManagedType::Named(
                                "Verse.CompProperties".to_string(),
                            ))),
                        },
                        ManagedField {
                            name: "statBases".to_string(),
                            aliases: Vec::new(),
                            value_type: ManagedType::List(Box::new(ManagedType::Named(
                                "RimWorld.StatModifier".to_string(),
                            ))),
                        },
                        ManagedField {
                            name: "shaderParameters".to_string(),
                            aliases: Vec::new(),
                            value_type: ManagedType::List(Box::new(ManagedType::Named(
                                "Verse.ShaderParameter".to_string(),
                            ))),
                        },
                        ManagedField {
                            name: "hyperlinks".to_string(),
                            aliases: Vec::new(),
                            value_type: ManagedType::List(Box::new(ManagedType::Named(
                                "Verse.DefHyperlink".to_string(),
                            ))),
                        },
                        ManagedField {
                            name: "products".to_string(),
                            aliases: Vec::new(),
                            value_type: ManagedType::List(Box::new(ManagedType::Named(
                                "Verse.ThingDefCountClass".to_string(),
                            ))),
                        },
                    ],
                    has_custom_loader: false,
                },
            ),
            (
                "Verse.ThingCategoryDef".to_string(),
                ManagedTypeInfo {
                    base_type: Some("Verse.Def".to_string()),
                    ..ManagedTypeInfo::default()
                },
            ),
            (
                "Verse.AbilityDef".to_string(),
                ManagedTypeInfo {
                    base_type: Some("Verse.Def".to_string()),
                    ..ManagedTypeInfo::default()
                },
            ),
            (
                "RimWorld.StatDef".to_string(),
                ManagedTypeInfo {
                    base_type: Some("Verse.Def".to_string()),
                    ..ManagedTypeInfo::default()
                },
            ),
            (
                "RimWorld.StatModifier".to_string(),
                ManagedTypeInfo {
                    has_custom_loader: true,
                    ..ManagedTypeInfo::default()
                },
            ),
            (
                "Verse.ShaderParameter".to_string(),
                ManagedTypeInfo {
                    has_custom_loader: true,
                    ..ManagedTypeInfo::default()
                },
            ),
            (
                "Verse.DefHyperlink".to_string(),
                ManagedTypeInfo {
                    has_custom_loader: true,
                    ..ManagedTypeInfo::default()
                },
            ),
            (
                "Verse.ThingDefCountClass".to_string(),
                ManagedTypeInfo {
                    has_custom_loader: true,
                    ..ManagedTypeInfo::default()
                },
            ),
            (
                "Verse.CompProperties".to_string(),
                ManagedTypeInfo::default(),
            ),
            (
                "Example.SpecialCompProperties".to_string(),
                ManagedTypeInfo {
                    base_type: Some("Verse.CompProperties".to_string()),
                    fields: vec![ManagedField {
                        name: "targetDef".to_string(),
                        aliases: Vec::new(),
                        value_type: ManagedType::Named("Verse.ThingDef".to_string()),
                    }],
                    has_custom_loader: false,
                },
            ),
        ]))
    }

    #[test]
    fn builds_outgoing_incoming_and_code_references() {
        let mut definitions = vec![
            definition("Source", vec![reference_element("Target")]),
            definition("Target", Vec::new()),
        ];

        build_reference_mappings(&mut definitions, None);

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

        build_reference_mappings(&mut definitions, None);

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

        build_reference_mappings(&mut definitions, None);

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

        build_reference_mappings(&mut definitions, None);

        assert_eq!(definitions[0].references_out.len(), 1);
        assert_eq!(definitions[0].references_out[0].name, "ConcreteName");
        assert_eq!(definitions[1].references_in.len(), 1);
    }

    #[test]
    fn typed_fields_reject_scalar_matches_and_incompatible_candidates() {
        let source_elements = vec![
            element("tickerType", Some("Normal"), Vec::new()),
            element(
                "thingCategories",
                None,
                vec![element("li", Some("Shared"), Vec::new())],
            ),
        ];
        let source = definition("Source", source_elements);
        let mut scalar_collision = definition("Normal", Vec::new());
        scalar_collision.def_type = "AbilityDef".to_string();
        let mut compatible = definition("Shared", Vec::new());
        compatible.def_type = "ThingCategoryDef".to_string();
        let mut incompatible = definition("Shared", Vec::new());
        incompatible.id = "Data/Core/Defs/SharedThing.xml#0".to_string();
        let mut definitions = vec![source, scalar_collision, compatible, incompatible];

        let schema = typed_schema();
        build_reference_mappings(&mut definitions, Some(&schema));

        assert_eq!(definitions[0].references_out.len(), 1);
        assert_eq!(definitions[0].references_out[0].name, "Shared");
        assert_eq!(definitions[0].references_out[0].targets.len(), 1);
        assert_eq!(
            definitions[0].references_out[0].targets[0].def_type,
            "ThingCategoryDef"
        );
        assert!(definitions[1].references_in.is_empty());
        assert_eq!(definitions[2].references_in.len(), 1);
        assert!(definitions[3].references_in.is_empty());
    }

    #[test]
    fn typed_traversal_honors_class_overrides_and_unknown_field_fallback() {
        let mut specialized_comp = element(
            "li",
            None,
            vec![element("targetDef", Some("TypedTarget"), Vec::new())],
        );
        specialized_comp.attributes.insert(
            "Class".to_string(),
            "Example.SpecialCompProperties".to_string(),
        );
        let source = definition(
            "Source",
            vec![
                element("comps", None, vec![specialized_comp]),
                element("unknownModField", Some("FallbackTarget"), Vec::new()),
            ],
        );
        let typed_target = definition("TypedTarget", Vec::new());
        let fallback_target = definition("FallbackTarget", Vec::new());
        let mut definitions = vec![source, typed_target, fallback_target];

        let schema = typed_schema();
        build_reference_mappings(&mut definitions, Some(&schema));

        let names: Vec<&str> = definitions[0]
            .references_out
            .iter()
            .map(|reference| reference.name.as_str())
            .collect();
        assert_eq!(names, ["FallbackTarget", "TypedTarget"]);
        assert_eq!(
            definitions[0].code_references,
            ["Example.SpecialCompProperties"]
        );
    }

    #[test]
    fn applies_stable_custom_loader_rules_without_guessing_from_values() {
        let source = definition(
            "Source",
            vec![
                element(
                    "statBases",
                    None,
                    vec![element("MarketValue", Some("100"), Vec::new())],
                ),
                element(
                    "shaderParameters",
                    None,
                    vec![element("FakeStat", Some("100"), Vec::new())],
                ),
                element(
                    "hyperlinks",
                    None,
                    vec![element("ThingDef", Some("LinkedThing"), Vec::new())],
                ),
                element(
                    "products",
                    None,
                    vec![
                        element(
                            "Steel",
                            None,
                            vec![
                                element("count", Some("5"), Vec::new()),
                                element("stuff", Some("Gold"), Vec::new()),
                            ],
                        ),
                        element("li", Some("WoodLog"), Vec::new()),
                    ],
                ),
            ],
        );
        let mut stat = definition("MarketValue", Vec::new());
        stat.def_type = "StatDef".to_string();
        let number_collision = definition("100", Vec::new());
        let mut shader_collision = definition("FakeStat", Vec::new());
        shader_collision.def_type = "StatDef".to_string();
        let linked_thing = definition("LinkedThing", Vec::new());
        let steel = definition("Steel", Vec::new());
        let gold = definition("Gold", Vec::new());
        let wood = definition("WoodLog", Vec::new());
        let mut incompatible_link = definition("LinkedThing", Vec::new());
        incompatible_link.id = "Data/Core/Defs/LinkedStat.xml#0".to_string();
        incompatible_link.def_type = "StatDef".to_string();
        let mut definitions = vec![
            source,
            stat,
            number_collision,
            shader_collision,
            linked_thing,
            incompatible_link,
            steel,
            gold,
            wood,
        ];

        let schema = typed_schema();
        build_reference_mappings(&mut definitions, Some(&schema));

        let references = &definitions[0].references_out;
        let names: Vec<&str> = references
            .iter()
            .map(|reference| reference.name.as_str())
            .collect();
        assert_eq!(
            names,
            ["Gold", "LinkedThing", "MarketValue", "Steel", "WoodLog"]
        );
        let hyperlink = references
            .iter()
            .find(|reference| reference.name == "LinkedThing")
            .unwrap();
        assert_eq!(hyperlink.targets.len(), 1);
        assert_eq!(hyperlink.targets[0].def_type, "ThingDef");
        assert!(definitions[2].references_in.is_empty());
        assert!(definitions[3].references_in.is_empty());
        assert!(definitions[5].references_in.is_empty());
    }
}
