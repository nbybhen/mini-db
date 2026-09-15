use crate::storage::disk::{DiskManager, PAGE_SIZE};
use std::collections::{HashMap, VecDeque};

// Specifies the number of pages that can be in buffer at a time
pub const BUFF_POOL_SIZE: usize = 64;

pub struct Frame {
    pub page_id: Option<u32>,
    // Holds the page data being caches
    pub data: [u8; PAGE_SIZE],
    // Number of active readers / writers
    pub pin_count: u32,
    // Flag for whether data has been modified by access methods since it was selected from disk
    pub is_dirty: bool,
}

//
//
//
pub struct BufferManager {
    // Holds instance of DiskManager
    disk_manager: DiskManager,
    // Holds BUFF_POOL_SIZE pages at a time in cache
    cache: VecDeque<Frame>,
    // NOTE(ansh): probably poor cache locality. mark for review later.
    // Hashes page_id --> index in cache vector for faster reads
    page_table: HashMap<u32, usize>,
}

impl Drop for BufferManager {
    fn drop(&mut self) {
        let _ = self.flush_all();
    }
}

impl BufferManager {
    pub fn new(disk_manager: DiskManager) -> Self {
        let mut cache = VecDeque::with_capacity(BUFF_POOL_SIZE);
        for _ in 0..BUFF_POOL_SIZE {
            cache.push_back(Frame {
                page_id: None,
                data: [0u8; PAGE_SIZE],
                pin_count: 0,
                is_dirty: false,
            });
        }

        Self {
            disk_manager,
            cache,
            page_table: HashMap::new(),
        }
    }

    // "Forwarding" allocation function for EE / Access Methods to call
    pub fn allocate_page(&mut self) -> std::io::Result<u32> {
        self.disk_manager.allocate_page()
    }

    // Helper fn for finding suitable Frame idx to be replaced.
    //
    // TEMPORARY eviction policy, was too lazy to look into LRU / CLOCK. This at least works for testing purposes,
    // but just panics when at max capacity...
    //
    // Also most definitely thrashes all over the place in a real setting....
    //
    // NOTE(ansh): currently a FIFO queue. might need to be upgraded later.
    // https://www.josehu.com/technical/2020/08/07/cache-eviction-algorithms.html
    pub fn find_replacable_frame(&self) -> usize {
        if self.cache.is_empty() || self.cache.len() == BUFF_POOL_SIZE {
            return 0;
        }

        return self.cache.len(); // `self.cache.len()-1` is the last element.
    }

    // Handles obtaining page information from cache.
    //
    // If the page doesn't exist in cache, requests page from DiskManager and writes the page
    // to an available Frame in cache.
    //
    // Frames are replaced when they either have a pin_count of 0 or no page_id assigned, while also
    // making sure to flush dirty data back to disk before replacing.
    //
    pub fn get_page(&mut self, page_id: u32) -> &mut [u8; PAGE_SIZE] {
        match self.page_table.get(&page_id) {
            Some(idx) => {
                let frame = self.cache.get_mut(*idx).expect("No frame at index!");
                frame.pin_count += 1;

                &mut frame.data
            }
            None => {
                let idx = self.find_replacable_frame();
                let frame = self.cache.get_mut(idx).expect("No frame at index!");

                // Flushes old page
                if frame.is_dirty {
                    self.disk_manager
                        .write_page(frame.page_id.unwrap(), &frame.data)
                        .expect("Failed to flush dirty page!");
                    frame.is_dirty = false;
                }

                if let Some(old_idx) = frame.page_id {
                    self.page_table.remove(&old_idx);
                }

                self.page_table.insert(page_id, idx);

                frame.pin_count = 1;
                frame.page_id = Some(page_id);

                self.disk_manager
                    .read_page(page_id, &mut frame.data)
                    .expect("Failed to read new page to frame!");

                &mut frame.data
            }
        }
    }

    // Decrements the pin_count by one off a frame, as well as marks as dirty if necessary
    pub fn unpin_page(&mut self, page_id: u32, is_dirty: bool) -> Result<(), &'static str> {
        let frame_idx = self
            .page_table
            .get(&page_id)
            .ok_or("Unpinning page that doesn't exist in cache!")?;

        let frame = self.cache.get_mut(*frame_idx).expect("Frame should exist");

        if is_dirty {
            frame.is_dirty = true;
        }
        frame.pin_count = frame.pin_count.saturating_sub(1);

        Ok(())
    }

    // Call to flush all dirty frames into disk.
    //
    // Useful for when BufferManager is Dropped (potential crash?), as well as for
    // "checkpointing" (future WAL / background thread for "cleanup"?).
    //
    pub fn flush_all(&mut self) -> Result<(), std::io::Error> {
        for frame in self.cache.iter_mut() {
            if frame.is_dirty {
                self.disk_manager.write_page(
                    frame.page_id.expect("Page is dirty w/o page_id?"),
                    &frame.data,
                )?;
                frame.is_dirty = false;
            }
        }

        self.disk_manager.sync()?;
        Ok(())
    }
}
