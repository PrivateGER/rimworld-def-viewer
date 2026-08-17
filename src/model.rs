use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefElement {
    pub name: String,
    pub attributes: HashMap<String, String>,
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<String>,
    pub children: Vec<DefElement>,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RimWorldDef {
    pub id: String,
    pub def_name: Option<String>,
    pub inheritance_name: Option<String>,
    pub class_name: Option<String>,
    pub def_type: String,
    pub label: Option<String>,
    pub description: Option<String>,
    pub parent_name: Option<String>,
    pub is_abstract: bool,
    pub elements: Vec<DefElement>,
    pub file_path: String,
    pub tags: Vec<String>,
    pub stats: Option<DefStats>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub references_out: Vec<DefinitionReference>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub references_in: Vec<DefinitionSummary>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub code_references: Vec<String>,
    pub raw_xml: String,
    pub extension: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefinitionReference {
    pub name: String,
    pub targets: Vec<DefinitionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefinitionSummary {
    pub id: String,
    pub def_name: Option<String>,
    pub inheritance_name: Option<String>,
    pub def_type: String,
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefStats {
    pub element_count: usize,
    pub max_depth: usize,
    pub has_complex_structure: bool,
}
