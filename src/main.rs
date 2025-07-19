use anyhow::Result;
use chrono::Utc;
use clap::{Arg, Command};
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use walkdir::WalkDir;


#[derive(Debug, Clone, Serialize, Deserialize)]
struct DefElement {
    name: String,
    attributes: HashMap<String, String>,
    content: Option<String>,
    children: Vec<DefElement>,
    depth: usize,
}

impl DefElement {
    fn to_xml(&self, indent: usize) -> String {
        let mut xml = String::new();
        let indent_str = "  ".repeat(indent);
        
        // Opening tag with attributes
        xml.push_str(&format!("{}<{}", indent_str, self.name));
        
        // Add attributes if any
        if !self.attributes.is_empty() {
            for (key, value) in &self.attributes {
                xml.push_str(&format!(" {}=\"{}\"", key, value));
            }
        }
        
        // Check if this is a self-closing tag (no content and no children)
        if self.content.is_none() && self.children.is_empty() {
            xml.push_str(" />\n");
            return xml;
        }
        
        xml.push('>');
        
        // Add content if it exists
        if let Some(content) = &self.content {
            if self.children.is_empty() {
                // Simple content on same line
                xml.push_str(content);
            } else {
                // Content with children - put content on new line
                xml.push('\n');
                xml.push_str(&format!("{}{}", "  ".repeat(indent + 1), content));
                xml.push('\n');
            }
        } else if !self.children.is_empty() {
            xml.push('\n');
        }
        
        // Add children
        for child in &self.children {
            xml.push_str(&child.to_xml(indent + 1));
        }
        
        // Closing tag
        if !self.children.is_empty() || (self.content.is_some() && !self.children.is_empty()) {
            xml.push_str(&format!("{}</{}>", indent_str, self.name));
        } else {
            xml.push_str(&format!("</{}>", self.name));
        }
        xml.push('\n');
        
        xml
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RimWorldDef {
    def_name: String,
    def_type: String,
    label: Option<String>,
    description: Option<String>,
    parent_name: Option<String>,
    is_abstract: bool,
    elements: Vec<DefElement>,
    file_path: String,
    tags: Vec<String>,
    stats: Option<DefStats>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    references_out: Vec<String>,  // DefNames this def references
    #[serde(skip_serializing_if = "Vec::is_empty")]
    references_in: Vec<String>,   // DefNames that reference this def
    #[serde(skip_serializing_if = "Vec::is_empty")]
    code_references: Vec<String>, // C# class names referenced (from Class attributes)
    raw_xml: String,             // Original XML representation
    extension: String,           // RimWorld extension/DLC: Core, Royalty, Ideology, Biotech, Anomaly
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DefStats {
    element_count: usize,
    max_depth: usize,
    has_complex_structure: bool,
}

struct DefParser {
    rimworld_data_path: String,
    parsed_defs: Vec<RimWorldDef>,
    def_name_map: HashMap<String, Vec<usize>>,  // Map def names to their indices in parsed_defs
}

impl DefParser {
    fn new(rimworld_data_path: String) -> Self {
        Self {
            rimworld_data_path,
            parsed_defs: Vec::new(),
            def_name_map: HashMap::new(),
        }
    }

    fn detect_extension(&self, file_path: &Path) -> String {
        // Convert path to string for analysis
        let path_str = file_path.to_string_lossy().to_lowercase();
        
        // Check for DLC/extension folders in the path
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
            // Default to Unknown if we can't determine the extension
            "Unknown".to_string()
        }
    }

    fn parse_xml_file(&mut self, file_path: &Path) -> Result<()> {
        let content = fs::read_to_string(file_path)?;
        let mut reader = Reader::from_str(&content);
        reader.trim_text(true);

        let mut buf = Vec::new();
        let mut element_stack = Vec::new();
        let mut in_defs = false;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = std::str::from_utf8(e.name().as_ref()).unwrap_or("").to_string();
                    
                    if name == "Defs" {
                        in_defs = true;
                        continue;
                    }

                    if in_defs && !element_stack.is_empty() {
                        let mut attributes = HashMap::new();
                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("").to_string();
                            let value = std::str::from_utf8(&attr.value).unwrap_or("").to_string();
                            attributes.insert(key, value);
                        }

                        let element = DefElement {
                            name: name.clone(),
                            attributes,
                            content: None,
                            children: Vec::new(),
                            depth: element_stack.len(),
                        };

                        element_stack.push(element);
                    } else if in_defs {
                        let mut attributes = HashMap::new();
                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("").to_string();
                            let value = std::str::from_utf8(&attr.value).unwrap_or("").to_string();
                            attributes.insert(key, value);
                        }

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
                    let name = std::str::from_utf8(e.name().as_ref()).unwrap_or("").to_string();
                    
                    if name == "Defs" {
                        in_defs = false;
                        continue;
                    }

                    if in_defs && !element_stack.is_empty() {
                        let element = element_stack.pop().unwrap();
                        
                        if element_stack.is_empty() {
                            let def_name = element.attributes.get("Name")
                                .or_else(|| element.children.iter().find(|c| c.name == "defName").and_then(|c| c.content.as_ref()))
                                .map_or("Unknown", |v| v).to_string();
                            
                            let label = element.children.iter().find(|c| c.name == "label").and_then(|c| c.content.as_ref()).cloned();
                            let description = element.children.iter().find(|c| c.name == "description").and_then(|c| c.content.as_ref()).cloned();
                            let parent_name = element.attributes.get("ParentName").cloned();
                            let is_abstract = element.attributes.get("Abstract").map(|v| v == "True").unwrap_or(false);
                            
                            let tags = self.generate_tags(&element, is_abstract, parent_name.is_some());
                            let stats = self.calculate_stats(&element.children);

                            // Generate raw XML
                            let raw_xml = element.to_xml(0);

                            // Detect extension from file path
                            let extension = self.detect_extension(file_path);

                            // Make file path relative to RimWorld directory
                            let relative_path = if let Ok(stripped) = file_path.strip_prefix(&self.rimworld_data_path) {
                                stripped.to_string_lossy().to_string()
                            } else {
                                file_path.file_name().unwrap_or_default().to_string_lossy().to_string()
                            };
                            
                            let rim_def = RimWorldDef {
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
                            };

                            self.parsed_defs.push(rim_def);
                        } else if let Some(parent) = element_stack.last_mut() {
                            parent.children.push(element);
                        }
                    }
                }
                Ok(Event::Text(e)) => {
                    let text = e.unescape().unwrap_or_default().trim().to_string();
                    if !text.is_empty() && !element_stack.is_empty() {
                        if let Some(element) = element_stack.last_mut() {
                            element.content = Some(text);
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(anyhow::anyhow!("Error parsing XML: {}", e)),
                _ => {}
            }
            buf.clear();
        }

        Ok(())
    }

    fn scan_defs_directory(&mut self) -> Result<()> {
        let defs_path = Path::new(&self.rimworld_data_path).join("Data");
        println!("Scanning directory: {}", defs_path.display());
        
        let mut file_count = 0;
        let mut processed_count = 0;
        let mut error_count = 0;
        
        for entry in WalkDir::new(&defs_path) {
            let entry = entry?;
            if entry.file_type().is_file() && entry.path().extension().unwrap_or_default() == "xml" {
                file_count += 1;
                let initial_def_count = self.parsed_defs.len();
                
                match self.parse_xml_file(entry.path()) {
                    Ok(_) => {
                        processed_count += 1;
                        let new_defs = self.parsed_defs.len() - initial_def_count;
                        if new_defs > 0 {
                            println!("  ✓ {}: {} definitions", 
                                entry.path().file_name().unwrap_or_default().to_string_lossy(), 
                                new_defs);
                        }
                    },
                    Err(e) => {
                        error_count += 1;
                        eprintln!("  ✗ Error parsing {}: {}", entry.path().display(), e);
                    }
                }
            }
        }
        
        println!("\nScan complete:");
        println!("  Files found: {}", file_count);
        println!("  Files processed: {}", processed_count);
        println!("  Errors: {}", error_count);
        println!("  Total definitions: {}", self.parsed_defs.len());
        
        // Build reference mappings
        self.build_reference_mappings();
        
        Ok(())
    }
    
    fn generate_tags(&self, element: &DefElement, is_abstract: bool, has_parent: bool) -> Vec<String> {
        let mut tags = Vec::new();
        
        if is_abstract {
            tags.push("Abstract".to_string());
        }
        
        if has_parent {
            tags.push("Inherits".to_string());
        }
        
        // Add tags based on common element names
        let common_elements: Vec<&str> = element.children.iter().map(|e| e.name.as_str()).collect();
        
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
        
        let element_count = self.count_elements(elements);
        let max_depth = self.calculate_max_depth(elements, 0);
        let has_complex_structure = element_count > 20 || max_depth > 4;
        
        Some(DefStats {
            element_count,
            max_depth,
            has_complex_structure,
        })
    }
    
    fn count_elements(&self, elements: &[DefElement]) -> usize {
        elements.len() + elements.iter().map(|e| self.count_elements(&e.children)).sum::<usize>()
    }
    
    fn calculate_max_depth(&self, elements: &[DefElement], current_depth: usize) -> usize {
        elements.iter().map(|e| {
            if e.children.is_empty() {
                current_depth + 1
            } else {
                self.calculate_max_depth(&e.children, current_depth + 1)
            }
        }).max().unwrap_or(current_depth)
    }
    
    fn build_reference_mappings(&mut self) {
        println!("\nBuilding reference mappings...");
        
        // First pass: build def name index
        for (idx, def) in self.parsed_defs.iter().enumerate() {
            self.def_name_map.entry(def.def_name.clone()).or_default().push(idx);
        }
        
        // Second pass: extract references and build relationships
        let mut reference_count = 0;
        for i in 0..self.parsed_defs.len() {
            let def_name = self.parsed_defs[i].def_name.clone();
            let (references, code_refs) = self.extract_references(&self.parsed_defs[i].elements);
            
            // Filter to only valid def names and exclude self-references
            let valid_refs: Vec<String> = references.into_iter()
                .filter(|ref_name| {
                    self.def_name_map.contains_key(ref_name) && ref_name != &def_name
                })
                .collect();
            
            reference_count += valid_refs.len();
            
            // Update outgoing references
            self.parsed_defs[i].references_out = valid_refs.clone();
            
            // Update code references (C# References)
            self.parsed_defs[i].code_references = code_refs;
            
            // Update incoming references for each referenced def
            for ref_name in valid_refs {
                if let Some(ref_indices) = self.def_name_map.get(&ref_name) {
                    // Add the reference to ALL definitions with this name
                    for &ref_idx in ref_indices {
                        self.parsed_defs[ref_idx].references_in.push(def_name.clone());
                    }
                }
            }
        }
        
        // Handle parent references
        for i in 0..self.parsed_defs.len() {
            if let Some(parent_name) = &self.parsed_defs[i].parent_name.clone() {
                if let Some(parent_indices) = self.def_name_map.get(parent_name) {
                    let child_name = self.parsed_defs[i].def_name.clone();
                    for &parent_idx in parent_indices {
                        if !self.parsed_defs[parent_idx].references_in.contains(&child_name) {
                            self.parsed_defs[parent_idx].references_in.push(child_name.clone());
                        }
                    }
                }
            }
        }
        
        println!("  ✓ Reference mappings built: {} references found", reference_count);
    }
    
    fn extract_references(&self, elements: &[DefElement]) -> (Vec<String>, Vec<String>) {
        let mut references = Vec::new();
        let mut code_references = Vec::new();
        
        self.extract_references_recursive(elements, &mut references, &mut code_references);
        
        // Deduplicate references
        references.sort();
        references.dedup();
        code_references.sort();
        code_references.dedup();
        
        (references, code_references)
    }
    
    fn extract_references_recursive(&self, elements: &[DefElement], references: &mut Vec<String>, code_references: &mut Vec<String>) {
        for element in elements {
            // Check element name - could be a def reference (like <Muffalo>0.1</Muffalo>)
            if element.name != "defName" && element.name != "li" {
                references.push(element.name.clone());
            }
            
            // Check element content - any element could contain a def reference
            if let Some(content) = &element.content {
                // Skip if it's the defName element itself
                if element.name != "defName" {
                    references.push(content.clone());
                }
            }
            
            // Check attributes
            for (key, value) in &element.attributes {
                if key == "Class" {
                    // C# class references
                    code_references.push(value.clone());
                } else {
                    // Other attributes might be def references
                    references.push(value.clone());
                }
            }
            
            // Recursively check children
            self.extract_references_recursive(&element.children, references, code_references);
        }
    }
}

struct DatasetGenerator {
    defs: Vec<RimWorldDef>,
    rimworld_path: String,
}

impl DatasetGenerator {
    fn new(defs: Vec<RimWorldDef>, rimworld_path: String) -> Result<Self> {
        Ok(Self { defs, rimworld_path })
    }

