# zcatr

A modern command-line tool for viewing content and information from compressed files and archives. Similar to the Unix `zcat` command but with enhanced capabilities and format support.

## Features

- **Multi-format Support**: Read and display content from various compressed formats:
  - ZIP archives (.zip)
  - TAR archives (.tar)
  - GZIP compressed files (.gz)
  - BZIP2 compressed files (.bz2)
  - Combined formats (TAR+GZIP, TAR+BZIP2)

- **Smart Content Handling**:
  - Automatic file type detection using magic bytes
  - Memory-efficient buffered reading
  - Proper display of text-based formats (plain text, markdown, CSV, JSON, XML)
  - Preview unavailable message for binary content
  - Directory entry filtering

- **Two Operating Modes**:
  - Content display (default)
  - Information listing (--list)

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/yourusername/zcatr.git
cd zcatr

# Build with cargo
cargo build --release

# Optional: Install globally
cargo install --path .
```

### Prerequisites

- Rust 1.70 or higher
- Cargo (Rust's package manager)

## Usage

### Basic Usage

Display content of a compressed file:
```bash
zcatr file.gz
```

List information about files in an archive:
```bash
zcatr --list archive.zip
```

Process multiple files:
```bash
zcatr file1.gz file2.tar.gz
```

### Examples

1. View content of a gzipped log file:
```bash
zcatr logs/application.log.gz
```

2. List contents of a ZIP archive:
```bash
zcatr --list documents.zip
```

Example output:
```
📂 "documents.zip"
├── File: document.txt
|   Size: 1.24 KB
├── File: data.json
|   Size: 2.5 MB
```

3. View content from a tar.gz archive:
```bash
zcatr dummy.txt.gz
```

Example output:
```
📄 Content from "dummy.txt":
────────────────────────────────────────
This is a dummy text

────────────────────────────────────────
```

### Supported File Types

For content display:
- Plain text files (.txt)
- Markdown files (.md)
- CSV files (.csv)
- JSON files (.json)
- XML files (.xml)

Binary files will display a "Preview not available in console" message.

## License

[MIT License](LICENSE)

## Credits

Built with:
- [clap](https://crates.io/crates/clap) - Command line argument parsing
- [zip](https://crates.io/crates/zip) - ZIP archive handling
- [flate2](https://crates.io/crates/flate2) - GZIP compression
- [tar](https://crates.io/crates/tar) - TAR archive handling
- [bzip2](https://crates.io/crates/bzip2) - BZIP2 compression
- [infer](https://crates.io/crates/infer) - File type detection
- [thiserror](https://crates.io/crates/thiserror) - Error handling