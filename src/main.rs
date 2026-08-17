use anyhow::Result;
use clap::{Arg, Command};
use rimworld_def_viewer::dataset::DatasetGenerator;
use rimworld_def_viewer::parser::DefParser;
use rimworld_def_viewer::references::build_reference_mappings;
use std::path::Path;

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

    let mut definitions = parser.into_defs();
    build_reference_mappings(&mut definitions);

    println!("\nCreating HTML generator...");
    let generator = DatasetGenerator::new(definitions, rimworld_path.clone())?;
    println!("  ✓ Generator initialized");

    generator.generate_dataset_file()?;

    println!("\n✓ Documentation generation complete!");
    Ok(())
}
