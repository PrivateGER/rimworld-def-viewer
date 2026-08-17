use anyhow::Result;
use chrono::Utc;
use clap::{Arg, Command};
use rimworld_def_viewer::model::RimWorldDef;
use rimworld_def_viewer::parser::DefParser;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;

struct DatasetGenerator {
    defs: Vec<RimWorldDef>,
    rimworld_path: String,
}

impl DatasetGenerator {
    fn new(defs: Vec<RimWorldDef>, rimworld_path: String) -> Result<Self> {
        Ok(Self {
            defs,
            rimworld_path,
        })
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

        let compressed_data = self.create_compressed_data()?;
        println!("  ✓ Data compressed: {} bytes", compressed_data.len());

        let dataset_path = "dataset.json.zstd";
        fs::write(dataset_path, &compressed_data)?;
        println!(
            "  ✓ Dataset file written: {} ({} bytes)",
            dataset_path,
            compressed_data.len()
        );

        Ok(())
    }

    fn create_compressed_data(&self) -> Result<Vec<u8>> {
        println!("    Processing definitions for compression...");

        let mut categories: HashMap<String, Vec<&RimWorldDef>> = HashMap::new();
        for definition in &self.defs {
            categories
                .entry(definition.def_type.clone())
                .or_default()
                .push(definition);
        }

        let mut category_data = Vec::new();
        for (name, definitions) in categories {
            let mut sorted_definitions = definitions.clone();
            sorted_definitions.sort_by(|left, right| left.def_name.cmp(&right.def_name));

            category_data.push(json!({
                "name": name,
                "display_name": self.format_category_name(&name),
                "count": sorted_definitions.len(),
                "definitions": sorted_definitions.iter().map(|definition| {
                    json!({
                        "def_name": definition.def_name,
                        "def_type": definition.def_type,
                        "label": definition.label,
                        "description": definition.description,
                        "parent_name": definition.parent_name,
                        "is_abstract": definition.is_abstract,
                        "file_path": definition.file_path,
                        "tags": definition.tags,
                        "elements": self.flatten_elements(&definition.elements),
                        "references_out": definition.references_out,
                        "references_in": definition.references_in,
                        "code_references": definition.code_references,
                        "raw_xml": definition.raw_xml,
                        "extension": definition.extension
                    })
                }).collect::<Vec<_>>()
            }));
        }

        category_data.sort_by(|left, right| {
            left["display_name"]
                .as_str()
                .cmp(&right["display_name"].as_str())
        });

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

        let mut encoder = zstd::Encoder::new(Vec::new(), 19)?;
        encoder.long_distance_matching(true)?;
        encoder.multithread(16)?;
        encoder.write_all(json_data.as_bytes())?;
        let compressed = encoder.finish()?;

        println!(
            "      Compressed size: {} bytes ({}% reduction)",
            compressed.len(),
            100 - (compressed.len() * 100 / json_data.len())
        );

        Ok(compressed)
    }

    fn format_category_name(&self, name: &str) -> String {
        let mut result = String::new();
        let mut previous_lowercase = false;

        for (index, character) in name.chars().enumerate() {
            if index == 0 {
                result.push(character.to_uppercase().next().unwrap());
            } else if character.is_uppercase() && previous_lowercase {
                result.push(' ');
                result.push(character);
            } else {
                result.push(character);
            }
            previous_lowercase = character.is_lowercase();
        }

        result
    }

    fn flatten_elements(
        &self,
        elements: &[rimworld_def_viewer::model::DefElement],
    ) -> Vec<serde_json::Value> {
        let mut result = Vec::new();

        for element in elements.iter().take(15) {
            self.flatten_element_recursive(element, &mut result, 0);
            if result.len() >= 50 {
                break;
            }
        }

        result
    }

    fn flatten_element_recursive(
        &self,
        element: &rimworld_def_viewer::model::DefElement,
        result: &mut Vec<serde_json::Value>,
        depth: usize,
    ) {
        if depth > 3 || result.len() >= 50 {
            return;
        }

        let mut attributes = String::new();
        if !element.attributes.is_empty() {
            attributes = element
                .attributes
                .iter()
                .map(|(key, value)| format!("{}=\"{}\"", key, value))
                .collect::<Vec<_>>()
                .join(" ");
        }

        result.push(json!({
            "name": element.name,
            "content": element.content,
            "depth": depth * 20,
            "attributes": attributes,
            "has_children": !element.children.is_empty()
        }));

        for child in element.children.iter().take(5) {
            self.flatten_element_recursive(child, result, depth + 1);
        }
    }

    fn get_stats(&self) -> Stats {
        let mut files = std::collections::HashSet::new();
        let mut categories = std::collections::HashSet::new();

        for definition in &self.defs {
            files.insert(&definition.file_path);
            categories.insert(&definition.def_type);
        }

        Stats {
            total_defs: self.defs.len(),
            total_categories: categories.len(),
            total_files: files.len(),
            game_version: self.read_game_version(),
            generated_at: Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
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
        .arg(
            Arg::new("rimworld-path")
                .short('p')
                .long("path")
                .value_name("PATH")
                .help("Path to RimWorld base installation directory")
                .required(true),
        )
        .get_matches();

    let rimworld_path = matches.get_one::<String>("rimworld-path").unwrap();

    println!("\nConfiguration:");
    println!("  RimWorld path: {}", rimworld_path);

    if !Path::new(rimworld_path).exists() {
        return Err(anyhow::anyhow!(
            "RimWorld path does not exist: {}",
            rimworld_path
        ));
    }

    let data_path = Path::new(rimworld_path).join("Data");
    if !data_path.exists() {
        return Err(anyhow::anyhow!(
            "Data directory not found: {}",
            data_path.display()
        ));
    }

    println!("  ✓ Paths validated");

    let mut parser = DefParser::new(rimworld_path.clone());
    parser.scan_defs_directory()?;

    println!("\nCreating HTML generator...");
    let generator = DatasetGenerator::new(parser.into_defs(), rimworld_path.clone())?;
    println!("  ✓ Generator initialized");

    generator.generate_dataset_file()?;

    println!("\n✓ Documentation generation complete!");
    Ok(())
}
