use std::fs::File;
use std::io::{BufRead, BufReader, Write};

fn main() -> std::io::Result<()> {
    let input_path = r"..\libunicode-table.h";
    let output_path = r"src\libunicode_table.rs";

    let file = File::open(input_path)?;
    let reader = BufReader::new(file);
    let mut out = File::create(output_path)?;

    writeln!(out, "#![allow(dead_code)]")?;
    writeln!(out, "#![allow(non_upper_case_globals)]")?;
    writeln!(out, "")?;

    let mut buffer: Vec<String> = Vec::new();
    let mut current_decl: Option<(String, String, String)> = None; // (rust_name, rust_type, size_str)
    let mut in_typedef = false;

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();

        if trimmed.starts_with("#") {
            continue;
        }
        if trimmed.is_empty() {
            if current_decl.is_none() {
                writeln!(out, "")?;
            }
            continue;
        }

        if trimmed.starts_with("typedef") {
            in_typedef = true;
            continue;
        }
        if in_typedef {
            if trimmed.contains(";") {
                in_typedef = false;
            }
            continue;
        }

        if trimmed.starts_with("static const") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            let mut rust_type = "u32";
            let mut name_part = "";

            // Check for pointer array: static const uint8_t * const unicode_prop_table[]
            if parts.len() >= 6 && parts[3] == "*" && parts[4] == "const" {
                rust_type = "&[u8]";
                name_part = parts[5];
            } else if parts.len() >= 4 {
                let ctype = parts[2];
                name_part = parts[3];

                rust_type = match ctype {
                    "uint32_t" => "u32",
                    "uint16_t" => "u16",
                    "uint8_t" => "u8",
                    "int32_t" => "i32",
                    "int16_t" => "i16",
                    "int8_t" => "i8",
                    "char" => "u8",
                    _ => "u32",
                };
            }

            if let Some(start_bracket) = name_part.find('[') {
                if let Some(end_bracket) = name_part.find(']') {
                    let name = &name_part[..start_bracket];
                    let size = &name_part[start_bracket + 1..end_bracket];
                    let rust_name = name.to_uppercase();

                    if !size.is_empty() {
                        writeln!(
                            out,
                            "pub const {}: [{}; {}] = [",
                            rust_name, rust_type, size
                        )?;
                        current_decl = None;
                    } else {
                        current_decl = Some((rust_name, rust_type.to_string(), String::new()));
                        buffer.clear();
                    }
                }
            }
        } else if trimmed == "};" {
            if let Some((rust_name, rust_type, _)) = &current_decl {
                // Check if it looks like a string table
                let is_string_table = rust_type == "u8"
                    && buffer.first().map_or(false, |s| s.trim().starts_with('"'));

                if is_string_table {
                    let mut full_str = String::new();
                    for l in &buffer {
                        let mut chars = l.chars();
                        let mut in_string = false;
                        let mut escaped = false;

                        while let Some(c) = chars.next() {
                            if escaped {
                                full_str.push(c);
                                escaped = false;
                            } else if c == '\\' {
                                full_str.push(c);
                                escaped = true;
                            } else if c == '"' {
                                in_string = !in_string;
                            } else {
                                if in_string {
                                    full_str.push(c);
                                }
                            }
                        }
                    }
                    writeln!(out, "pub const {}: &[u8] = b\"{}\";", rust_name, full_str)?;
                } else {
                    // Count elements
                    let content = buffer.join(" ");
                    let count = content.split(',').filter(|s| !s.trim().is_empty()).count();
                    writeln!(
                        out,
                        "pub const {}: [{}; {}] = [",
                        rust_name, rust_type, count
                    )?;

                    for l in &buffer {
                        let mut line_content = l.clone();

                        // Handle countof(x) -> x.len() as u16
                        if line_content.contains("countof(") {
                            let start = line_content.find("countof(").unwrap();
                            let end = line_content[start..].find(')').unwrap() + start;
                            let inner = &line_content[start + 8..end];
                            let replacement =
                                format!("{}.len() as {}", inner.to_uppercase(), rust_type);
                            line_content.replace_range(start..end + 1, &replacement);
                        } else if rust_type == "&[u8]" {
                            // Handle pointer array elements: unicode_prop_Dash_table -> &UNICODE_PROP_DASH_TABLE
                            // Split by comma to handle multiple entries per line
                            let parts: Vec<String> = line_content
                                .split(',')
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
                                .map(|s| {
                                    if s.starts_with("unicode_prop_") {
                                        format!("&{}", s.to_uppercase())
                                    } else {
                                        s.to_string()
                                    }
                                })
                                .collect();
                            line_content = parts.join(", ");
                            if l.trim().ends_with(',') {
                                line_content.push(',');
                            }
                        }

                        writeln!(out, "{}", line_content)?;
                    }
                    writeln!(out, "];")?;
                }
                current_decl = None;
                buffer.clear();
            } else {
                writeln!(out, "];")?;
            }
        } else {
            if current_decl.is_some() {
                buffer.push(line.clone());
            } else {
                writeln!(out, "{}", line)?;
            }
        }
    }

    Ok(())
}
