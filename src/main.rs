use std::{
    fs::File,
    io::{self, BufReader, Read},
    path::PathBuf,
    sync::OnceLock,
};

use clap::Parser;
use flate2::read::GzDecoder;
use infer::Type;
use thiserror::Error;

#[cfg(target_os = "windows")]
const LINE_ENDING: &str = "\r\n";

#[cfg(not(target_os = "windows"))]
const LINE_ENDING: &str = "\n";

const MAGIC_BYTES_SIZE: usize = 512;
const BUFFER_SIZE: usize = 8192;



#[derive(Error, Debug)]
enum ZcatError {
    #[error("I/O error: {0}")]
    IoError(#[from] io::Error),
    #[error("ZIP error: {0}")]
    ZipError(#[from] zip::result::ZipError),
}

#[derive(Parser, Debug)]
#[command(
    version = "0.1.0",
    about = "A tool to view content and information from compressed files and archives",
    long_about = "zcatr is a command-line tool that displays the content of compressed files and archives. \
    Similar to the Unix zcat command, it allows you to view file contents without manual decompression. \
    It supports viewing content from ZIP, TAR, GZIP, and BZIP2 files, with additional capabilities to display \
    file information such as sizes and names."
)]
struct Args {
    #[arg(
        short,
        long,
        action,
        help = "Show archive information instead of content",
        long_help = "Instead of displaying file contents, show information about the files \
        in the archive including their names and sizes. This is useful for previewing \
        what's inside an archive without viewing its contents."
    )]
    list: bool,

    #[arg(
        short,
        long,
        action,
        help = "When printing the content of the file(s), the header and the footer are not displayed!"
    )]
    no_styling: bool,

    #[arg(
        required = true,
        help = "Files to read",
        value_name = "FILES",
        long_help = "One or more files to process. Supported formats:\n\
        - ZIP archives (.zip)\n\
        - TAR archives (.tar)\n\
        - GZIP compressed files (.gz)\n\
        - BZIP2 compressed files (.bz2)\n\
        - TAR+GZIP archives (.tar.gz, .tgz)\n\
        - TAR+BZIP2 archives (.tar.bz2)"
    )]
    files: Vec<PathBuf>,
}

#[derive(Debug)]
struct Context {
    with_styling: bool,
}

static CONTEXT: OnceLock<Context> = OnceLock::new();

/// Determines the MIME type of file using file signature detection.
///
/// This function examines the file's content to identify its type based on magic bytes,
/// rather than relying on file extensions. It uses the `infer` crate for detection.
///
/// # Arguments
/// * `path` - A reference to a PathBuf containing the path to the file to analyze
///
/// # Returns
/// * `Result<Option<Type>, Box<dyn Error>>` - Returns:
///   * `Ok(Some(Type))` - If the file type was successfully identified
///   * `Ok(None)` - If the file type could not be determined
///   * `Err(_)` - If there was an error accessing or reading the file
#[inline]
fn infer_file_type(path: &PathBuf) -> Result<Option<Type>, ZcatError> {
    let mime_type = infer::get_from_path(path.as_path())?;
    Ok(mime_type)
}

/// Formats file size in human-readable format
///
/// # Arguments
/// * `bytes` - Size in bytes to format
///
/// # Returns
/// A string representation of the size with appropriate unit
#[inline]
fn format_file_size(bytes: usize) -> String {
    if bytes == 0 {
        return String::from("0 Bytes");
    }

    const UNITS: [&str; 4] = ["Bytes", "KB", "MB", "GB"];

    let exp = (bytes as f64).ln() / 1024_f64.ln();
    let i = exp.floor() as usize;

    if i >= UNITS.len() {
        let value = bytes as f64 / 1024_f64.powi(3);
        return format!("{:.2} {}", value, UNITS[3]);
    }

    if i == 0 {
        // For bytes, show without decimal places
        return format!("{} {}", bytes, UNITS[0]);
    }

    let value = bytes as f64 / 1024_f64.powi(i as i32);
    format!("{:.2} {}", value, UNITS[i])
}

