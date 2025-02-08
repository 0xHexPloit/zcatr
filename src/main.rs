use std::{fs::File, io::{self, BufReader, Read}, path::PathBuf};

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
    files: Vec<PathBuf>
}


/// Determines the MIME type of a file using file signature detection.
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
    println!("|
├── File: {file_name}
|   Size: {}", format_file_size(file_size));
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
fn display_file_content<R>(file_name: &str, mut reader: R) where R: Read {
    println!("\n📄 Content from \"{}\":", file_name);
    println!("{}", "─".repeat(40));


    let mut magic_bytes_buffer = [0u8; MAGIC_BYTES_SIZE];
    let magic_bytes_read = reader.read(&mut magic_bytes_buffer).unwrap();
    let magic_bytes = &magic_bytes_buffer[..magic_bytes_read];
    let printing_handler = move || {
        let mut reader = BufReader::new(io::Cursor::new(magic_bytes).chain(reader));
        let mut buffer = [0; BUFFER_SIZE];
        
        // Stream the content
        while let Ok(n) = reader.read(&mut buffer) {
            if n == 0 { break; }
            if let Ok(text) = std::str::from_utf8(&buffer[..n]) {
                print!("{}", text);
            } else {
                println!("[Error: Invalid UTF-8 sequence encountered]");
                break;
            }
        }
    };
    
    match infer::get(magic_bytes) {
        Some(mime_type) => match mime_type.mime_type() {
            "text/plain" | "text/markdown" | "text/csv" | "application/json" | "application/xml" | "text/xml" => {
                printing_handler();
            },
            _ => {
                print!("Preview not available in console.")
            }
        },
        None => {
            printing_handler();
        }
    } 
    
    println!("{}{}", LINE_ENDING, "─".repeat(40));
}


/// Prints information about a single entry within a TAR archive.
///
/// Takes a TAR entry and displays its path and size in a tree-like structure.
/// This function unwraps the entry's path and size, then delegates the actual
/// display formatting to `display_file_info`.
///
/// # Arguments
/// * `entry` - A TAR entry implementing the `Read` trait
fn print_tar_entry_info<R>(entry: tar::Entry<R>) where R: Read {
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
fn print_tar_entry_content<R>(entry: tar::Entry<R>) where R: Read {
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
fn handle_tar_entries_from_tar_archive<R, F>(mut archive: tar::Archive<R>, handler: F) -> Result<(), ZcatError> where R: Read, F: Fn(tar::Entry<R>) -> () {
    for entry in archive.entries()? {
        let entry = entry?;
        let entry_header = entry.header();

        if entry_header.entry_type().is_dir() {
            continue;
        }

        if entry.path().unwrap().to_str().unwrap().contains("._") {
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
fn handle_tar_entries<F>(path: &PathBuf, handler: F) -> Result<(), ZcatError> where F: Fn(tar::Entry<File>) -> () {
    let file = File::open(path)?;
    let archive= tar::Archive::new(file);
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
fn handle_zip_entries(path: &PathBuf, handler: fn(zip::read::ZipFile) -> ()) -> Result<(), ZcatError> {
    let file = File::open(path).unwrap();
    let mut archive = zip::read::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let file  = archive.by_index(i)?;
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
fn extract_and_display_content<R>(file_path: &PathBuf, reader: R) -> Result<(), ZcatError> where R: Read {
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
fn extract_and_display_info<R>(file_path: &PathBuf, mut reader: R) -> Result<(), ZcatError> where R: Read {
    let arr: Vec<&str> = file_path.to_str().unwrap().split(".").collect();
    let file_name = arr[..arr.len() - 1].join(".");

    if file_name.ends_with(".tar") {
        let archive = tar::Archive::new(reader);
        handle_tar_entries_from_tar_archive(archive, print_tar_entry_info)?;
    } else {
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer).unwrap();

        display_file_info(&file_name, buffer.len());
    }
    Ok(())
}


fn main() {
    let args = Args::parse();

    for file_path in args.files {
        let file_type = match infer_file_type(&file_path) {
            Ok(infer_output) => match infer_output {
                Some(file_type) => &file_type.to_string(),
                None => ""
            },
            Err(_) => {
                eprintln!("Could not infer the type of the following file: {:?}", file_path);
                continue;
            }
        };

        if args.list {
            println!("📂 {file_path:?}");
            let output = match file_type {
                "application/zip" => handle_zip_entries(&file_path, print_zip_entry_info),
                "application/x-tar" => handle_tar_entries(&file_path, print_tar_entry_info,),
                "application/gzip" => {
                    let file = File::open(&file_path).unwrap();
                    let gz = GzDecoder::new(file);
                    extract_and_display_info(&file_path, gz)
                },
                "application/x-bzip2" => {
                    let file = File::open(&file_path).unwrap();
                    let bz = bzip2::read::BzDecoder::new(file);
                    extract_and_display_info(&file_path, bz)
                },
                _ =>  {
                    eprintln!("The following file type is not supported: {:?}", file_type);
                    std::process::exit(1);
                }
            };

            if output.is_err() {
                eprintln!("An error occurred while processing the file: {:?}. Error: {:?}", file_path, output.err().unwrap());
            }
        } else {
            let output = match file_type {
                "application/zip" => handle_zip_entries(&file_path, print_zip_entry_content),
                "application/x-tar" => handle_tar_entries(&file_path, print_tar_entry_content),
                "application/gzip" => {
                    let file = File::open(&file_path).unwrap();
                    let gz = GzDecoder::new(file);
                    extract_and_display_content(&file_path, gz)
                },
                "application/x-bzip2" => {
                    let file = File::open(&file_path).unwrap();
                    let bz = bzip2::read::BzDecoder::new(file);
                    extract_and_display_content(&file_path, bz)
                },
                _ => {
                    eprintln!("The following file type is not supported: {:?}", file_type);
                    std::process::exit(1);
                }
            };
            if output.is_err() {
                eprintln!("An error occurred while processing the file: {:?}. Error: {:?}", file_path, output.err().unwrap());
            }
        }
        println!("");
    }
}
