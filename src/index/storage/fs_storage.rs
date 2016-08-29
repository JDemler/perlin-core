use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;
use std::marker::PhantomData;



use index::storage::{Result, Storage, StorageError};
use utils::compression::{vbyte_encode, VByteDecoder};
use utils::byte_code::{ByteDecodable, ByteEncodable};

pub struct FsStorage<TItem> {
    // Stores for every id the offset in the file and the length
    entries: BTreeMap<u64, (u64 /* offset */, u32 /* length */)>,
    persistent_entries: File,
    data: File,
    current_offset: u64,
    _item_type: PhantomData<TItem>
}

impl<TItem> FsStorage<TItem> {
    /// Creates a new and empty instance of FsStorage
    pub fn new(path: &Path) -> Self {
        assert!(path.is_dir(),
                "FsStorage::new expects a directory not a file!");
        FsStorage {
            current_offset: 0,
            entries: BTreeMap::new(),
            persistent_entries: OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(path.join("entries.bin"))
                .unwrap(),
            data: OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(path.join("data.bin"))
                .unwrap(),
            _item_type: PhantomData
        }
    }

    /// Reads a FsStorage from an previously populated folder.
    // TODO: Return result
    pub fn from_folder(path: &Path) -> Self {
        // Read from entry file to BTreeMap.
        let mut entries = BTreeMap::new();
        // 1. Open file
        let mut entries_file =
            OpenOptions::new().read(true).open(path.join("entries.bin")).unwrap();
        let mut bytes = Vec::with_capacity(entries_file.metadata().unwrap().len() as usize);
        // 2. Read file
        assert!(entries_file.read_to_end(&mut bytes).is_ok());
        let mut decoder = VByteDecoder::new(bytes.into_iter());
        // 3. Decode entries and write them to BTreeMap
        while let Some(entry) = decode_entry(&mut decoder) {
            entries.insert(entry.0, (entry.1, entry.2));
        }

        // Get data file length for offset
        let offset = File::open(path.join("data.bin"))
            .unwrap()
            .metadata()
            .unwrap()
            .len();

        FsStorage {
            current_offset: offset,
            entries: entries,
            persistent_entries: OpenOptions::new()
                .append(true)
                .open(path.join("entries.bin"))
                .unwrap(),
            data: OpenOptions::new()
                .read(true)
                .append(true)
                .open(path.join("data.bin"))
                .unwrap(),
            _item_type: PhantomData
        }
    }
}


impl<TItem: ByteDecodable + ByteEncodable + Sync> Storage<TItem> for FsStorage<TItem> {
    fn get(&self, id: u64) -> Result<Arc<TItem>> {
        if let Some(item_position) = self.entries.get(&id) {
            // Get filehandle
            let mut f = self.data.try_clone().unwrap();
            // Seek to position of item
            f.seek(SeekFrom::Start(item_position.0)).unwrap();
            let mut bytes = vec![0; item_position.1 as usize];
            // Read all bytes
            f.read_exact(&mut bytes).unwrap();
            // Decode item
            let item = TItem::decode(bytes.into_iter()).unwrap();
            Ok(Arc::new(item))
        } else {
            Err(StorageError::KeyNotFound)
        }
    }

    fn store(&mut self, id: u64, data: TItem) -> Result<()> {
        // Encode the data
        let bytes = data.encode();
        // Append it to the file
        if let Err(e) = self.data.write_all(&bytes) {
            return Err(StorageError::WriteError(Some(e)));
        }
        // And save the offset and the number of bytes written for later recovery
        self.entries.insert(id, (self.current_offset, bytes.len() as u32));
        // Also write the id, offset and number of bytes written to file for persistence
        let entry_bytes = encode_entry(id, self.current_offset, bytes.len() as u32);
        if let Err(e) = self.persistent_entries.write_all(&entry_bytes) {
            return Err(StorageError::WriteError(Some(e)));
        }

        // Update offset
        self.current_offset += bytes.len() as u64;
        Ok(())
    }
}

fn encode_entry(id: u64, offset: u64, length: u32) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.append(&mut vbyte_encode(id as usize));
    bytes.append(&mut vbyte_encode(offset as usize));
    bytes.append(&mut vbyte_encode(length as usize));
    bytes
}

fn decode_entry(decoder: &mut VByteDecoder) -> Option<(u64, u64, u32)> {
    let id = try_option!(decoder.next()) as u64;
    let offset = try_option!(decoder.next()) as u64;
    let length = try_option!(decoder.next()) as u32;
    Some((id, offset, length))
}




#[cfg(test)]
mod tests {
    use std::fs::create_dir_all;
    use std::path::Path;

    use super::*;
    use index::storage::{Storage, StorageError};

    #[test]
    pub fn basic() {
        let item1 = 15;
        let item2 = 32;
        assert!(create_dir_all(Path::new("/tmp/test_index")).is_ok());
        let mut prov = FsStorage::new(Path::new("/tmp/test_index"));
        assert!(prov.store(0, item1.clone()).is_ok());
        assert_eq!(prov.get(0).unwrap().as_ref(), &item1);
        assert!(prov.store(1, item2.clone()).is_ok());
        assert_eq!(prov.get(1).unwrap().as_ref(), &item2);
        assert!(prov.get(0).unwrap().as_ref() != &item2);
        assert_eq!(prov.get(0).unwrap().as_ref(), &item1);
    }

    #[test]
    pub fn not_found() {
        let posting1 = vec![(10, vec![0, 1, 2, 3, 4]), (1, vec![15])];
        let posting2 = vec![(0, vec![0, 1, 4]), (1, vec![5, 15566, 3423565]), (5, vec![0, 24, 56])];
        assert!(create_dir_all(Path::new("/tmp/test_index")).is_ok());
        let mut prov = FsStorage::new(Path::new("/tmp/test_index"));
        assert!(prov.store(0, posting1.clone()).is_ok());
        assert!(prov.store(1, posting2.clone()).is_ok());
        assert!(if let StorageError::KeyNotFound = prov.get(2).err().unwrap() {
            true
        } else {
            false
        });
    }

    #[test]
    pub fn persistence() {
        let item1 = 1556;
        let item2 = 235425354;
        let item3 = 234543463709865987;
        assert!(create_dir_all(Path::new("/tmp/test_index2")).is_ok());
        {
            let mut prov1 = FsStorage::new(Path::new("/tmp/test_index2"));
            assert!(prov1.store(0, item1.clone()).is_ok());
            assert!(prov1.store(1, item2.clone()).is_ok());
        }

        {
            let mut prov2: FsStorage<usize> = FsStorage::from_folder(Path::new("/tmp/test_index2"));
            assert_eq!(prov2.get(0).unwrap().as_ref(), &item1);
            assert_eq!(prov2.get(1).unwrap().as_ref(), &item2);
            assert!(prov2.store(2, item3.clone()).is_ok());
            assert_eq!(prov2.get(2).unwrap().as_ref(), &item3);
        }

        {
            let prov3: FsStorage<usize> = FsStorage::from_folder(Path::new("/tmp/test_index2"));
            assert_eq!(prov3.get(0).unwrap().as_ref(), &item1);
            assert_eq!(prov3.get(1).unwrap().as_ref(), &item2);
            assert_eq!(prov3.get(2).unwrap().as_ref(), &item3);
        }
    }
}