    fn read_game_version(&self) -> String {
        let version_path = Path::new(&self.rimworld_path).join("Version.txt");
        match fs::read_to_string(version_path) {
            Ok(content) => content.trim().to_string(),
            Err(_) => "Unknown".to_string(),
        }
    }

    fn generate_dataset_file(&self) -> Result<()> {
        println!("\nGenerating compressed dataset file...");
        
        // Create compressed data
        let compressed_data = self.create_compressed_data()?;
        println!("  ✓ Data compressed: {} bytes", compressed_data.len());
        
        // Write to static dataset file
        let dataset_path = "dataset.json.zstd";
        fs::write(dataset_path, &compressed_data)?;
        println!("  ✓ Dataset file written: {} ({} bytes)", dataset_path, compressed_data.len());
        
        Ok(())
    }
    
    fn create_compressed_data(&self) -> Result<Vec<u8>> {
        println!("    Processing definitions for compression...");
        
        // Create a simplified data structure for the frontend
        let mut categories: HashMap<String, Vec<&RimWorldDef>> = HashMap::new();
        for def in &self.defs {
            categories.entry(def.def_type.clone()).or_insert_with(Vec::new).push(def);
        }
        
        let mut category_data = Vec::new();
        for (name, defs) in categories {
            let mut sorted_defs = defs.clone();
            sorted_defs.sort_by(|a, b| a.def_name.cmp(&b.def_name));
            
            category_data.push(json!({
                "name": name,
                "display_name": self.format_category_name(&name),
                "count": sorted_defs.len(),
                "definitions": sorted_defs.iter().map(|def| {
                    json!({
                        "def_name": def.def_name,
                        "def_type": def.def_type,
                        "label": def.label,
                        "description": def.description,
                        "parent_name": def.parent_name,
                        "is_abstract": def.is_abstract,
                        "file_path": def.file_path,
                        "tags": def.tags,
                        "elements": self.flatten_elements(&def.elements),
                        "references_out": def.references_out,
                        "references_in": def.references_in,
                        "code_references": def.code_references,
                        "raw_xml": def.raw_xml,
                        "extension": def.extension
                    })
                }).collect::<Vec<_>>()
            }));
        }
        
        category_data.sort_by(|a, b| a["display_name"].as_str().cmp(&b["display_name"].as_str()));
        
        let stats = self.get_stats();
        
        let data = json!({
            "categories": category_data,
            "stats": {
                "total_defs": stats.total_defs,
                "total_categories": stats.total_categories,
                "total_files": stats.total_files,
                "game_version": stats.game_version,
                "generated_at": stats.generated_at
            }
        });
        
        let json_data = serde_json::to_string(&data)?;
        println!("      JSON size: {} bytes", json_data.len());
        
        // Compress with zstd using manual encoder with long distance matching
        let mut encoder = zstd::Encoder::new(Vec::new(), 19)?;
        encoder.long_distance_matching(true)?;
        encoder.multithread(16)?;
        encoder.write_all(json_data.as_bytes())?;
        let compressed = encoder.finish()?;
        
        println!("      Compressed size: {} bytes ({}% reduction)", 
            compressed.len(), 
            100 - (compressed.len() * 100 / json_data.len()));
        
        // Return raw compressed bytes
        Ok(compressed)
    }

    
    fn format_category_name(&self, name: &str) -> String {
        // Convert camelCase to Title Case
        let mut result = String::new();
        let mut prev_lower = false;
        
        for (i, ch) in name.chars().enumerate() {
            if i == 0 {
                result.push(ch.to_uppercase().next().unwrap());
            } else if ch.is_uppercase() && prev_lower {
                result.push(' ');
                result.push(ch);
            } else {
                result.push(ch);
            }
            prev_lower = ch.is_lowercase();
        }
        
        result
    }

