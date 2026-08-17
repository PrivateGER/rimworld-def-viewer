use crate::model::{DefElement, DefStats, RimWorldDef};
use anyhow::Result;
use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

pub struct DefParser {
    rimworld_data_path: String,
    parsed_defs: Vec<RimWorldDef>,
}

impl DefParser {
    pub fn new(rimworld_data_path: String) -> Self {
        Self {
            rimworld_data_path,
            parsed_defs: Vec::new(),
        }
    }

    pub fn into_defs(self) -> Vec<RimWorldDef> {
        self.parsed_defs
    }

    fn detect_extension(&self, file_path: &Path) -> String {
        let path_str = file_path.to_string_lossy().to_lowercase();

        if path_str.contains("anomaly") {
            "Anomaly".to_string()
        } else if path_str.contains("biotech") {
            "Biotech".to_string()
        } else if path_str.contains("ideology") {
            "Ideology".to_string()
        } else if path_str.contains("royalty") {
            "Royalty".to_string()
        } else if path_str.contains("odyssey") {
            "Odyssey".to_string()
        } else if path_str.contains("core") {
            "Core".to_string()
        } else {
            "Unknown".to_string()
        }
    }

    fn parse_xml_file(&mut self, file_path: &Path) -> Result<()> {
        let content = fs::read_to_string(file_path)?;
        let mut reader = Reader::from_str(&content);
        reader.trim_text(true);
        reader.expand_empty_elements(true);

        let mut buf = Vec::new();
        let mut element_stack = Vec::new();
        let mut in_defs = false;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = std::str::from_utf8(e.name().as_ref())
                        .unwrap_or("")
                        .to_string();

                    if name == "Defs" {
                        in_defs = true;
                        continue;
                    }

                    let mut attributes = HashMap::new();
                    for attr in e.attributes().flatten() {
                        let key = std::str::from_utf8(attr.key.as_ref())
                            .unwrap_or("")
                            .to_string();
                        let value = std::str::from_utf8(&attr.value).unwrap_or("").to_string();
                        attributes.insert(key, value);
                    }

                    if in_defs && !element_stack.is_empty() {
                        let element = DefElement {
                            name: name.clone(),
                            attributes,
                            content: None,
                            children: Vec::new(),
                            depth: element_stack.len(),
                        };

                        element_stack.push(element);
                    } else if in_defs {
                        let element = DefElement {
                            name: name.clone(),
                            attributes,
                            content: None,
                            children: Vec::new(),
                            depth: 0,
                        };

                        element_stack.push(element);
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name = std::str::from_utf8(e.name().as_ref())
                        .unwrap_or("")
                        .to_string();

                    if name == "Defs" {
                        in_defs = false;
                        continue;
                    }

                    if in_defs && !element_stack.is_empty() {
                        let element = element_stack.pop().unwrap();

                        if element_stack.is_empty() {
                            let def_name = element
                                .attributes
                                .get("Name")
                                .or_else(|| {
                                    element
                                        .children
                                        .iter()
                                        .find(|child| child.name == "defName")
                                        .and_then(|child| child.content.as_ref())
                                })
                                .map_or("Unknown", |value| value)
                                .to_string();

                            let label = element
                                .children
                                .iter()
                                .find(|child| child.name == "label")
                                .and_then(|child| child.content.as_ref())
                                .cloned();
                            let description = element
                                .children
                                .iter()
                                .find(|child| child.name == "description")
                                .and_then(|child| child.content.as_ref())
                                .cloned();
                            let parent_name = element.attributes.get("ParentName").cloned();
                            let is_abstract = element
                                .attributes
                                .get("Abstract")
                                .map(|value| value == "True")
                                .unwrap_or(false);

                            let tags =
                                self.generate_tags(&element, is_abstract, parent_name.is_some());
                            let stats = self.calculate_stats(&element.children);
                            let raw_xml = element.to_xml(0);
                            let extension = self.detect_extension(file_path);

                            let relative_path = if let Ok(stripped) =
                                file_path.strip_prefix(&self.rimworld_data_path)
                            {
                                stripped.to_string_lossy().to_string()
                            } else {
                                file_path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string()
                            };

                            self.parsed_defs.push(RimWorldDef {
                                def_name,
                                def_type: element.name.clone(),
                                label,
                                description,
                                parent_name,
                                is_abstract,
                                elements: element.children.clone(),
                                file_path: relative_path,
                                tags,
                                stats,
                                references_out: Vec::new(),
                                references_in: Vec::new(),
                                code_references: Vec::new(),
                                raw_xml,
                                extension,
                            });
                        } else if let Some(parent) = element_stack.last_mut() {
                            parent.children.push(element);
                        }
                    }
                }
                Ok(Event::Text(e)) => {
                    let text = e.unescape().unwrap_or_default().trim().to_string();
                    if !text.is_empty()
                        && !element_stack.is_empty()
                        && let Some(element) = element_stack.last_mut()
                    {
                        element.content = Some(text);
                    }
                }
                Ok(Event::Eof) => break,
                Err(error) => return Err(anyhow::anyhow!("Error parsing XML: {}", error)),
                _ => {}
            }
            buf.clear();
        }

        Ok(())
    }

    pub fn scan_defs_directory(&mut self) -> Result<()> {
        let defs_path = Path::new(&self.rimworld_data_path).join("Data");
        println!("Scanning directory: {}", defs_path.display());

        let mut file_count = 0;
        let mut processed_count = 0;
        let mut error_count = 0;

        for entry in WalkDir::new(&defs_path) {
            let entry = entry?;
            if entry.file_type().is_file() && entry.path().extension().unwrap_or_default() == "xml"
            {
                file_count += 1;
                let initial_def_count = self.parsed_defs.len();

                match self.parse_xml_file(entry.path()) {
                    Ok(_) => {
                        processed_count += 1;
                        let new_defs = self.parsed_defs.len() - initial_def_count;
                        if new_defs > 0 {
                            println!(
                                "  ✓ {}: {} definitions",
                                entry
                                    .path()
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy(),
                                new_defs
                            );
                        }
                    }
                    Err(error) => {
                        error_count += 1;
                        eprintln!("  ✗ Error parsing {}: {}", entry.path().display(), error);
                    }
                }
            }
        }

        println!("\nScan complete:");
        println!("  Files found: {}", file_count);
        println!("  Files processed: {}", processed_count);
        println!("  Errors: {}", error_count);
        println!("  Total definitions: {}", self.parsed_defs.len());

        Ok(())
    }

    fn generate_tags(
        &self,
        element: &DefElement,
        is_abstract: bool,
        has_parent: bool,
    ) -> Vec<String> {
        let mut tags = Vec::new();

        if is_abstract {
            tags.push("Abstract".to_string());
        }

        if has_parent {
            tags.push("Inherits".to_string());
        }

        let common_elements: Vec<&str> = element
            .children
            .iter()
            .map(|child| child.name.as_str())
            .collect();

        if common_elements.contains(&"costList") {
            tags.push("Craftable".to_string());
        }
        if common_elements.contains(&"researchPrerequisites") {
            tags.push("Research Required".to_string());
        }
        if common_elements.contains(&"statBases") {
            tags.push("Has Stats".to_string());
        }
        if common_elements.contains(&"comps") {
            tags.push("Has Components".to_string());
        }
        if common_elements.contains(&"recipes") {
            tags.push("Has Recipes".to_string());
        }

        tags
    }

    fn calculate_stats(&self, elements: &[DefElement]) -> Option<DefStats> {
        if elements.is_empty() {
            return None;
        }

        let element_count = Self::count_elements(elements);
        let max_depth = Self::calculate_max_depth(elements, 0);
        let has_complex_structure = element_count > 20 || max_depth > 4;

        Some(DefStats {
            element_count,
            max_depth,
            has_complex_structure,
        })
    }

    fn count_elements(elements: &[DefElement]) -> usize {
        elements.len()
            + elements
                .iter()
                .map(|element| Self::count_elements(&element.children))
                .sum::<usize>()
    }

    fn calculate_max_depth(elements: &[DefElement], current_depth: usize) -> usize {
        elements
            .iter()
            .map(|element| {
                if element.children.is_empty() {
                    current_depth + 1
                } else {
                    Self::calculate_max_depth(&element.children, current_depth + 1)
                }
            })
            .max()
            .unwrap_or(current_depth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_self_closing_elements_without_losing_attributes_or_siblings() -> Result<()> {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let file_path = std::env::temp_dir().join(format!(
            "rimworld-def-viewer-{}-{unique}.xml",
            std::process::id()
        ));
        fs::write(
            &file_path,
            r#"<Defs>
                <GenStepDef>
                    <defName>WorkSite_ChopTrees</defName>
                    <genStep Class="GenStep_ChopTrees"/>
                    <container>
                        <marker value="present" />
                    </container>
                    <after>still parsed</after>
                </GenStepDef>
            </Defs>"#,
        )?;

        let mut parser = DefParser::new(std::env::temp_dir().to_string_lossy().into_owned());
        let parse_result = parser.parse_xml_file(&file_path);
        let _ = fs::remove_file(&file_path);
        parse_result?;

        assert_eq!(parser.parsed_defs.len(), 1);
        let parsed_def = &parser.parsed_defs[0];
        assert_eq!(parsed_def.def_name, "WorkSite_ChopTrees");

        let gen_step = parsed_def
            .elements
            .iter()
            .find(|element| element.name == "genStep")
            .expect("self-closing genStep should be retained");
        assert_eq!(
            gen_step.attributes.get("Class").map(String::as_str),
            Some("GenStep_ChopTrees")
        );
        assert!(gen_step.content.is_none());
        assert!(gen_step.children.is_empty());

        let marker = parsed_def
            .elements
            .iter()
            .find(|element| element.name == "container")
            .and_then(|element| element.children.first())
            .expect("nested self-closing element should be retained");
        assert_eq!(marker.name, "marker");
        assert_eq!(
            marker.attributes.get("value").map(String::as_str),
            Some("present")
        );

        assert_eq!(
            parsed_def
                .elements
                .iter()
                .find(|element| element.name == "after")
                .and_then(|element| element.content.as_deref()),
            Some("still parsed")
        );
        assert!(
            parsed_def
                .raw_xml
                .contains(r#"<genStep Class="GenStep_ChopTrees" />"#)
        );

        Ok(())
    }
}