/// Displays formatted information about a file in a tree-like structure.
///
/// Prints the filename and its size in a human-readable format using
/// a hierarchical display style. The size is automatically converted to
/// appropriate units (Bytes, KB, MB, GB).
///
/// # Arguments
/// * `file_name` - The name of the file to display
/// * `file_size` - The size of the file in bytes
#[inline]
fn display_file_info(file_name: &str, file_size: usize) {
    println!(
        "|
├── File: {file_name}
|   Size: {}",
        format_file_size(file_size)
    );
}

/// Displays the content of a file with formatted header and footer.
///
/// This function reads and displays file content with a few key features:
/// - Checks the first 512 bytes to determine if the content is displayable
/// - Only displays text-based content (plain text, markdown, CSV, JSON, XML)
/// - Uses buffered reading for memory efficiency
/// - Includes formatted header and footer for visual separation
///
/// # Arguments
/// * `file_name` - The name of the file being displayed
/// * `reader` - Any type implementing the `Read` trait that provides the file content
///
/// # Output Format
/// ```text
/// 📄 Content from "example.txt":
/// ────────────────────────────────
/// [actual file content here]
/// ────────────────────────────────
/// ```
fn display_file_content<R>(file_name: &str, mut reader: R)
where
    R: Read,
{
    let context = CONTEXT.get().unwrap();
    if context.with_styling {
        println!("📄 Content from \"{}\":", file_name);
        println!("{}", "─".repeat(40));
    }

    let mut buffer = [0u8; BUFFER_SIZE];
    let mut read_bytes = reader.read(&mut buffer[..MAGIC_BYTES_SIZE]).unwrap();
    let magic_bytes = &buffer[..read_bytes];

    let mut printing_handler = move || {
        let mut cursor = io::Cursor::new(magic_bytes);
        read_bytes = cursor.read(&mut buffer).unwrap();



        // Stream the content
        loop {
            // Replacing cursor to avoid a UTF8 parsing error.
            let mut right_ptr = read_bytes - 1;
            let mut inspected_byte = 0;
            loop {
                inspected_byte = buffer[right_ptr];
                if inspected_byte >> 7 == 0x0 || inspected_byte >> 5 == 0x6 || inspected_byte >> 4 == 0xE || inspected_byte >> 3 == 30 {
                    break;
                }

                if right_ptr == 0 {
                    return;
                }

                right_ptr -= 1;
            }

            let range  = match inspected_byte >> 7 == 0 {
                true => ..right_ptr+1,
                false => ..right_ptr
            };

            if let Ok(text) = std::str::from_utf8(&buffer[range]) {
                print!("{}", text);
            } else {
                let str_lossy = String::from_utf8_lossy(&buffer[range]);
                let filtered = str_lossy.split(LINE_ENDING).filter(|s| std::str::from_utf8(s.as_bytes()).is_ok()).collect::<Vec<&str>>().join(LINE_ENDING);
                print!("{}", filtered);
            }

            let mut offset = 0;

            if inspected_byte >> 7 != 0 {
                buffer.copy_within(right_ptr..read_bytes, 0);
                offset = read_bytes - right_ptr;
            }

            read_bytes = reader.read(&mut buffer[offset..]).unwrap_or(0);

            if read_bytes == 0 {
                break;
            }

            read_bytes += offset;
        }
    };

    match infer::get(magic_bytes) {
        Some(mime_type) => match mime_type.mime_type() {
            "text/plain" | "text/markdown" | "text/csv" | "application/json"
            | "application/xml" | "text/xml" => {
                printing_handler();
            }
            _ => {
                print!("Preview not available in console.")
            }
        },
        None => {
            printing_handler();
        }
    }

    if context.with_styling {
        println!("{}{}", LINE_ENDING, "─".repeat(40));
    }
}

/// Prints information about a single entry within a TAR archive.
///
/// Takes a TAR entry and displays its path and size in a tree-like structure.
/// This function unwraps the entry's path and size, then delegates the actual
/// display formatting to `display_file_info`.
///
/// # Arguments
/// * `entry` - A TAR entry implementing the `Read` trait
fn print_tar_entry_info<R>(entry: tar::Entry<R>)
where
    R: Read,
{
    let path = entry.path().unwrap().into_owned();
    let size = entry.header().size().unwrap();
    display_file_info(path.to_str().unwrap(), size as usize);
}

