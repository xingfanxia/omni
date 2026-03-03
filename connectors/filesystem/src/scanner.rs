use crate::content_extractor;
use crate::models::{FileSystemFile, FileSystemPermissions, FileSystemSource};
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info, warn};
use walkdir::WalkDir;

pub struct FileSystemScanner {
    source: FileSystemSource,
}

impl FileSystemScanner {
    pub fn new(source: FileSystemSource) -> Self {
        Self { source }
    }

    pub async fn scan_directory(&self) -> Result<Vec<FileSystemFile>> {
        info!("Starting filesystem scan for source: {}", self.source.name);
        let mut files = Vec::new();

        if !self.source.base_path.exists() {
            return Err(anyhow::anyhow!(
                "Base path does not exist: {}",
                self.source.base_path.display()
            ));
        }

        if !self.source.base_path.is_dir() {
            return Err(anyhow::anyhow!(
                "Base path is not a directory: {}",
                self.source.base_path.display()
            ));
        }

        let walker = WalkDir::new(&self.source.base_path)
            .follow_links(false)
            .max_depth(100);

        for entry in walker {
            match entry {
                Ok(entry) => {
                    if let Some(file) = self.process_entry(entry).await? {
                        files.push(file);
                    }
                }
                Err(e) => {
                    warn!("Error walking directory: {}", e);
                    continue;
                }
            }
        }

        info!("Completed filesystem scan, found {} files", files.len());
        Ok(files)
    }

    async fn process_entry(&self, entry: walkdir::DirEntry) -> Result<Option<FileSystemFile>> {
        let path = entry.path().to_path_buf();
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to get metadata for {}: {}", path.display(), e);
                return Ok(None);
            }
        };

        let is_directory = metadata.is_dir();

        // Skip directories for now, we only want files
        if is_directory {
            return Ok(None);
        }

        // Check if file should be included based on filters
        if !self.source.should_include_file(&path) {
            debug!("Skipping file due to filters: {}", path.display());
            return Ok(None);
        }

        // Check file size limit
        if let Some(max_size) = self.source.max_file_size_bytes {
            if metadata.len() > max_size {
                debug!(
                    "Skipping file due to size limit ({} > {}): {}",
                    metadata.len(),
                    max_size,
                    path.display()
                );
                return Ok(None);
            }
        }

        let name = entry.file_name().to_string_lossy().to_string();

        let mime_type = mime_guess::from_path(&path)
            .first_or_octet_stream()
            .to_string();

        let permissions = self.get_file_permissions(&path)?;

        let filesystem_file = FileSystemFile {
            path: path.clone(),
            name,
            size: metadata.len(),
            mime_type,
            created_time: metadata.created().ok(),
            modified_time: metadata.modified().ok(),
            is_directory,
            permissions,
        };

        debug!("Processed file: {}", path.display());
        Ok(Some(filesystem_file))
    }

    /// Get file info from a path directly (used by watcher for real-time events)
    pub async fn get_file_info(&self, path: &PathBuf) -> Result<Option<FileSystemFile>> {
        let metadata = match tokio::fs::metadata(path).await {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to get metadata for {}: {}", path.display(), e);
                return Ok(None);
            }
        };

        let is_directory = metadata.is_dir();

        // Skip directories
        if is_directory {
            return Ok(None);
        }

        // Check if file should be included based on filters
        if !self.source.should_include_file(path) {
            debug!("Skipping file due to filters: {}", path.display());
            return Ok(None);
        }

        // Check file size limit
        if let Some(max_size) = self.source.max_file_size_bytes {
            if metadata.len() > max_size {
                debug!(
                    "Skipping file due to size limit ({} > {}): {}",
                    metadata.len(),
                    max_size,
                    path.display()
                );
                return Ok(None);
            }
        }

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let mime_type = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();

        let permissions = self.get_file_permissions(path)?;

        // Convert std metadata times to SystemTime
        let created_time = metadata.created().ok();
        let modified_time = metadata.modified().ok();

        let file = FileSystemFile {
            path: path.clone(),
            name,
            size: metadata.len(),
            mime_type,
            created_time,
            modified_time,
            is_directory,
            permissions,
        };

        debug!("Got file info: {}", path.display());
        Ok(Some(file))
    }

    fn get_file_permissions(&self, path: &PathBuf) -> Result<FileSystemPermissions> {
        let metadata = fs::metadata(path)
            .with_context(|| format!("Failed to get metadata for {}", path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = metadata.permissions().mode();

            // Check owner permissions (assuming we're the owner for simplicity)
            let readable = (mode & 0o400) != 0;
            let writable = (mode & 0o200) != 0;
            let executable = (mode & 0o100) != 0;

            Ok(FileSystemPermissions {
                readable,
                writable,
                executable,
            })
        }

        #[cfg(windows)]
        {
            let readonly = metadata.permissions().readonly();

            Ok(FileSystemPermissions {
                readable: true, // Assume readable if we can access it
                writable: !readonly,
                executable: false, // Windows doesn't have simple executable bit
            })
        }
    }

    pub async fn read_file_content(&self, file: &FileSystemFile) -> Result<String> {
        if file.is_directory {
            return Ok(String::new());
        }

        const MAX_CONTENT_SIZE: u64 = 10 * 1024 * 1024; // 10MB
        if file.size > MAX_CONTENT_SIZE {
            warn!(
                "File too large to read content: {} ({}MB)",
                file.path.display(),
                file.size / 1024 / 1024
            );
            return Ok(String::new());
        }

        match content_extractor::extract_text_content(&file.path, &file.mime_type) {
            Ok(content) => {
                if !content.is_empty() {
                    debug!(
                        "Extracted {} bytes from {}",
                        content.len(),
                        file.path.display()
                    );
                }
                Ok(content)
            }
            Err(e) => {
                warn!(
                    "Failed to extract content from {}: {}",
                    file.path.display(),
                    e
                );
                Ok(String::new())
            }
        }
    }
}
