use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use serde_json::Value;
use regex::Regex;
use colored::*;

#[cfg(windows)]
mod windows_console {
    use winapi::um::wincon::SetConsoleTitleW;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    pub fn set_console_title(title: &str) {
        let wide: Vec<u16> = OsStr::new(title).encode_wide().chain(Some(0).into_iter()).collect();
        unsafe {
            SetConsoleTitleW(wide.as_ptr());
        }
    }
}

#[cfg(not(windows))]
mod windows_console {
    pub fn set_console_title(_title: &str) {
        // No-op on non-Windows platforms
    }
}

fn goodbye() {
    #[cfg(windows)]
    {
        use std::io::{self, Write};
        print!("Press Enter to continue...");
        io::stdout().flush().unwrap();
        io::stdin().read_line(&mut String::new()).unwrap();
    }
}

fn replace_hex_at_offset(data: &mut Vec<u8>, offset: usize, repl: &str) -> Result<(), String> {
    let bytes: Vec<u8> = repl.split_whitespace()
        .map(|s| u8::from_str_radix(s, 16).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;

    if offset + bytes.len() > data.len() {
        return Err("Replacement exceeds data size".into());
    }

    data[offset..offset + bytes.len()].copy_from_slice(&bytes);
    Ok(())
}

fn wildcard_pattern_scan(data: &[u8], pattern: &str, log_style: u8) -> Option<usize> {
    let pattern_bytes: Vec<Option<u8>> = pattern.split_whitespace()
        .map(|s| if s == "??" { None } else { Some(u8::from_str_radix(s, 16).ok()?) })
        .collect();

    'outer: for i in 0..=data.len() - pattern_bytes.len() {
        for (j, &pat_byte) in pattern_bytes.iter().enumerate() {
            if let Some(pat_byte) = pat_byte {
                if data[i + j] != pat_byte {
                    continue 'outer;
                }
            }
        }
        if log_style == 1 {
            println!("{}", format!("[FOUND] Match for pattern: {}", pattern.blue()).green());
        }
        return Some(i);
    }
    None
}

fn find_offset_by_method_name(method_name: &str, dump_path: &str, log_style: u8) -> Result<Option<usize>, io::Error> {
    let file = File::open(dump_path)?;
    let reader = BufReader::new(file);
    let offset_regex = Regex::new(r"Offset:\s*0x([0-9A-Fa-f]+)").unwrap();

    let mut previous_line = String::new();

    for line in reader.lines() {
        let line = line?;
        if line.contains(method_name) {
            if let Some(caps) = offset_regex.captures(&previous_line) {
                let offset = usize::from_str_radix(&caps[1], 16).unwrap();
                if log_style == 1 {
                    println!("{}", format!("[FOUND] {}", method_name.blue()).green());
                } else {
                    println!("{}", format!("Found {} at Offset: 0x{:X}", method_name, offset).green());
                }
                return Ok(Some(offset));
            } else {
                if log_style == 1 {
                    println!("{}", format!("[WARNING] No offset found for {}.", method_name.yellow()).red());
                } else {
                    println!("{}", format!("Warning: No offset found for {}.", method_name).yellow());
                }
                return Ok(None);
            }
        }
        previous_line = line;
    }
    Ok(None)
}

fn apply_patch(data: &mut Vec<u8>, offset: usize, patch: &Value, log_style: u8) -> Result<(), String> {
    if offset >= data.len() {
        return Err(format!("Error: Offset 0x{:X} is out of range for the input file.", offset).red().to_string());
    }

    if log_style == 1 {
        println!("{}", format!("[OFFSET] At: 0x{:X}", offset).cyan());
    } else {
        println!("{}", format!("Patching at Offset: 0x{:X}", offset).cyan());
    }
    replace_hex_at_offset(data, offset, patch.get("hex_code").unwrap().as_str().unwrap())?;
    if log_style == 1 {
        println!("{}", format!("[PATCH] Replaced with: {}", patch.get("hex_code").unwrap().as_str().unwrap()).purple());
    }
    Ok(())
}

