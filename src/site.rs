use crate::dataset::DatasetGenerator;
use anyhow::Result;
use std::fs;
use std::path::Path;

const STATIC_ASSETS: [(&str, &[u8]); 3] = [
    ("index.html", include_bytes!("../index.html")),
    ("app.js", include_bytes!("../app.js")),
    ("styles.css", include_bytes!("../styles.css")),
];

pub fn generate_site(generator: &DatasetGenerator, output_dir: &Path) -> Result<()> {
    fs::create_dir_all(output_dir)?;

    for (file_name, contents) in STATIC_ASSETS {
        fs::write(output_dir.join(file_name), contents)?;
    }

    generator.generate_dataset_files(output_dir)
}

#[cfg(test)]
mod tests {
    use super::generate_site;
    use crate::dataset::DatasetGenerator;
    use anyhow::Result;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn generated_site_contains_the_current_frontend_and_dataset() -> Result<()> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let output_dir = std::env::temp_dir().join(format!(
            "rimworld-def-viewer-site-{}-{nonce}",
            std::process::id()
        ));
        let generator = DatasetGenerator::new(Vec::new(), "/missing/rimworld".to_string())?;

        generate_site(&generator, &output_dir)?;

        assert_eq!(
            fs::read(output_dir.join("index.html"))?,
            include_bytes!("../index.html")
        );
        assert_eq!(
            fs::read(output_dir.join("app.js"))?,
            include_bytes!("../app.js")
        );
        assert_eq!(
            fs::read(output_dir.join("styles.css"))?,
            include_bytes!("../styles.css")
        );

        let index = fs::read_to_string(output_dir.join("index.html"))?;
        let app = fs::read_to_string(output_dir.join("app.js"))?;
        assert!(app.contains("def.def_type = category.name"));
        assert!(app.contains("def.extension = extensionFromFilePath(def.file_path)"));
        assert!(app.contains("target.references_in.push(def.id)"));
        assert!(app.contains("loadCompressedJson('raw-xml.json.zstd', 'raw XML')"));
        assert!(app.contains("this.rawXmlLoadPromise ||= loadRawXmlFromFile()"));
        assert!(app.contains("SEARCH_RESULT_BATCH_SIZE = 50"));
        assert!(index.contains("v-for=\"sourceId in def.references_in\""));
        assert!(index.contains("definitionDisplayName(definitionById(sourceId))"));
        assert!(index.contains("class=\"search-results-list\""));
        assert!(index.contains("class=\"xml-drawer\""));

        let compressed = fs::read(output_dir.join("dataset.json.zstd"))?;
        let json = zstd::decode_all(compressed.as_slice())?;
        let data: serde_json::Value = serde_json::from_slice(&json)?;
        assert!(data["categories"].is_array());
        assert!(data["stats"].is_object());

        let compressed_raw_xml = fs::read(output_dir.join("raw-xml.json.zstd"))?;
        let raw_xml_json = zstd::decode_all(compressed_raw_xml.as_slice())?;
        let raw_xml: serde_json::Value = serde_json::from_slice(&raw_xml_json)?;
        assert!(raw_xml.is_object());

        fs::remove_dir_all(output_dir)?;
        Ok(())
    }
}
