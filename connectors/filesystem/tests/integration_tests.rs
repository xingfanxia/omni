use omni_filesystem_connector::content_extractor::extract_text_content;
use omni_filesystem_connector::models::{FileSystemFile, FileSystemPermissions, FileSystemSource};
use omni_filesystem_connector::scanner::FileSystemScanner;
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

// ─── Helpers ────────────────────────────────────────────────────────────────

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

fn make_source(dir: &TempDir) -> FileSystemSource {
    FileSystemSource {
        id: "test-source".to_string(),
        name: "Test Source".to_string(),
        base_path: dir.path().to_path_buf(),
        scan_interval_seconds: 300,
        file_extensions: None,
        exclude_patterns: None,
        max_file_size_bytes: None,
    }
}

fn make_file(path: PathBuf, mime_type: &str) -> FileSystemFile {
    let metadata = std::fs::metadata(&path).unwrap();
    FileSystemFile {
        name: path.file_name().unwrap().to_string_lossy().to_string(),
        size: metadata.len(),
        mime_type: mime_type.to_string(),
        created_time: metadata.created().ok(),
        modified_time: metadata.modified().ok(),
        is_directory: false,
        permissions: FileSystemPermissions {
            readable: true,
            writable: true,
            executable: false,
        },
        path,
    }
}

// ─── Content Extraction Tests ───────────────────────────────────────────────

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

// ─── Scanner Tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_scan_directory_discovers_files() {
    let dir = TempDir::new().unwrap();
    create_text_file(&dir, "notes.txt", "some text");
    create_text_file(&dir, "readme.md", "# Hello");
    create_docx_file(&dir, "doc.docx", "word content");

    let source = make_source(&dir);
    let scanner = FileSystemScanner::new(source);
    let files = scanner.scan_directory().await.unwrap();

    assert_eq!(files.len(), 3);
}

#[tokio::test]
async fn test_scan_directory_extension_filter() {
    let dir = TempDir::new().unwrap();
    create_text_file(&dir, "notes.txt", "text content");
    create_text_file(&dir, "readme.md", "markdown content");
    create_text_file(&dir, "code.rs", "fn main() {}");

    let mut source = make_source(&dir);
    source.file_extensions = Some(vec!["txt".to_string()]);

    let scanner = FileSystemScanner::new(source);
    let files = scanner.scan_directory().await.unwrap();

    assert_eq!(files.len(), 1);
    assert!(files[0].name.ends_with(".txt"));
}

#[tokio::test]
async fn test_scan_directory_exclude_patterns() {
    let dir = TempDir::new().unwrap();
    create_text_file(&dir, "notes.txt", "text content");

    // Create a subdirectory with a file
    let sub = dir.path().join("hidden");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("secret.txt"), "secret").unwrap();

    let mut source = make_source(&dir);
    source.exclude_patterns = Some(vec!["hidden".to_string()]);

    let scanner = FileSystemScanner::new(source);
    let files = scanner.scan_directory().await.unwrap();

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "notes.txt");
}

#[tokio::test]
async fn test_scan_directory_size_limit() {
    let dir = TempDir::new().unwrap();
    create_text_file(&dir, "small.txt", "small");

    let big_path = dir.path().join("big.txt");
    std::fs::write(&big_path, vec![b'x'; 2000]).unwrap();

    let mut source = make_source(&dir);
    source.max_file_size_bytes = Some(1000);

    let scanner = FileSystemScanner::new(source);
    let files = scanner.scan_directory().await.unwrap();

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "small.txt");
}

// ─── Permissions Tests ──────────────────────────────────────────────────────

#[test]
fn test_connector_event_has_public_permissions() {
    let dir = TempDir::new().unwrap();
    let path = create_text_file(&dir, "test.txt", "content");
    let file = make_file(path, "text/plain");

    let event = file.to_connector_event(
        "sync-1".to_string(),
        "source-1".to_string(),
        "content-1".to_string(),
    );

    match event {
        shared::models::ConnectorEvent::DocumentCreated { permissions, .. } => {
            assert!(permissions.public, "Permissions should be public");
        }
        _ => panic!("Expected DocumentCreated event"),
    }
}

// ─── Read File Content Tests ────────────────────────────────────────────────

#[tokio::test]
async fn test_read_file_content_text() {
    let dir = TempDir::new().unwrap();
    let path = create_text_file(&dir, "notes.txt", "Hello from scanner");
    let file = make_file(path, "text/plain");

    let source = make_source(&dir);
    let scanner = FileSystemScanner::new(source);
    let content = scanner.read_file_content(&file).await.unwrap();

    assert_eq!(content, "Hello from scanner");
}

#[tokio::test]
async fn test_read_file_content_docx() {
    let dir = TempDir::new().unwrap();
    let path = create_docx_file(&dir, "doc.docx", "Scanner DOCX test");
    let file = make_file(
        path,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    );

    let source = make_source(&dir);
    let scanner = FileSystemScanner::new(source);
    let content = scanner.read_file_content(&file).await.unwrap();

    assert!(
        content.contains("Scanner DOCX test"),
        "Expected DOCX content via scanner, got: '{}'",
        content
    );
}

#[tokio::test]
async fn test_read_file_content_source_code_returns_empty() {
    let dir = TempDir::new().unwrap();
    let path = create_text_file(&dir, "main.rs", "fn main() {}");
    let file = make_file(path, "text/plain");

    let source = make_source(&dir);
    let scanner = FileSystemScanner::new(source);
    let content = scanner.read_file_content(&file).await.unwrap();

    assert!(
        content.is_empty(),
        "Expected empty for source code, got: '{}'",
        content
    );
}

#[tokio::test]
async fn test_read_file_content_directory_returns_empty() {
    let dir = TempDir::new().unwrap();

    let file = FileSystemFile {
        path: dir.path().to_path_buf(),
        name: "testdir".to_string(),
        size: 0,
        mime_type: "inode/directory".to_string(),
        created_time: None,
        modified_time: None,
        is_directory: true,
        permissions: FileSystemPermissions {
            readable: true,
            writable: true,
            executable: true,
        },
    };

    let source = make_source(&dir);
    let scanner = FileSystemScanner::new(source);
    let content = scanner.read_file_content(&file).await.unwrap();

    assert!(content.is_empty());
}