    fn flatten_elements(&self, elements: &[DefElement]) -> Vec<serde_json::Value> {
        let mut result = Vec::new();
        
        for element in elements.iter().take(15) {
            self.flatten_element_recursive(element, &mut result, 0);
            if result.len() >= 50 {
                break;
            }
        }

        result
    }
    
    fn flatten_element_recursive(&self, element: &DefElement, result: &mut Vec<serde_json::Value>, depth: usize) {
        if depth > 3 || result.len() >= 50 {
            return;
        }
        
        let mut attributes_str = String::new();
        if !element.attributes.is_empty() {
            attributes_str = element.attributes.iter()
                .map(|(k, v)| format!("{}=\"{}\"", k, v))
                .collect::<Vec<_>>()
                .join(" ");
        }
        
        result.push(json!({
            "name": element.name,
            "content": element.content,
            "depth": depth * 20,
            "attributes": attributes_str,
            "has_children": !element.children.is_empty()
        }));
        
        for child in element.children.iter().take(5) {
            self.flatten_element_recursive(child, result, depth + 1);
        }
    }

    fn get_stats(&self) -> Stats {
        let mut files = std::collections::HashSet::new();
        let mut categories = std::collections::HashSet::new();
        
        for def in &self.defs {
            files.insert(&def.file_path);
            categories.insert(&def.def_type);
        }

        let game_version = self.read_game_version();
        let generated_at = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();

        Stats {
            total_defs: self.defs.len(),
            total_categories: categories.len(),
            total_files: files.len(),
            game_version,
            generated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct Stats {
    total_defs: usize,
    total_categories: usize,
    total_files: usize,
    game_version: String,
    generated_at: String,
}

fn main() -> Result<()> {
    println!("RimWorld XML Documentation Generator");
    println!("====================================");
    
    let matches = Command::new("rimworld-xml")
        .about("Generate compressed HTML documentation for RimWorld XML definitions")
        .arg(Arg::new("rimworld-path")
            .short('p')
            .long("path")
            .value_name("PATH")
            .help("Path to RimWorld base installation directory")
            .required(true))
        .get_matches();

    let rimworld_path = matches.get_one::<String>("rimworld-path").unwrap();

    println!("\nConfiguration:");
    println!("  RimWorld path: {}", rimworld_path);

    // Verify paths exist
    if !Path::new(rimworld_path).exists() {
        return Err(anyhow::anyhow!("RimWorld path does not exist: {}", rimworld_path));
    }
    
    let data_path = Path::new(rimworld_path).join("Data");
    if !data_path.exists() {
        return Err(anyhow::anyhow!("Data directory not found: {}", data_path.display()));
    }
    
    println!("  ✓ Paths validated");
    
    let mut parser = DefParser::new(rimworld_path.clone());
    parser.scan_defs_directory()?;
    
    println!("\nCreating HTML generator...");
    let generator = DatasetGenerator::new(parser.parsed_defs, rimworld_path.clone())?;
    println!("  ✓ Generator initialized");

    generator.generate_dataset_file()?;
    
    println!("\n✓ Documentation generation complete!");
    Ok(())
}