/// Displays the content of a single entry within a TAR archive.
///
/// Takes a TAR entry and displays its content. The function extracts the entry's path
/// and passes the entry itself as a reader to `display_file_content` for content display.
///
/// # Arguments
/// * `entry` - A TAR entry implementing the `Read` trait
fn print_tar_entry_content<R>(entry: tar::Entry<R>)
where
    R: Read,
{
    let path = entry.path().unwrap().into_owned();
    display_file_content(path.to_str().unwrap(), entry);
}

/// Applies a handler function to each file entry in a TAR archive stream.
///
/// This function iterates through all entries in a TAR archive, skipping:
/// - Directory entries
/// - macOS specific hidden files (entries starting with "._")
///
/// # Arguments
/// * `archive` - A TAR archive reader
/// * `handler` - A function that processes each entry (e.g., displaying content or info)
///
/// # Returns
/// * `Ok(())` if all operations succeeded
/// * `Err(ZcatError)` if any operation fails
///
/// # Errors
/// This function can return:
/// * `ZcatError::TarError` - If there's an error reading entries from the archive
fn handle_tar_entries_from_tar_archive<R, F>(
    mut archive: tar::Archive<R>,
    handler: F,
) -> Result<(), ZcatError>
where
    R: Read,
    F: Fn(tar::Entry<R>) -> (),
{
    for entry in archive.entries()? {
        let entry = entry?;
        let entry_header = entry.header();

        if entry_header.entry_type().is_dir() {
            continue;
        }

        handler(entry);
    }
    Ok(())
}

/// Applies a handler function to each file entry in a TAR archive file.
///
/// This is a convenience wrapper around `handle_tar_entries_from_tar_archive` that handles
/// opening the file and creating the archive reader.
///
/// # Arguments
/// * `path` - Path to the TAR archive file
/// * `handler` - A function that processes each entry (e.g., displaying content or info)
///
/// # Returns
/// * `Ok(())` if all operations succeeded
/// * `Err(ZcatError)` if any operation fails
///
/// # Errors
/// This function can return:
/// * `ZcatError::IoError` - If there's an error opening or reading the file
/// * `ZcatError::TarError` - If there's an error processing the TAR archive
fn handle_tar_entries<F>(path: &PathBuf, handler: F) -> Result<(), ZcatError>
where
    F: Fn(tar::Entry<File>) -> (),
{
    let file = File::open(path)?;
    let archive = tar::Archive::new(file);
    handle_tar_entries_from_tar_archive(archive, handler)?;
    Ok(())
}

/// Displays formatted information about a single file within a ZIP archive.
///
/// Takes a ZIP file entry and displays its name and size in a tree-like structure
/// using the `display_file_info` function.
///
/// # Arguments
/// * `file` - A ZIP file entry to display information about
fn print_zip_entry_info(file: zip::read::ZipFile) {
    display_file_info(file.name(), file.size() as usize);
}

/// Displays the content of a single file within a ZIP archive.
///
/// Takes a ZIP file entry and displays its content using the `display_file_content` function.
/// Only text-based content (plain text, markdown, CSV, JSON, XML) will be displayed.
///
/// # Arguments
/// * `file` - A ZIP file entry to display the content of
fn print_zip_entry_content(file: zip::read::ZipFile) {
    let path = file.name().to_owned();
    display_file_content(&path, file);
}

/// Processes entries in a ZIP archive with a provided handler function.
///
/// Iterates through all files in a ZIP archive, skipping directories, and applies
/// the specified handler function to each file entry.
///
/// # Arguments
/// * `path` - Path to the ZIP archive file
/// * `handler` - A function that takes a `ZipFile` and processes it (e.g., displaying content or info)
///
/// # Returns
/// * `Ok(())` if all operations succeeded
/// * `Err(ZcatError)` if any operation fails, with details about the failure
///
/// # Errors
/// This function can return the following errors:
/// * `ZcatError::IoError` - If there's an error opening the file
/// * `ZcatError::ZipError` - If there's an error reading the ZIP archive or its entries
fn handle_zip_entries(
    path: &PathBuf,
    handler: fn(zip::read::ZipFile) -> (),
) -> Result<(), ZcatError> {
    let file = File::open(path)?;
    let mut archive = zip::read::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        if file.is_dir() {
            continue;
        }
        handler(file);
    }
    Ok(())
}

