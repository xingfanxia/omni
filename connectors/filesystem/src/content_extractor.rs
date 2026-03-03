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

#[cfg(test)]
mod tests {
    use super::extract_text_content;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_text_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn create_docx_file(dir: &TempDir, name: &str, text: &str) -> PathBuf {
        let path = dir.path().join(name);
        let docx = docx_rs::Docx::new()
            .add_paragraph(docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(text)));
        let file = std::fs::File::create(&path).unwrap();
        docx.build().pack(file).unwrap();
        path
    }

    fn create_xlsx_file(dir: &TempDir, name: &str, rows: &[&[&str]]) -> PathBuf {
        use zip::write::FileOptions;

        let path = dir.path().join(name);
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);

        // [Content_Types].xml
        zip.start_file("[Content_Types].xml", FileOptions::default())
            .unwrap();
        write!(
            zip,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
    <Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
      <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
      <Default Extension="xml" ContentType="application/xml"/>
      <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
      <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
    </Types>"#
        )
        .unwrap();

        // _rels/.rels
        zip.start_file("_rels/.rels", FileOptions::default())
            .unwrap();
        write!(
            zip,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
    <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
      <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
    </Relationships>"#
        )
        .unwrap();

        // xl/_rels/workbook.xml.rels
        zip.start_file("xl/_rels/workbook.xml.rels", FileOptions::default())
            .unwrap();
        write!(
            zip,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
    <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
      <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
    </Relationships>"#
        )
        .unwrap();

        // xl/workbook.xml
        zip.start_file("xl/workbook.xml", FileOptions::default())
            .unwrap();
        write!(
            zip,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
    <workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
              xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
      <sheets>
        <sheet name="Sheet1" sheetId="1" r:id="rId1"/>
      </sheets>
    </workbook>"#
        )
        .unwrap();

        // xl/worksheets/sheet1.xml — build inline string rows
        let mut sheet_xml = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
    <worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
      <sheetData>"#,
        );
        for (row_idx, row) in rows.iter().enumerate() {
            sheet_xml.push_str(&format!("\n    <row r=\"{}\">", row_idx + 1));
            for (col_idx, cell_val) in row.iter().enumerate() {
                let col_letter = (b'A' + col_idx as u8) as char;
                sheet_xml.push_str(&format!(
                    "<c r=\"{}{}\" t=\"inlineStr\"><is><t>{}</t></is></c>",
                    col_letter,
                    row_idx + 1,
                    cell_val
                ));
            }
            sheet_xml.push_str("</row>");
        }
        sheet_xml.push_str(
            r#"
      </sheetData>
    </worksheet>"#,
        );

        zip.start_file("xl/worksheets/sheet1.xml", FileOptions::default())
            .unwrap();
        write!(zip, "{}", sheet_xml).unwrap();

        zip.finish().unwrap();
        path
    }

    fn create_pptx_file(dir: &TempDir, name: &str, slide_texts: &[&str]) -> PathBuf {
        use zip::write::FileOptions;

        let path = dir.path().join(name);
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);

        for (i, text) in slide_texts.iter().enumerate() {
            let slide_name = format!("ppt/slides/slide{}.xml", i + 1);
            zip.start_file(&slide_name, FileOptions::default()).unwrap();
            write!(
                zip,
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
    <p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
           xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
      <p:cSld><p:spTree><p:sp><p:txBody>
        <a:p><a:r><a:t>{}</a:t></a:r></a:p>
      </p:txBody></p:sp></p:spTree></p:cSld>
    </p:sld>"#,
                text
            )
            .unwrap();
        }

        zip.finish().unwrap();
        path
    }

    #[test]
    fn test_extract_plain_text() {
        let dir = TempDir::new().unwrap();
        let path = create_text_file(&dir, "notes.txt", "Hello, world!");
        let result = extract_text_content(&path, "text/plain").unwrap();
        assert_eq!(result, "Hello, world!");
    }

    #[test]
    fn test_extract_markdown() {
        let dir = TempDir::new().unwrap();
        let path = create_text_file(&dir, "readme.md", "# Title\n\nSome content");
        let result = extract_text_content(&path, "text/markdown").unwrap();
        assert_eq!(result, "# Title\n\nSome content");
    }

    #[test]
    fn test_extract_csv() {
        let dir = TempDir::new().unwrap();
        let path = create_text_file(&dir, "data.csv", "name,age\nAlice,30\nBob,25");
        let result = extract_text_content(&path, "text/csv").unwrap();
        assert!(result.contains("Alice"));
        assert!(result.contains("Bob"));
    }

    #[test]
    fn test_extract_docx() {
        let dir = TempDir::new().unwrap();
        let path = create_docx_file(&dir, "document.docx", "This is a test document");
        let result = extract_text_content(
            &path,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        )
        .unwrap();
        assert!(
            result.contains("This is a test document"),
            "Expected DOCX content, got: '{}'",
            result
        );
    }

    #[test]
    fn test_extract_xlsx() {
        let dir = TempDir::new().unwrap();
        let path = create_xlsx_file(&dir, "data.xlsx", &[&["Name", "Age"], &["Alice", "30"]]);
        let result = extract_text_content(
            &path,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        )
        .unwrap();
        assert!(
            result.contains("Name") && result.contains("Alice"),
            "Expected XLSX content, got: '{}'",
            result
        );
    }

    #[test]
    fn test_extract_pptx() {
        let dir = TempDir::new().unwrap();
        let path = create_pptx_file(
            &dir,
            "presentation.pptx",
            &["Welcome slide", "Second slide"],
        );
        let result = extract_text_content(
            &path,
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        )
        .unwrap();
        assert!(
            result.contains("Welcome slide"),
            "Expected PPTX content, got: '{}'",
            result
        );
        assert!(
            result.contains("Second slide"),
            "Expected second slide, got: '{}'",
            result
        );
    }

    // ─── Excluded Format Tests ─────────────────────────────────────────────────

    #[test]
    fn test_source_code_returns_empty() {
        let dir = TempDir::new().unwrap();

        let cases = &[
            ("main.rs", "text/plain"),
            ("app.py", "text/plain"),
            ("index.js", "text/plain"),
        ];

        for (name, mime) in cases {
            let path = create_text_file(&dir, name, "fn main() { println!(\"hello\"); }");
            let result = extract_text_content(&path, mime).unwrap();
            assert!(
                result.is_empty(),
                "Expected empty for {}, got: '{}'",
                name,
                result
            );
        }
    }

    #[test]
    fn test_config_files_return_empty() {
        let dir = TempDir::new().unwrap();

        // Config-like extensions that get text/plain MIME but should be excluded
        let cases = &[
            ("config.json", "application/json"),
            ("data.xml", "application/xml"),
            ("settings.yaml", "text/plain"),
            ("config.toml", "text/plain"),
        ];

        for (name, mime) in cases {
            let path = create_text_file(&dir, name, "{\"key\": \"value\"}");
            let result = extract_text_content(&path, mime).unwrap();
            assert!(
                result.is_empty(),
                "Expected empty for {}, got: '{}'",
                name,
                result
            );
        }
    }

    #[test]
    fn test_unknown_binary_returns_empty() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("image.png");
        std::fs::write(&path, &[0x89, 0x50, 0x4E, 0x47]).unwrap();
        let result = extract_text_content(&path, "image/png").unwrap();
        assert!(result.is_empty());
    }
}
