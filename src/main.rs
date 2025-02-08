use std::{error::Error, fs::File, io::{self, BufReader, Read}, path::PathBuf};

use clap::Parser;
use flate2::read::GzDecoder;
use infer::Type;

#[cfg(target_os = "windows")]
const LINE_ENDING: &str = "\r\n";

#[cfg(not(target_os = "windows"))]
const LINE_ENDING: &str = "\n";

const MAGIC_BYTES_SIZE: usize = 512;
const BUFFER_SIZE: usize = 8192;


#[derive(Parser, Debug)]
#[command(version="0.1.0", about="zcatr is a program similar to the famous Unix-based zcat command.  It supports tar archives, tar archives compress with gzip, zip files and bzip files.")]
struct Args {
    #[arg(short, long, action, help="This option permits to show information about all the entries contained in the parsed file.")]
    list: bool,
    #[arg(help="The files to be parsed.")]
    files: Vec<PathBuf>
}

#[inline]
fn infer_file_type(path: &PathBuf) -> Result<Option<Type>, Box<dyn Error>> {
    let mime_type = infer::get_from_path(path.as_path())?;
    Ok(mime_type)
  
}


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


#[inline]
fn display_file_info(file_name: &str, file_size: usize) {
    println!("|
├── File: {file_name}
    Size: {}", format_file_size(file_size));
}


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


fn print_tar_entry_content<R>(entry: tar::Entry<R>) where R: Read {
    let path = entry.path().unwrap().into_owned();
    display_file_content(path.to_str().unwrap(), entry);
}

fn handle_tar_entries_from_tar_archive<R, F>(mut archive: tar::Archive<R>, handler: F) where R: Read, F: Fn(tar::Entry<R>) -> () {
    for entry in archive.entries().unwrap() {
        let entry = entry.unwrap();
        let entry_header = entry.header();

        if entry_header.entry_type().is_dir() {
            continue;
        }

        if entry.path().unwrap().to_str().unwrap().contains("._") {
            continue;
        }

        handler(entry);
    }
}

fn handle_tar_entries<F>(path: PathBuf, handler: F) where F: Fn(tar::Entry<File>) -> () {
    let file = File::open(path).unwrap();
    let archive= tar::Archive::new(file);
    handle_tar_entries_from_tar_archive(archive, handler);
}


fn print_zip_entry_info(file: zip::read::ZipFile) {
    display_file_info(file.name(), file.size() as usize);
}

fn print_zip_entry_content(file: zip::read::ZipFile) {
    let path = file.name().to_owned();
    display_file_content(&path, file);
}


fn handle_zip_entries(path: PathBuf, handler: fn(zip::read::ZipFile) -> ()) {
    let file = File::open(path).unwrap();
    let mut archive = zip::read::ZipArchive::new(file).unwrap();

    for i in 0..archive.len() {
        let file  = archive.by_index(i).unwrap();
        if file.is_dir() {
            continue;
        }
        handler(file);
    }
}

fn handle_bzip2_and_gzip_compressed_file<R>(file_path: PathBuf, reader: R) where R: Read {
    if file_path.to_str().unwrap().contains("tar") {
        let archive = tar::Archive::new(reader);
        handle_tar_entries_from_tar_archive(archive, print_tar_entry_content);
    } else {
        let arr= file_path.to_str().unwrap().split(".gz").collect::<Vec<&str>>();
        display_file_content(*arr.get(0).unwrap(), reader);
    }
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
            match file_type {
                "application/zip" => handle_zip_entries(file_path, print_zip_entry_info),
                _ =>  {
                    eprintln!("The following file type is not supported: {:?}", file_type);
                    std::process::exit(1);
                }
            }
        } else {
            match file_type {
                "application/zip" => handle_zip_entries(file_path, print_zip_entry_content),
                "application/x-tar" => handle_tar_entries(file_path, print_tar_entry_content),
                "application/gzip" => {
                    let file = File::open(&file_path).unwrap();
                    let gz = GzDecoder::new(file);
                    handle_bzip2_and_gzip_compressed_file(file_path, gz);
                },
                "application/x-bzip2" => {
                    let file = File::open(&file_path).unwrap();
                    let bz = bzip2::read::BzDecoder::new(file);
                    handle_bzip2_and_gzip_compressed_file(file_path, bz);
                },
                _ => {
                    eprintln!("The following file type is not supported: {:?}", file_type);
                    std::process::exit(1);
                }
            }
        }
        println!("");
    }
}
