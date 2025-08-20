# RimWorld XML Documentation Generator

A Rust-based tool that parses RimWorld's XML definition files and generates a compressed, interactive HTML documentation site.

Fancy name pending.

## Features

Automatically discovers and parses all XML definition files from RimWorld's Data directory

Builds bidirectional reference maps between definitions to show relationships

Vue.js-powered frontend with search, filtering, and category-based navigation

Uses zstd compression to create compact compressed datasets.

## Live Version

There is a hosted version available here: https://rimworld.lattemacchiato.dev/

## Installation

Ensure you have Rust installed, then clone the repository:

```bash
git clone https://github.com/PrivateGER/rimworld-def-viewer
cd rimworld-def-viewer
cargo build --release
```

## Usage

Run the tool with your RimWorld installation path:

```bash
cargo run --release -- --path "/path/to/RimWorld"
```

Do not run in debug mode unless you have a reason to. zstd compression is VERY slow when using an unoptimized build.

Example for a typical Steam installation:
```bash
cargo run --release -- --path "C:\Program Files (x86)\Steam\steamapps\common\Rimworld"
```

This project is for educational and documentation purposes. RimWorld content belongs to Ludeon Studios. 

No Rimworld content is included in this repository. This software is not official and is not endorsed by Ludeon.
