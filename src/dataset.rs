use crate::model::{DefinitionReference, DefinitionSummary, RimWorldDef};
use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;

pub struct DatasetGenerator {
    definitions: Vec<RimWorldDef>,
    rimworld_path: String,
}

impl DatasetGenerator {
    pub fn new(definitions: Vec<RimWorldDef>, rimworld_path: String) -> Result<Self> {
        Ok(Self {
            definitions,
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

    pub(crate) fn generate_dataset_file(&self, output_dir: &Path) -> Result<()> {
        println!("\nGenerating compressed dataset file...");

        let compressed_data = self.create_compressed_data()?;
        println!("  ✓ Data compressed: {} bytes", compressed_data.len());

        let dataset_path = output_dir.join("dataset.json.zstd");
        fs::write(&dataset_path, &compressed_data)?;
        println!(
            "  ✓ Dataset file written: {} ({} bytes)",
            dataset_path.display(),
            compressed_data.len()
        );

        Ok(())
    }

    fn create_compressed_data(&self) -> Result<Vec<u8>> {
        println!("    Processing definitions for compression...");

        let mut categories: HashMap<String, Vec<&RimWorldDef>> = HashMap::new();
        for definition in &self.definitions {
            categories
                .entry(definition.def_type.clone())
                .or_default()
                .push(definition);
        }

        let mut category_data = Vec::new();
        for (name, definitions) in categories {
            let mut sorted_definitions = definitions.clone();
            sorted_definitions.sort_by(|left, right| {
                left.def_name
                    .cmp(&right.def_name)
                    .then_with(|| left.id.cmp(&right.id))
            });

            category_data.push(json!({
                "name": name,
                "display_name": self.format_category_name(&name),
                "definitions": sorted_definitions.iter().map(|definition| {
                    DefinitionPayload::from(*definition)
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

    fn get_stats(&self) -> Stats {
        let mut files = std::collections::HashSet::new();
        let mut categories = std::collections::HashSet::new();

        for definition in &self.definitions {
            files.insert(&definition.file_path);
            categories.insert(&definition.def_type);
        }

        Stats {
            total_defs: self.definitions.len(),
            total_categories: categories.len(),
            total_files: files.len(),
            game_version: self.read_game_version(),
            generated_at: Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        }
    }
}

#[derive(Serialize)]
struct DefinitionPayload<'a> {
    id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    def_name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inheritance_name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_name: Option<&'a str>,
    #[serde(skip_serializing_if = "is_false")]
    is_abstract: bool,
    file_path: &'a str,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    tags: &'a [String],
    #[serde(skip_serializing_if = "<[DefinitionReference]>::is_empty")]
    references_out: &'a [DefinitionReference],
    #[serde(skip_serializing_if = "<[DefinitionSummary]>::is_empty")]
    references_in: &'a [DefinitionSummary],
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    code_references: &'a [String],
    raw_xml: &'a str,
}

impl<'a> From<&'a RimWorldDef> for DefinitionPayload<'a> {
    fn from(definition: &'a RimWorldDef) -> Self {
        Self {
            id: &definition.id,
            def_name: definition.def_name.as_deref(),
            inheritance_name: definition.inheritance_name.as_deref(),
            label: definition.label.as_deref(),
            description: definition.description.as_deref(),
            parent_name: definition.parent_name.as_deref(),
            is_abstract: definition.is_abstract,
            file_path: &definition.file_path,
            tags: &definition.tags,
            references_out: &definition.references_out,
            references_in: &definition.references_in,
            code_references: &definition.code_references,
            raw_xml: &definition.raw_xml,
        }
    }
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Debug, Clone, Serialize)]
struct Stats {
    total_defs: usize,
    total_categories: usize,
    total_files: usize,
    game_version: String,
    generated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition(name: &str, def_type: &str, file_path: &str) -> RimWorldDef {
        RimWorldDef {
            id: format!("{file_path}#{name}"),
            def_name: Some(name.to_string()),
            inheritance_name: None,
            class_name: None,
            def_type: def_type.to_string(),
            label: None,
            description: None,
            parent_name: None,
            is_abstract: false,
            elements: Vec::new(),
            file_path: file_path.to_string(),
            tags: Vec::new(),
            stats: None,
            references_out: Vec::new(),
            references_in: Vec::new(),
            code_references: Vec::new(),
            raw_xml: format!("<{def_type}><defName>{name}</defName></{def_type}>"),
            extension: "Core".to_string(),
        }
    }

    #[test]
    fn compressed_dataset_preserves_the_sorted_frontend_contract() -> Result<()> {
        let mut unnamed = definition("Unused", "SongDef", "Data/Core/Songs.xml");
        unnamed.id = "Data/Core/Songs.xml#0".to_string();
        unnamed.def_name = None;
        unnamed.inheritance_name = Some("SongTemplate".to_string());
        unnamed.class_name = Some("Verse.SpecialSongDef".to_string());
        let mut abstract_definition = definition("Beta", "AbilityDef", "Data/Core/Abilities.xml");
        abstract_definition.is_abstract = true;
        let definitions = vec![
            definition("Zeta", "ThingDef", "Data/Core/Things.xml"),
            abstract_definition,
            definition("Alpha", "AbilityDef", "Data/Core/Abilities.xml"),
            unnamed,
        ];
        let generator = DatasetGenerator::new(definitions, "/missing/rimworld".to_string())?;

        let compressed = generator.create_compressed_data()?;
        let json = zstd::decode_all(compressed.as_slice())?;
        let data: serde_json::Value = serde_json::from_slice(&json)?;

        let categories = data["categories"].as_array().unwrap();
        assert_eq!(categories[0]["name"], "AbilityDef");
        assert_eq!(categories[1]["name"], "SongDef");
        assert_eq!(categories[2]["name"], "ThingDef");
        assert_eq!(categories[0]["definitions"][0]["def_name"], "Alpha");
        assert_eq!(categories[0]["definitions"][1]["def_name"], "Beta");
        assert_eq!(categories[0]["definitions"][1]["is_abstract"], true);
        assert!(categories[0].get("count").is_none());
        assert!(categories[1]["definitions"][0].get("def_name").is_none());
        assert_eq!(
            categories[1]["definitions"][0]["inheritance_name"],
            "SongTemplate"
        );
        assert_eq!(
            categories[1]["definitions"][0]["id"],
            "Data/Core/Songs.xml#0"
        );
        for category in categories {
            for definition in category["definitions"].as_array().unwrap() {
                assert!(definition.get("class_name").is_none());
                assert!(definition.get("def_type").is_none());
                assert!(definition.get("elements").is_none());
                assert!(definition.get("extension").is_none());
                if definition["def_name"] != "Beta" {
                    assert!(definition.get("is_abstract").is_none());
                }
            }
        }
        assert_eq!(data["stats"]["total_defs"], 4);
        assert_eq!(data["stats"]["total_categories"], 3);
        assert_eq!(data["stats"]["total_files"], 3);
        assert_eq!(data["stats"]["game_version"], "Unknown");

        Ok(())
    }
}
