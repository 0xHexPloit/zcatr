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
    if entry.header().entry_type().is_dir() {
        return;
    }

    let path = entry.path().unwrap().into_owned();

    if path.to_str().unwrap().contains("._") {
        return;
    }

    display_file_content(path.to_str().unwrap(), entry);
}

fn handle_tar_entries_from_tar_archive<R, F>(mut archive: tar::Archive<R>, handler: F) where R: Read, F: Fn(tar::Entry<R>) -> () {
    for entry in archive.entries().unwrap() {
        let entry = entry.unwrap();
        handler(entry);
    }
}

fn handle_tar_entries<F>(path: PathBuf, handler: F) where F: Fn(tar::Entry<File>) -> () {
    let file = File::open(path).unwrap();
    let archive= tar::Archive::new(file);
    handle_tar_entries_from_tar_archive(archive, handler);
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

        println!("{}", file_type);

        if args.list {
            match file_type {
                _ => ()
            }
        } else {
            match file_type {
                "application/x-tar" => handle_tar_entries(file_path, print_tar_entry_content),
                "application/gzip" => {
                    let file = File::open(&file_path).unwrap();
                    let gz = GzDecoder::new(file);

                    if file_path.to_str().unwrap().contains("tar") {
                        let archive = tar::Archive::new(gz);
                        handle_tar_entries_from_tar_archive(archive, print_tar_entry_content);
                    } else {
                        let arr= file_path.to_str().unwrap().split(".gz").collect::<Vec<&str>>();
                        display_file_content(*arr.get(0).unwrap(), gz);
                    }
                },
                _ => ()
            }
        }
    }
}
