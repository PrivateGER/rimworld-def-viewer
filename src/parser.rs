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

    fn parse_xml_file(&self, file_path: &Path) -> Result<Vec<RimWorldDef>> {
        let content = fs::read_to_string(file_path)?;
        let mut reader = Reader::from_str(&content);
        reader.trim_text(true);
        reader.expand_empty_elements(true);

        let mut buf = Vec::new();
        let mut element_stack = Vec::new();
        let mut in_defs = false;
        let mut parsed_defs = Vec::new();

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
                    for attr in e.attributes() {
                        let attr = attr?;
                        let key = std::str::from_utf8(attr.key.as_ref())?.to_string();
                        let value = attr.decode_and_unescape_value(&reader)?.into_owned();
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
                                .cloned();

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
                            let id = format!("{}#{}", relative_path, parsed_defs.len());

                            parsed_defs.push(RimWorldDef {
                                id,
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
                    let text = e.unescape()?;
                    Self::append_content(&mut element_stack, &text);
                }
                Ok(Event::CData(e)) => {
                    let text = reader.decoder().decode(e.as_ref())?;
                    Self::append_content(&mut element_stack, &text);
                }
                Ok(Event::Eof) => break,
                Err(error) => return Err(anyhow::anyhow!("Error parsing XML: {}", error)),
                _ => {}
            }
            buf.clear();
        }

        Ok(parsed_defs)
    }

    fn append_content(element_stack: &mut [DefElement], text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }

        if let Some(element) = element_stack.last_mut() {
            element
                .content
                .get_or_insert_with(String::new)
                .push_str(text);
        }
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
                match self.parse_xml_file(entry.path()) {
                    Ok(mut parsed_defs) => {
                        processed_count += 1;
                        let new_defs = parsed_defs.len();
                        self.parsed_defs.append(&mut parsed_defs);
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

        if error_count > 0 {
            return Err(anyhow::anyhow!(
                "Failed to parse {} of {} XML files",
                error_count,
                file_count
            ));
        }

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

    fn parse_definitions(xml: &str) -> Result<Vec<RimWorldDef>> {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let file_path = std::env::temp_dir().join(format!(
            "rimworld-def-viewer-{}-{unique}.xml",
            std::process::id()
        ));
        fs::write(&file_path, xml)?;

        let parser = DefParser::new(std::env::temp_dir().to_string_lossy().into_owned());
        let parse_result = parser.parse_xml_file(&file_path);
        let _ = fs::remove_file(&file_path);
        parse_result
    }

    fn parse_single_definition(xml: &str) -> Result<RimWorldDef> {
        let mut parsed_defs = parse_definitions(xml)?;
        assert_eq!(parsed_defs.len(), 1);
        Ok(parsed_defs.remove(0))
    }

    fn unique_temp_directory(name: &str) -> Result<std::path::PathBuf> {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "rimworld-def-viewer-{name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(directory.join("Data/Core/Defs"))?;
        Ok(directory)
    }

    #[test]
    fn parses_self_closing_elements_without_losing_attributes_or_siblings() -> Result<()> {
        let parsed_def = parse_single_definition(
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

        assert_eq!(parsed_def.def_name.as_deref(), Some("WorkSite_ChopTrees"));

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

    #[test]
    fn preserves_text_split_by_comments() -> Result<()> {
        let parsed_def = parse_single_definition(
            r#"<Defs>
                <ThingDef>
                    <defName>SplitText</defName>
                    <label>left<!-- split -->right &amp; more</label>
                </ThingDef>
            </Defs>"#,
        )?;

        assert_eq!(parsed_def.label.as_deref(), Some("leftright & more"));
        Ok(())
    }

    #[test]
    fn preserves_cdata_as_element_content() -> Result<()> {
        let parsed_def = parse_single_definition(
            r#"<Defs>
                <ThingDef>
                    <defName>CdataText</defName>
                    <description><![CDATA[Use <tag> & keep text]]></description>
                </ThingDef>
            </Defs>"#,
        )?;

        assert_eq!(
            parsed_def.description.as_deref(),
            Some("Use <tag> & keep text")
        );
        Ok(())
    }

    #[test]
    fn reconstructed_xml_escapes_decoded_text_entities() -> Result<()> {
        let parsed_def = parse_single_definition(
            r#"<Defs>
                <ThingDef>
                    <defName>EntityText</defName>
                    <label>A &amp; B &lt; C</label>
                </ThingDef>
            </Defs>"#,
        )?;

        assert_eq!(parsed_def.label.as_deref(), Some("A & B < C"));
        assert!(
            parsed_def
                .raw_xml
                .contains("<label>A &amp; B &lt; C</label>"),
            "reconstructed XML was not escaped: {}",
            parsed_def.raw_xml
        );
        Ok(())
    }

    #[test]
    fn decodes_attribute_entities_and_reescapes_them_once() -> Result<()> {
        let parsed_def = parse_single_definition(
            r#"<Defs>
                <ThingDef>
                    <defName>AttributeEntity</defName>
                    <value note="A &amp; B &quot;quoted&quot;" />
                </ThingDef>
            </Defs>"#,
        )?;

        let value = parsed_def
            .elements
            .iter()
            .find(|element| element.name == "value")
            .unwrap();
        assert_eq!(
            value.attributes.get("note").map(String::as_str),
            Some(r#"A & B "quoted""#)
        );
        assert!(
            parsed_def
                .raw_xml
                .contains(r#"note="A &amp; B &quot;quoted&quot;""#),
            "reconstructed XML escaped the attribute incorrectly: {}",
            parsed_def.raw_xml
        );
        Ok(())
    }

    #[test]
    fn rejects_a_malformed_file_without_retaining_partial_definitions() -> Result<()> {
        let rimworld_path = unique_temp_directory("malformed")?;
        fs::write(
            rimworld_path.join("Data/Core/Defs/Broken.xml"),
            r#"<Defs>
                <ThingDef><defName>MustBeRolledBack</defName></ThingDef>
                <ThingDef><defName>NeverCompleted</defName>
            </Defs>"#,
        )?;

        let mut parser = DefParser::new(rimworld_path.to_string_lossy().into_owned());
        let scan_result = parser.scan_defs_directory();
        let _ = fs::remove_dir_all(&rimworld_path);

        assert!(scan_result.is_err(), "malformed input should fail the scan");
        assert!(
            parser.parsed_defs.is_empty(),
            "definitions from a malformed file must not be committed"
        );
        Ok(())
    }

    #[test]
    fn assigns_unique_ids_without_inventing_definition_names() -> Result<()> {
        let definitions = parse_definitions(
            r#"<Defs>
                <SongDef><clipPath>Songs/First</clipPath></SongDef>
                <SongDef><clipPath>Songs/Second</clipPath></SongDef>
            </Defs>"#,
        )?;

        assert_eq!(definitions.len(), 2);
        assert!(
            definitions
                .iter()
                .all(|definition| definition.def_name.is_none())
        );
        assert_ne!(definitions[0].id, definitions[1].id);
        assert!(definitions[0].id.ends_with("#0"));
        assert!(definitions[1].id.ends_with("#1"));
        Ok(())
    }
}
