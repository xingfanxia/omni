use anyhow::{anyhow, Context, Result};
use calamine::{open_workbook_auto_from_rs, Reader};
use docx_rs::read_docx;
use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;
use std::io::Cursor;
use std::path::Path;
use tracing::{debug, warn};
use zip::ZipArchive;

/// Extract human-readable text content from a file based on its MIME type.
///
/// Only document formats (plain text docs, PDF, DOCX, XLSX, PPTX) are supported.
/// Source code, config files, and other machine-oriented formats return empty string.
pub fn extract_text_content(path: &Path, mime_type: &str) -> Result<String> {
    match mime_type {
        // Plain text document formats
        "text/plain" | "text/markdown" | "text/rtf" | "text/csv" => {
            // Only index if the file extension indicates a document, not source code
            if is_source_code_extension(path) {
                debug!("Skipping source code file: {}", path.display());
                return Ok(String::new());
            }
            std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read text file: {}", path.display()))
        }

        // PDF
        "application/pdf" => {
            let data = std::fs::read(path)
                .with_context(|| format!("Failed to read PDF file: {}", path.display()))?;
            extract_pdf_text(data)
        }

        // DOCX (Office Open XML)
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            let data = std::fs::read(path)
                .with_context(|| format!("Failed to read DOCX file: {}", path.display()))?;
            extract_docx_text(data)
        }

        // XLSX (Office Open XML)
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
            let data = std::fs::read(path)
                .with_context(|| format!("Failed to read XLSX file: {}", path.display()))?;
            extract_excel_text(data)
        }

        // PPTX (Office Open XML)
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
            let data = std::fs::read(path)
                .with_context(|| format!("Failed to read PPTX file: {}", path.display()))?;
            extract_pptx_text(data)
        }

        // Legacy MS Office formats (best-effort)
        "application/msword" => {
            let data = std::fs::read(path)
                .with_context(|| format!("Failed to read DOC file: {}", path.display()))?;
            extract_docx_text(data).or_else(|e| {
                warn!("Failed to extract text from legacy .doc file: {}", e);
                Ok(String::new())
            })
        }

        "application/vnd.ms-excel" => {
            let data = std::fs::read(path)
                .with_context(|| format!("Failed to read XLS file: {}", path.display()))?;
            extract_excel_text(data).or_else(|e| {
                warn!("Failed to extract text from legacy .xls file: {}", e);
                Ok(String::new())
            })
        }

        // Everything else: source code, config, binary, etc. — skip
        _ => {
            debug!(
                "Skipping unsupported MIME type '{}': {}",
                mime_type,
                path.display()
            );
            Ok(String::new())
        }
    }
}

fn is_source_code_extension(path: &Path) -> bool {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e.to_lowercase(),
        None => return false,
    };

    matches!(
        ext.as_str(),
        "rs" | "py"
            | "js"
            | "ts"
            | "jsx"
            | "tsx"
            | "go"
            | "java"
            | "c"
            | "cpp"
            | "h"
            | "hpp"
            | "rb"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "pl"
            | "pm"
            | "php"
            | "swift"
            | "kt"
            | "scala"
            | "clj"
            | "ex"
            | "exs"
            | "erl"
            | "hs"
            | "lua"
            | "r"
            | "m"
            | "mm"
            | "cs"
            | "fs"
            | "vb"
            | "dart"
            | "zig"
            | "nim"
            | "v"
            | "sql"
            | "xml"
            | "yaml"
            | "yml"
            | "toml"
            | "json"
            | "ini"
            | "conf"
            | "cfg"
            | "hocon"
            | "properties"
            | "html"
            | "htm"
            | "css"
            | "scss"
            | "sass"
            | "less"
    )
}

fn extract_pdf_text(data: Vec<u8>) -> Result<String> {
    let text =
        pdf_extract::extract_text_from_mem(&data).context("Failed to extract text from PDF")?;
    Ok(text.trim().to_string())
}

fn extract_docx_text(data: Vec<u8>) -> Result<String> {
    let docx = read_docx(&data).context("Failed to read DOCX file")?;

    let mut text = String::new();

    for child in &docx.document.children {
        match child {
            docx_rs::DocumentChild::Paragraph(paragraph) => {
                for para_child in &paragraph.children {
                    if let docx_rs::ParagraphChild::Run(run) = para_child {
                        for run_child in &run.children {
                            if let docx_rs::RunChild::Text(text_element) = run_child {
                                text.push_str(&text_element.text);
                            }
                        }
                    }
                }
                text.push('\n');
            }
            _ => {}
        }
    }

    Ok(text.trim().to_string())
}

fn extract_excel_text(data: Vec<u8>) -> Result<String> {
    let cursor = Cursor::new(data);
    let mut workbook =
        open_workbook_auto_from_rs(cursor).context("Failed to open Excel file from binary data")?;

    let mut text = String::new();
    let sheet_names = workbook.sheet_names().to_owned();

    for sheet_name in &sheet_names {
        text.push_str(&format!("Sheet: {}\n", sheet_name));

        if let Some(Ok(range)) = workbook.worksheet_range(sheet_name) {
            for row in range.rows() {
                let row_text: Vec<String> = row.iter().map(|cell| cell.to_string()).collect();
                text.push_str(&row_text.join("\t"));
                text.push('\n');
            }
        }
        text.push('\n');
    }

    Ok(text.trim().to_string())
}

fn extract_pptx_text(data: Vec<u8>) -> Result<String> {
    let cursor = Cursor::new(data);
    let mut archive = ZipArchive::new(cursor).context("Failed to read PPTX as ZIP archive")?;

    let mut text = String::new();
    let mut slide_counter = 0;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .context("Failed to read file from PPTX archive")?;
        let file_name = file.name().to_string();

        if file_name.starts_with("ppt/slides/slide") && file_name.ends_with(".xml") {
            slide_counter += 1;
            text.push_str(&format!("Slide {}\n", slide_counter));

            let mut slide_content = String::new();
            std::io::Read::read_to_string(&mut file, &mut slide_content)
                .context("Failed to read slide XML content")?;

            let slide_text = extract_text_from_pptx_xml(&slide_content)?;
            text.push_str(&slide_text);
            text.push_str("\n\n");
        }
    }

    Ok(text.trim().to_string())
}

fn extract_text_from_pptx_xml(xml_content: &str) -> Result<String> {
    let mut reader = XmlReader::from_str(xml_content);
    reader.trim_text(true);

    let mut text = String::new();
    let mut buf = Vec::new();
    let mut inside_text = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                if e.name().as_ref() == b"a:t" {
                    inside_text = true;
                }
            }
            Ok(Event::Text(e)) => {
                if inside_text {
                    let content = e.unescape().context("Failed to unescape XML text")?;
                    text.push_str(&content);
                    text.push(' ');
                }
            }
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                b"a:t" => inside_text = false,
                b"a:p" => text.push('\n'),
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("Error reading PowerPoint XML: {}", e)),
            _ => {}
        }
        buf.clear();
    }

    Ok(text.trim().to_string())
}