/// Displays the content of compressed files or archives.
///
/// This function handles both single compressed files and tar archives:
/// - For single compressed files (e.g., .gz, .bz2), it displays the decompressed content
/// - For tar archives (e.g., .tar.gz, .tar.bz2), it displays the content of each file in the archive
///
/// The function includes formatting with headers and footers for visual separation between files.
/// Only text-based content (plain text, markdown, CSV, JSON, XML) will be displayed.
///
/// # Arguments
/// * `file_path` - Path to the compressed file
/// * `reader` - A reader implementing the `Read` trait that provides access to the compressed content
///
/// # Returns
/// * `Ok(())` if all operations succeeded
/// * `Err(ZcatError)` if any operation fails
///
/// # Errors
/// This function can return:
/// * `ZcatError::IoError` - If there's an error reading from the provided reader
/// * `ZcatError::TarError` - If there's an error processing a tar archive
fn extract_and_display_content<R>(file_path: &PathBuf, reader: R) -> Result<(), ZcatError>
where
    R: Read,
{
    let arr: Vec<&str> = file_path.to_str().unwrap().split(".").collect();
    let file_name = arr[..arr.len() - 1].join(".");

    if file_name.ends_with(".tar") {
        let archive = tar::Archive::new(reader);
        handle_tar_entries_from_tar_archive(archive, print_tar_entry_content)?;
    } else {
        display_file_content(&file_name, reader);
    }
    Ok(())
}

/// Displays information about compressed files or archives.
///
/// This function handles both single compressed files and tar archives:
/// - For single compressed files (e.g., .gz, .bz2), it shows the decompressed file size
/// - For tar archives (e.g., .tar.gz, .tar.bz2), it shows information about each file in the archive
///
/// # Arguments
/// * `file_path` - Path to the compressed file
/// * `reader` - A reader implementing the `Read` trait that provides access to the compressed content
///
/// # Returns
/// * `Ok(())` if all operations succeeded
/// * `Err(ZcatError)` if any operation fails
///
/// # Errors
/// This function can return:
/// * `ZcatError::IoError` - If there's an error reading from the provided reader
/// * `ZcatError::TarError` - If there's an error processing a tar archive
fn extract_and_display_info<R>(file_path: &PathBuf, mut reader: R) -> Result<(), ZcatError>
where
    R: Read,
{
    let arr: Vec<&str> = file_path.to_str().unwrap().split(".").collect();
    let file_name = arr[..arr.len() - 1].join(".");

    if file_name.ends_with(".tar") {
        let archive = tar::Archive::new(reader);
        handle_tar_entries_from_tar_archive(archive, print_tar_entry_info)?;
    } else {
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer)?;

        display_file_info(&file_name, buffer.len());
    }
    Ok(())
}

