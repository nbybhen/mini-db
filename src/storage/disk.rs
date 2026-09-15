use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

#[derive(Debug)]
#[repr(u8)]
enum PageType {
    DataPage = 1,
    IndexLeaf = 2,
    IndexInternal = 3,
}

// Specifies bytes per page
pub const PAGE_SIZE: usize = 4096;

pub struct PageHeader {
    page_type: PageType,
}

pub struct Page {
    pub id: u32,
    pub header: PageHeader,
}

//
// DiskManager handles I/O for the disk, including reading and writing pages
//
pub struct DiskManager {
    file: File,
    num_pages: u32,
}

impl DiskManager {
    pub fn new(file: File, num_pages: u32) -> Self {
        Self { file, num_pages }
    }

    // Reads page by calculating offset (page_id * 4096)
    pub fn read_page(&mut self, page_id: u32, buffer: &mut [u8; 4096]) -> std::io::Result<()> {
        // Ensures no overflow from u32 * PAGE_SIZE
        let offset = (page_id as u64) * (PAGE_SIZE as u64);

        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(buffer)?;

        Ok(())
    }

    // Writes buffer data to page_id
    pub fn write_page(&mut self, page_id: u32, buffer: &[u8; 4096]) -> std::io::Result<()> {
        let offset = (page_id as u64) * (PAGE_SIZE as u64);

        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(buffer)?;

        Ok(())
    }

    // Appends [0u8; 4096] to end of file, returning newly allocated page id
    pub fn allocate_page(&mut self) -> std::io::Result<u32> {
        let new_page_id = self.num_pages;
        let offset = (new_page_id as u64) * (PAGE_SIZE as u64);

        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(&[0u8; PAGE_SIZE])?;
        self.num_pages += 1;

        Ok(new_page_id)
    }
    // Should sync any in-memory data to be stored into disk (fsync)
    pub fn sync(&mut self) -> std::io::Result<()> {
        self.file.sync_all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::time::SystemTime;

    struct TmpFile(std::path::PathBuf);

    impl TmpFile {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "{name}_{}.db",
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .expect("test_read_write (src/storage/disk.rs): shouldn't error")
                    .as_secs()
            ));
            Self(path)
        }
    }

    // Cleans up file from testing
    impl Drop for TmpFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn test_read_write() -> std::io::Result<()> {
        let test_file = TmpFile::new("minidb_test");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&test_file.0)?;

        let mut dm = DiskManager::new(file, 0);

        let page0_id = dm.allocate_page()?;
        let page1_id = dm.allocate_page()?;

        // Ensures page 0 and page 1 are allocated properly
        assert_eq!(page0_id, 0);
        assert_eq!(page1_id, 1);
        assert_eq!(dm.num_pages, 2);

        // Makes page buffers for data
        let mut page0_data = [0u8; PAGE_SIZE];
        let mut page1_data = [0u8; PAGE_SIZE];

        // Stores text in first 10 bytes
        page0_data[0..10].copy_from_slice("HELLOTHERE".as_bytes());
        page1_data[0..11].copy_from_slice("HELLOTHERE2".as_bytes());

        // Writes to both pages
        dm.write_page(page0_id, &page0_data)?;
        dm.write_page(page1_id, &page1_data)?;

        // Imitates buffer mgr's cache requesting read using mutable buffers
        let mut read_page0 = [0u8; PAGE_SIZE];
        let mut read_page1 = [0u8; PAGE_SIZE];

        // Reads data from both pages
        dm.read_page(page0_id, &mut read_page0)?;
        dm.read_page(page1_id, &mut read_page1)?;

        assert_eq!(&read_page0[0..10], "HELLOTHERE".as_bytes());
        assert_eq!(&read_page1[0..11], "HELLOTHERE2".as_bytes());

        Ok(())
    }
}
