use quick_xml::escape::escape;
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

impl DefElement {
    pub fn to_xml(&self, indent: usize) -> String {
        let mut xml = String::new();
        let indent_str = "  ".repeat(indent);

        xml.push_str(&format!("{}<{}", indent_str, self.name));

        if !self.attributes.is_empty() {
            for (key, value) in &self.attributes {
                xml.push_str(&format!(" {}=\"{}\"", key, escape(value)));
            }
        }

        if self.content.is_none() && self.comments.is_empty() && self.children.is_empty() {
            xml.push_str(" />\n");
            return xml;
        }

        xml.push('>');
        let has_nested_content = !self.comments.is_empty() || !self.children.is_empty();

        if let Some(content) = &self.content {
            if !has_nested_content {
                xml.push_str(&escape(content));
            } else {
                xml.push('\n');
                xml.push_str(&format!("{}{}", "  ".repeat(indent + 1), escape(content)));
                xml.push('\n');
            }
        } else if has_nested_content {
            xml.push('\n');
        }

        for comment in &self.comments {
            xml.push_str(&format!("{}<!--{}-->\n", "  ".repeat(indent + 1), comment));
        }

        for child in &self.children {
            xml.push_str(&child.to_xml(indent + 1));
        }

        if has_nested_content {
            xml.push_str(&format!("{}</{}>", indent_str, self.name));
        } else {
            xml.push_str(&format!("</{}>", self.name));
        }
        xml.push('\n');

        xml
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RimWorldDef {
    pub id: String,
    pub def_name: Option<String>,
    pub inheritance_name: Option<String>,
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