fn main() {
    let args = Args::parse();

    CONTEXT
        .set(Context {
            with_styling: !args.no_styling,
        })
        .unwrap();

    for file_path in args.files {
        let file_type = match infer_file_type(&file_path) {
            Ok(infer_output) => match infer_output {
                Some(file_type) => &file_type.to_string(),
                None => "",
            },
            Err(_) => {
                eprintln!(
                    "Could not infer the type of the following file: {:?}",
                    file_path
                );
                std::process::exit(1);
            }
        };

        if args.list {
            println!("📂 {file_path:?}");
            let output = match file_type {
                "application/zip" => handle_zip_entries(&file_path, print_zip_entry_info),
                "application/x-tar" => handle_tar_entries(&file_path, print_tar_entry_info),
                "application/gzip" => {
                    let file = File::open(&file_path).unwrap();
                    let gz = GzDecoder::new(file);
                    extract_and_display_info(&file_path, gz)
                }
                "application/x-bzip2" => {
                    let file = File::open(&file_path).unwrap();
                    let bz = bzip2::read::BzDecoder::new(file);
                    extract_and_display_info(&file_path, bz)
                }
                _ => {
                    let file_res =
                        File::open(file_path.clone()).map_err(|err| ZcatError::IoError(err));
                    file_res.map(|file| {
                        display_file_info(
                            &file_path.to_str().unwrap(),
                            file.metadata().unwrap().len() as usize,
                        );
                    })
                }
            };

            if output.is_err() {
                eprintln!(
                    "An error occurred while processing the file: {:?}. Error: {:?}",
                    file_path,
                    output.err().unwrap()
                );
                std::process::exit(1);
            }
        } else {
            let output = match file_type {
                "application/zip" => handle_zip_entries(&file_path, print_zip_entry_content),
                "application/x-tar" => handle_tar_entries(&file_path, print_tar_entry_content),
                "application/gzip" => {
                    let file = File::open(&file_path).unwrap();
                    let gz = GzDecoder::new(file);
                    extract_and_display_content(&file_path, gz)
                }
                "application/x-bzip2" => {
                    let file = File::open(&file_path).unwrap();
                    let bz = bzip2::read::BzDecoder::new(file);
                    extract_and_display_content(&file_path, bz)
                }
                _ => {
                    let file_res =
                        File::open(file_path.clone()).map_err(|err| ZcatError::IoError(err));
                    file_res.map(|file| {
                        display_file_content(
                            &file_path.clone().to_str().unwrap(),
                            BufReader::new(file),
                        )
                    })
                }
            };
            if output.is_err() {
                eprintln!(
                    "An error occurred while processing the file: {:?}. Error: {:?}",
                    file_path,
                    output.err().unwrap()
                );
                std::process::exit(1);
            }
        }
        println!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_file_size() {
        // Test bytes
        assert_eq!(format_file_size(0), "0 Bytes");
        assert_eq!(format_file_size(1), "1 Bytes");
        assert_eq!(format_file_size(512), "512 Bytes");
        assert_eq!(format_file_size(1023), "1023 Bytes");

        // Test kilobytes
        assert_eq!(format_file_size(1024), "1.00 KB");
        assert_eq!(format_file_size(1500), "1.46 KB");
        assert_eq!(format_file_size(1024 * 1024 - 1), "1024.00 KB");

        // Test megabytes
        assert_eq!(format_file_size(1024 * 1024), "1.00 MB");
        assert_eq!(format_file_size(1024 * 1024 * 3 / 2usize), "1.50 MB");
        assert_eq!(format_file_size(1024 * 1024 * 1024 - 1), "1024.00 MB");

        // Test gigabytes
        assert_eq!(format_file_size(1024 * 1024 * 1024), "1.00 GB");
        assert_eq!(format_file_size(1024 * 1024 * 1024 * 2), "2.00 GB");

        // Test very large sizes (should cap at GB)
        assert_eq!(format_file_size(1024 * 1024 * 1024 * 1024), "1024.00 GB");
        assert_eq!(
            format_file_size(1024 * 1024 * 1024 * 1024 * 5),
            "5120.00 GB"
        );
    }
}

#[cfg(test)]
mod integration_tests {
    use std::{
        fs::{self, File},
        io::{Write},
        path::{PathBuf},
    };

    use assert_cmd::Command;
    use flate2::write::GzEncoder;
    use predicates::prelude::PredicateBooleanExt;
    use predicates::prelude::*;
    use tempfile::TempDir;

    use crate::LINE_ENDING;

    const TEST_MESSAGE: &str = "Hello, World!\nThis is a test file.\n";
    const TAR_ARCHIVE_CONTENT: &[(&str, &str)] = &[
        ("file1.txt", "Content of file 1"),
        ("file2.txt", "Content of file 2"),
    ];

    const ZIP_TEST_FILES: &[(&str, &str)] = &[
        ("document.txt", "This is a plain text file.\nIt has multiple lines.\nTest content here."),
        ("readme.md", "# Test Document\n## Section 1\nThis is a markdown file with **bold** and *italic* text.\n\n- List item 1\n- List item 2"),
        ("data.csv", "id,name,value\n1,item1,100\n2,item2,200\n3,item3,300"),
        ("config.json", "{\n  \"name\": \"test\",\n  \"version\": \"1.0.0\",\n  \"settings\": {\n    \"enabled\": true,\n    \"timeout\": 30\n  }\n}"),
        ("data.xml", "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root>\n  <item id=\"1\">\n    <name>Test Item</name>\n    <value>100</value>\n  </item>\n</root>"),
        ("config.xml", "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE configuration>\n<configuration>\n  <settings>\n    <setting name=\"timeout\" value=\"30\"/>\n  </settings>\n</configuration>")
    ];

    fn create_test_gz_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
        let file_path = dir.path().join(name);
        let file = File::create(&file_path).unwrap();
        let mut encoder = GzEncoder::new(file, flate2::Compression::default());
        encoder.write_all(content.as_bytes()).unwrap();
        encoder.finish().unwrap();
        file_path
    }

    fn create_tar_with_encoder<W>(files: &[(&str, &str)], encoder: W) -> W
    where
        W: Write,
    {
        let mut tar = tar::Builder::new(encoder);

        for (file_name, file_content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(file_content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, file_name, file_content.as_bytes())
                .unwrap();
        }
        tar.finish().unwrap();
        tar.into_inner().unwrap()
    }

    fn create_test_tar_gz(dir: &TempDir, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let file_path = dir.path().join(name);
        let tar_gz = File::create(&file_path).unwrap();
        let mut encoder = GzEncoder::new(tar_gz, flate2::Compression::default());
        encoder = create_tar_with_encoder(files, encoder);
        encoder.flush().unwrap();
        encoder.finish().unwrap();
        file_path
    }

    fn create_test_bz2_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
        let file_path = dir.path().join(name);
        let file = File::create(&file_path).unwrap();
        let mut encoder = bzip2::write::BzEncoder::new(file, bzip2::Compression::default());
        encoder.write_all(content.as_bytes()).unwrap();
        encoder.finish().unwrap();

        file_path
    }

    fn create_test_tar_bz2_file(dir: &TempDir, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let file_path = dir.path().join(name);
        let file = File::create(&file_path).unwrap();
        let mut encoder = bzip2::write::BzEncoder::new(file, bzip2::Compression::default());
        encoder = create_tar_with_encoder(files, encoder);
        encoder.flush().unwrap();
        encoder.finish().unwrap();
        file_path
    }

    fn create_test_zip(dir: &TempDir, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let file_path = dir.path().join(name);
        let file = File::create(&file_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        for &(file_name, file_content) in files {
            zip.start_file(file_name, options).unwrap();
            zip.write_all(file_content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        file_path
    }

    fn create_test_zip_with_dirs(dir: &TempDir, name: &str) -> PathBuf {
        let file_path = dir.path().join(name);
        let file = File::create(&file_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        zip.add_directory("empty_dir/", options).unwrap();
        zip.add_directory("nested/", options).unwrap();
        zip.start_file("root_file.txt", options).unwrap();
        zip.write_all(b"Root level file\n").unwrap();
        zip.start_file("nested/nested_file.txt", options).unwrap();
        zip.write_all(b"Nested file content\n").unwrap();

        zip.finish().unwrap();

        file_path
    }

    #[test]
    fn test_gz_file_content() {
        let temp_dir = TempDir::new().unwrap();
        let gz_path = create_test_gz_file(&temp_dir, "text.txt.gz", TEST_MESSAGE);

        let assert = Command::cargo_bin("zcatr").unwrap().arg(gz_path).assert();

        assert
            .success()
            .stdout(predicates::str::contains(TEST_MESSAGE));
    }

    #[test]
    fn test_gz_file_info() {
        let temp_dir = TempDir::new().unwrap();
        let gz_path = create_test_gz_file(&temp_dir, "text.txt.gz", TEST_MESSAGE);

        let assert = Command::cargo_bin("zcatr")
            .unwrap()
            .arg("--list")
            .arg(gz_path)
            .assert();

        assert
            .success()
            .stdout(predicates::str::contains("text.txt"))
            .stdout(predicates::str::contains("Bytes"));
    }

    #[test]
    fn test_tar_gz_content() {
        let temp_dir = TempDir::new().unwrap();
        let tar_gz_path = create_test_tar_gz(&temp_dir, "test.tar.gz", TAR_ARCHIVE_CONTENT);

        let assert = Command::cargo_bin("zcatr")
            .unwrap()
            .arg(tar_gz_path)
            .assert();

        assert
            .success()
            .stdout(predicates::str::contains(TAR_ARCHIVE_CONTENT[0].1))
            .stdout(predicates::str::contains(TAR_ARCHIVE_CONTENT[1].1));
    }

    #[test]
    fn test_tar_gz_info() {
        let temp_dir = TempDir::new().unwrap();
        let tar_gz_path = create_test_tar_gz(&temp_dir, "test.tar.gz", TAR_ARCHIVE_CONTENT);

        let assert = Command::cargo_bin("zcatr")
            .unwrap()
            .arg("--list")
            .arg(tar_gz_path)
            .assert();

        assert
            .success()
            .stdout(predicates::str::contains("file1.txt"))
            .stdout(predicates::str::contains("file2.txt"))
            .stdout(predicates::str::contains("Bytes"));
    }

    #[test]
    fn test_non_existent_file() {
        let assert = Command::cargo_bin("zcatr")
            .unwrap()
            .arg("nonexistent.gz")
            .assert();

        assert.failure().stderr(predicates::str::contains(
            "Could not infer the type of the following file",
        ));
    }

    #[test]
    fn test_bz2_file_content() {
        let temp_dir = TempDir::new().unwrap();
        let bz2_path = create_test_bz2_file(&temp_dir, "text.txt.bz2", TEST_MESSAGE);

        let assert = Command::cargo_bin("zcatr").unwrap().arg(bz2_path).assert();

        assert
            .success()
            .stdout(predicates::str::contains(TEST_MESSAGE));
    }

    #[test]
    fn test_bz2_file_info() {
        let temp_dir = TempDir::new().unwrap();
        let bz2_path = create_test_bz2_file(&temp_dir, "text.txt.bz2", TEST_MESSAGE);

        let assert = Command::cargo_bin("zcatr")
            .unwrap()
            .arg("--list")
            .arg(bz2_path)
            .assert();

        assert
            .success()
            .stdout(predicates::str::contains("text.txt"))
            .stdout(predicates::str::contains("Bytes"));
    }

    #[test]
    fn test_tar_bz2_content() {
        let temp_dir = TempDir::new().unwrap();
        let tar_bz2_path = create_test_tar_bz2_file(&temp_dir, "test.tar.bz2", TAR_ARCHIVE_CONTENT);

        println!("{:?}", tar_bz2_path);

        let assert = Command::cargo_bin("zcatr")
            .unwrap()
            .arg(tar_bz2_path)
            .assert();

        assert
            .success()
            .stdout(predicates::str::contains(TAR_ARCHIVE_CONTENT[0].1))
            .stdout(predicates::str::contains(TAR_ARCHIVE_CONTENT[1].1));
    }

    #[test]
    fn test_zip_file_content() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = create_test_zip(&temp_dir, "test.zip", ZIP_TEST_FILES);

        let assert = Command::cargo_bin("zcatr").unwrap().arg(zip_path).assert();

        // Test specific content from each file type
        assert
            .success()
            // Plain text content
            .stdout(predicates::str::contains("This is a plain text file"))
            // Markdown content
            .stdout(predicates::str::contains("# Test Document"))
            .stdout(predicates::str::contains("**bold** and *italic*"))
            // CSV content
            .stdout(predicates::str::contains("id,name,value"))
            .stdout(predicates::str::contains("1,item1,100"))
            // JSON content
            .stdout(predicates::str::contains("\"version\": \"1.0.0\""))
            // XML content
            .stdout(predicates::str::contains("<item id=\"1\">"))
            .stdout(predicates::str::contains("<configuration>"));
    }

    #[test]
    fn test_mime_type_headers() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = create_test_zip(&temp_dir, "test.zip", ZIP_TEST_FILES);

        let assert = Command::cargo_bin("zcatr").unwrap().arg(zip_path).assert();

        // Verify file type recognition through header display
        assert
            .success()
            .stdout(predicates::str::contains("Content from \"document.txt\""))
            .stdout(predicates::str::contains("Content from \"readme.md\""))
            .stdout(predicates::str::contains("Content from \"data.csv\""))
            .stdout(predicates::str::contains("Content from \"config.json\""))
            .stdout(predicates::str::contains("Content from \"data.xml\""))
            .stdout(predicates::str::contains("Content from \"config.xml\""));
    }

    #[test]
    fn test_zip_with_directories() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = create_test_zip_with_dirs(&temp_dir, "test_with_dirs.zip");

        // Test listing mode
        let list_assert = Command::cargo_bin("zcatr")
            .unwrap()
            .arg("--list")
            .arg(&zip_path)
            .assert();

        list_assert
            .success()
            .stdout(predicates::str::contains("root_file.txt"))
            .stdout(predicates::str::contains("nested/nested_file.txt"))
            // Directory entries should be skipped
            .stdout(predicates::str::contains("empty_dir").not());

        // Test content mode
        let content_assert = Command::cargo_bin("zcatr").unwrap().arg(&zip_path).assert();

        content_assert
            .success()
            .stdout(predicates::str::contains("Root level file"))
            .stdout(predicates::str::contains("Nested file content"));
    }

    #[test]
    fn test_corrupted_zip() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("corrupted.zip");
        let mut file = File::create(&file_path).unwrap();

        // Write some random bytes that look like a ZIP but are invalid
        file.write_all(b"PK\x03\x04corrupted content").unwrap();

        let assert = Command::cargo_bin("zcatr").unwrap().arg(file_path).assert();

        assert.failure();
    }

    #[test]
    fn test_zip_file_info() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = create_test_zip(&temp_dir, "test.zip", ZIP_TEST_FILES);

        let assert = Command::cargo_bin("zcatr")
            .unwrap()
            .arg("--list")
            .arg(zip_path)
            .assert();

        let stdout = String::from_utf8_lossy(assert.get_output().stdout.as_slice());

        // Verify all file names are listed
        for &(name, _) in ZIP_TEST_FILES {
            assert!(predicates::str::contains(name).eval(&stdout));
            assert!(predicates::str::contains("Bytes").eval(&stdout));
        }
    }

    #[test]
    fn test_no_preview_for_binary_files() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("mixed_content.zip");
        let file = File::create(&file_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default().unix_permissions(0o755);

        // Add binary files (should not be previewable)
        zip.start_file("image.png", options).unwrap();
        zip.write_all(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
            .unwrap(); // PNG header

        zip.start_file("binary.pdf", options).unwrap();
        zip.write_all(b"%PDF-1.5\n%\x82\x82").unwrap(); // PDF header

        zip.start_file("program.exe", options).unwrap();
        zip.write_all(&[0x4D, 0x5A, 0x90, 0x00]).unwrap(); // EXE header

        zip.finish().unwrap();

        let assert = Command::cargo_bin("zcatr")
            .unwrap()
            .arg(&file_path)
            .assert();

        assert
            .success()
            // Binary files should show no preview message
            .stdout(predicates::str::contains("Preview not available in console").count(3));
    }

    #[test]
    fn test_it_should_display_the_content_of_a_simple_text_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("dummy.txt");
        let dummy_text = "THIS IS A DUMMY TEXT";
        File::create(file_path.clone())
            .unwrap()
            .write_all(dummy_text.as_bytes())
            .unwrap();

        let assert = Command::cargo_bin("zcatr")
            .unwrap()
            .arg(&file_path.clone())
            .assert();

        assert
            .success()
            .stdout(predicates::str::contains(dummy_text));

        fs::remove_file(file_path).unwrap();
    }

    #[test]
    fn test_it_should_display_the_size_of_a_simple_text_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("dummy.txt");
        let dummy_text = "THIS IS A DUMMY TEXT";
        let mut file = File::create(file_path.clone()).unwrap();
        file.write_all(dummy_text.as_bytes()).unwrap();

        let assert = Command::cargo_bin("zcatr")
            .unwrap()
            .arg("-l")
            .arg(&file_path.clone())
            .assert();

        assert.success().stdout(predicates::str::contains(format!(
            "{} Bytes",
            file.metadata().unwrap().len()
        )));

        fs::remove_file(file_path).unwrap();
    }

    #[test]
    fn test_it_should_not_display_header_and_footer_when_printing_file_content() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("dummy.txt");
        let dummy_text = "THIS IS A DUMMY TEXT";
        let mut file = File::create(file_path.clone()).unwrap();
        file.write_all(dummy_text.as_bytes()).unwrap();

        let assert = Command::cargo_bin("zcatr")
            .unwrap()
            .arg("--no-styling")
            .arg(&file_path.clone())
            .assert();

        assert
            .success()
            .stdout(predicates::str::contains("📄").not());

        fs::remove_file(file_path).unwrap();
    }
}