fn patch_code(input_filename: &str, output_filename: &str, patch_list: &Value, dump_path: Option<&str>, log_style: u8) -> Result<(), io::Error> {
    let mut input_file = File::open(input_filename)?;
    let mut data = Vec::new();
    input_file.read_to_end(&mut data)?;

    for patch in patch_list.as_array().unwrap() {
        let offset = if let Some(method_name) = patch.get("method_name") {
            if let Some(dump_path) = dump_path {
                if let Some(offset) = find_offset_by_method_name(method_name.as_str().unwrap(), dump_path, log_style)? {
                    offset
                } else {
                    if log_style == 1 {
                        println!("{}", format!("[WARNING] Method '{}' not found. Skipping patch.", method_name.as_str().unwrap()).yellow().red());
                    } else {
                        println!("{}", format!("Warning: Method '{}' not found. Skipping patch.", method_name.as_str().unwrap()).yellow());
                    }
                    continue;
                }
            } else {
                if log_style == 1 {
                    println!("{}", "Warning: dump_path is required for method_name patches. Skipping patch.".yellow().red());
                } else {
                    println!("{}", "Warning: dump_path is required for method_name patches. Skipping patch.".yellow());
                }
                continue;
            }
        } else if let Some(offset_str) = patch.get("offset").and_then(|v| v.as_str()) {
            match usize::from_str_radix(offset_str.trim_start_matches("0x"), 16) {
                Ok(offset) => offset,
                Err(e) => {
                    if log_style == 1 {
                        println!("{}", format!("[ERROR] Parsing offset '{}': {}", offset_str, e).red());
                    } else {
                        println!("{}", format!("Error parsing offset '{}': {}", offset_str, e).red());
                    }
                    continue;
                }
            }
        } else if let Some(wildcard) = patch.get("wildcard") {
            if let Some(offset) = wildcard_pattern_scan(&data, wildcard.as_str().unwrap(), log_style) {
                offset
            } else {
                if log_style == 1 {
                    println!("{}", format!("[WARNING] Wildcard pattern '{}' not found. Skipping patch.", wildcard.as_str().unwrap()).yellow().red());
                } else {
                    println!("{}", format!("Warning: Wildcard pattern '{}' not found. Skipping patch.", wildcard.as_str().unwrap()).yellow());
                }
                continue;
            }
        } else {
            if log_style == 1 {
                println!("{}", "[WARNING] Patch does not contain a valid method_name, offset, or wildcard. Skipping patch.".yellow().red());
            } else {
                println!("{}", "Warning: Patch does not contain a valid method_name, offset, or wildcard. Skipping patch.".yellow());
            }
            continue;
        };

        // Apply the patch at the calculated offset
        if let Err(e) = apply_patch(&mut data, offset, patch, log_style) {
            if log_style == 1 {
                println!("{}", format!("[ERROR] Applying patch: {}", e).red());
            } else {
                println!("{}", format!("Error applying patch: {}", e).red());
            }
        }
    }

    let mut output_file = File::create(output_filename)?;
    output_file.write_all(&data)?;

    if log_style == 1 {
        println!("{}", format!("[DONE] Patched file saved as: '{}'.", output_filename).green());
    } else {
        println!("{}", format!("Patched to: '{}'.", output_filename).green());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows_console::set_console_title("Binary Patcher");

    let config_path = std::env::args().nth(1).unwrap_or_else(|| "config.json".to_string());

    let file = File::open(&config_path)?;
    let reader = BufReader::new(file);
    let config: Value = serde_json::from_reader(reader)?;

    let input_filename = config["Binary"]["input_file"].as_str().ok_or("Missing input_file in config")?;
    let output_filename = config["Binary"]["output_file"].as_str().ok_or("Missing output_file in config")?;
    let patch_list = &config["Binary"]["patches"];
    let dump_path = config["Binary"].get("dump_path").and_then(|v| v.as_str());
    let log_style = config["Binary"]["log_style"].as_u64().unwrap_or(0) as u8;
    if let Err(e) = patch_code(input_filename, output_filename, patch_list, dump_path, log_style) {
        eprintln!("{}", format!("Error: {}", e).red());
    }

    goodbye();
    Ok(())
}