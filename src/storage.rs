// Storage module for FeralCTF
// Implements file system storage as per FERALCTF_SPEC.md section 6.1

use std::fs::File;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Storage {
    base_path: PathBuf,
}

impl Storage {
    /// Create a new Storage instance with the given base path
    pub fn new<P: AsRef<Path>>(base_path: P) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let base_path = base_path.as_ref().to_path_buf();
        
        // Ensure the base directory exists
        if !base_path.exists() {
            fs::create_dir_all(&base_path)?;
        }
        
        Ok(Self { base_path })
    }

    /// Read a file from storage
    pub fn read<P: AsRef<Path>>(&self, path: P) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let full_path = self.base_path.join(path);
        
        if !full_path.exists() {
            return Err(format!("File not found: {}", full_path.display()).into());
        }
        
        let mut file = File::open(&full_path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        
        Ok(contents)
    }

    /// Write data to a file in storage
    pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(
        &self,
        path: P,
        data: C,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let full_path = self.base_path.join(path);
        let parent = full_path.parent().ok_or("Cannot determine parent directory")?;
        
        // Ensure parent directory exists
        fs::create_dir_all(parent)?;
        
        let mut file = File::create(&full_path)?;
        file.write_all(data.as_ref())?;
        
        Ok(())
    }

    /// Delete a file from storage
    pub fn delete<P: AsRef<Path>>(&self, path: P) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let full_path = self.base_path.join(path);
        
        if full_path.exists() {
            fs::remove_file(&full_path)?;
        }
        
        Ok(())
    }

    /// List files in a directory
    pub fn list<P: AsRef<Path>>(&self, path: P) -> Result<Vec<PathBuf>, Box<dyn std::error::Error + Send + Sync>> {
        let full_path = self.base_path.join(path);
        
        if !full_path.exists() || !full_path.is_dir() {
            return Err(format!("Directory not found: {}", full_path.display()).into());
        }
        
        let mut entries = Vec::new();
        if let Ok(dir_read) = fs::read_dir(&full_path) {
            for entry_result in dir_read {
                if let Ok(entry) = entry_result {
                    let path = entry.path();
                    if path.is_file() {
                        entries.push(path);
                    }
                }
            }
        }
        
        Ok(entries)
    }

    /// Get file size in bytes
    pub fn size<P: AsRef<Path>>(&self, path: P) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        let full_path = self.base_path.join(path);
        
        if !full_path.exists() {
            return Err(format!("File not found: {}", full_path.display()).into());
        }
        
        Ok(fs::metadata(&full_path)?.len())
    }

    /// Check if a file exists
    pub fn exists<P: AsRef<Path>>(&self, path: P) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let full_path = self.base_path.join(path);
        Ok(full_path.exists())
    }

    /// Move a file within storage
    pub fn move_file<P: AsRef<Path>>(
        &self,
        from: P,
        to: P,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let full_path = self.base_path.join(from);
        let dest_path = self.base_path.join(to);
        
        if full_path.exists() {
            fs::rename(&full_path, &dest_path)?;
        } else {
            return Err(format!("Source file not found: {}", full_path.display()).into());
        }
        
        Ok(())
    }

    /// Copy a file within storage
    pub fn copy<P: AsRef<Path>>(
        &self,
        from: P,
        to: P,
    ) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
        let full_path = self.base_path.join(from);
        let dest_path = self.base_path.join(to);
        
        if !full_path.exists() {
            return Err(format!("Source file not found: {}", full_path.display()).into());
        }
        
        let mut dest_file = File::create(&dest_path)?;
        let mut source_file = File::open(&full_path)?;
        std::io::copy(&mut source_file, &mut dest_file)?;
        
        Ok(dest_path)
    }
}

impl Default for Storage {
    fn default() -> Self {
        Self::new(".").unwrap_or_else(|_| Self {
            base_path: PathBuf::from("./storage"),
        })
    }
}