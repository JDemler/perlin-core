use std::slice;
use std::fmt;
use std::mem;
use std::io;
use std::u64;
use std::ops::{Index, IndexMut};

use page_manager:: {BLOCKSIZE, Block, BlockId};

pub const PAGESIZE: usize = 64;

#[derive(Copy)]
pub struct Page(pub [Block; PAGESIZE]);

#[derive(Copy, Clone, Ord, PartialOrd, PartialEq, Eq, Debug)]
pub struct PageId(pub u64);

impl PageId {
    pub fn none() -> PageId {
        PageId(u64::MAX)
    }
}

#[derive(Clone, Debug)]
pub struct Pages(pub Vec<PageId>, pub Option<UnfullPage>);

#[derive(Copy, Clone, Ord, PartialOrd, PartialEq, Eq, Debug)]
pub struct UnfullPage(pub PageId, BlockId, BlockId);

impl UnfullPage {

    pub fn new(page_id: PageId, from: BlockId, to: BlockId) -> Self {
        UnfullPage(page_id,from,to)
    }

    pub fn page_id(&self) -> PageId {
        self.0
    }
    
    pub fn from(&self) -> BlockId {
        self.1
    }

    pub fn to(&self) -> BlockId {
        self.2
    }
}

impl Pages {
    pub fn new() -> Pages {
        Pages(Vec::new(), None)
    }

    pub fn len(&self) -> usize {
        self.0.len() + self.1.map_or(0, |_| 1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, ptr: usize) -> Option<PageId> {
        if ptr < self.0.len() {
            Some(self.0[ptr])
        } else if self.has_unfull() && ptr < self.len() {
            Some((self.1).unwrap().0)
        } else {
            None
        }
    }
    
    #[inline]
    pub fn push(&mut self, page_id: PageId)  {
        self.0.push(page_id);
    }

    #[inline]
    pub fn add_unfull(&mut self, unfull_page: UnfullPage) {
        self.1 = Some(unfull_page);
    }

    #[inline]
    pub fn take_unfull(&mut self) -> Option<UnfullPage> {
        self.1.take()
    }

    #[inline]
    pub fn unfull(&self) -> Option<UnfullPage> {
        self.1
    }

    #[inline]
    pub fn has_unfull(&self) -> bool {
        self.1.is_some()
    }
}

impl Default for Pages {
    fn default() -> Self {
        Self::new()
    }
}

impl Page {
    pub fn empty() -> Self {
        Page([Block::empty(); PAGESIZE])
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(&self.0[0].0 as *const u8, BLOCKSIZE*PAGESIZE) }
    }

    pub fn from_read<R: io::Read>(source: &mut R) -> Page {
        let mut raw: [u8; BLOCKSIZE*PAGESIZE] = unsafe {mem::uninitialized()};
        source.read_exact(&mut raw).unwrap();
        unsafe {mem::transmute(raw)}
    }

}

impl fmt::Debug for Page {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", &self.0 as &[Block])
    }
}

impl PartialEq for Page {
    fn eq(&self, other: &Self) -> bool {
        &self.0 as &[Block] == &other.0 as &[Block]
    }
}

impl Eq for Page {}

impl Clone for Page {
    fn clone(&self) -> Page { *self }
}


impl Index<BlockId> for Page {
    type Output = Block;
    
    fn index(&self, _index: BlockId) -> &Block {
        &self.0[_index.0 as usize]
    }
}

impl IndexMut<BlockId> for Page {
    fn index_mut(&mut self, _index: BlockId) -> &mut Block {
        &mut self.0[_index.0 as usize]
    }
}
