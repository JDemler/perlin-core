//! This module provides the implementation for boolean information retrieval
//! Use `IndexBuilder` to build indices
//! Use `QueryBuilder` to build queries that run on these indices
use std;
use std::io;
use std::path::{Path, PathBuf};
use std::collections::BTreeMap;
use std::iter::Iterator;
use std::fs::OpenOptions;
use std::io::{Read, Write};

use index::Index;
use storage::{Storage, StorageError};
use index::boolean_index::boolean_query::*;
use index::boolean_index::query_result_iterator::*;
use index::boolean_index::query_result_iterator::nary_query_iterator::*;
use index::boolean_index::posting::{Listing, Posting};

use storage::{vbyte_encode, VByteDecoder, ByteEncodable, ByteDecodable};
use utils::owning_iterator::ArcIter;
use utils::persistence::Persistent;

pub use index::boolean_index::query_builder::QueryBuilder;
pub use index::boolean_index::index_builder::IndexBuilder;

mod query_result_iterator;
mod index_builder;
mod query_builder;
mod posting;
mod boolean_query;



const VOCAB_FILENAME: &'static str = "vocabulary.bin";
const STATISTICS_FILENAME: &'static str = "statistics.bin";
const CHUNKSIZE: usize = 1_000_000;

/// A specialized `Result` type for operations related to `BooleanIndex`
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
/// Error kinds that can occur during operations related to `BooleanIndex`
pub enum Error {
    /// A persistent `BooleanIndex` should be build but no path where to persist it was specified
    /// Call the `IndexBuilder::persist()`
    PersistPathNotSpecified,
    /// A `BooleanIndex` should be loaded from a directory but the specified directory is empty
    EmptyPersistPath,
    /// Tried to load a `BooleanIndex` from a corrupted file
    CorruptedIndexFile,
    /// An IO-Error occured
    IO(io::Error),
    /// A Storage-Error occured
    Storage(StorageError),
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::IO(err)
    }
}

impl From<StorageError> for Error {
    fn from(err: StorageError) -> Self {
        Error::Storage(err)
    }
}

/// Implements the `Index` trait. Limited to boolean retrieval.
pub struct BooleanIndex<TTerm: Ord> {
    document_count: usize,
    term_ids: BTreeMap<TTerm, u64>,
    postings: Box<Storage<Listing>>,
    persist_path: Option<PathBuf>,
}

// Index implementation
impl<'a, TTerm: Ord> Index<'a, TTerm> for BooleanIndex<TTerm> {
    type Query = BooleanQuery<TTerm>;
    type QueryResult = Box<Iterator<Item = u64>>;

    /// Executes a `BooleanQuery` and returns a boxed iterator over the resulting document ids.
    /// The query execution is lazy.
    fn execute_query(&'a self, query: &Self::Query) -> Self::QueryResult {
        Box::new(self.run_query(query))
    }
}

impl<TTerm> BooleanIndex<TTerm>
    where TTerm: Ord + ByteDecodable + ByteEncodable
{
    /// Load a `BooleanIndex` from a previously populated folder
    /// Not intended for public use. Please use the `IndexBuilder` instead
    fn load<TStorage>(path: &Path) -> Result<Self>
        where TStorage: Storage<Listing> + Persistent + 'static
    {
        let storage = TStorage::load(path);
        let vocab = try!(Self::load_vocabulary(path));
        let doc_count = try!(Self::load_statistics(path));
        BooleanIndex::from_parts(Box::new(storage), vocab, doc_count)
    }

    /// Creates a new `BooleanIndex` instance which is written to the passed
    /// path
    /// Not intended for public use. Please use the `IndexBuilder` instead
    fn new_persistent<TDocsIterator, TDocIterator, TStorage>(storage: TStorage,
                                                             documents: TDocsIterator,
                                                             path: &Path)
                                                             -> Result<Self>
        where TDocsIterator: Iterator<Item = TDocIterator>,
              TDocIterator: Iterator<Item = TTerm>,
              TStorage: Storage<Listing> + Persistent + 'static
    {
        let mut index = BooleanIndex {
            document_count: 0,
            term_ids: BTreeMap::new(),
            postings: Box::new(storage),
            persist_path: Some(path.to_path_buf()),
        };
        try!(index.index_documents(documents));
        try!(index.save_vocabulary());
        try!(index.save_statistics());
        Ok(index)
    }

    fn save_vocabulary(&self) -> Result<()> {
        if let Some(filename) = self.persist_path.as_ref().map(|p| p.join(VOCAB_FILENAME)) {
            // Open file
            let mut vocab_file = try!(OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(filename));
            // Iterate over vocabulary and encode data
            let mut byte_buffer = Vec::with_capacity(2 * CHUNKSIZE);
            for vocab_entry in &self.term_ids {
                // Encode term and number of its bytes
                let term_bytes = vocab_entry.0.encode();
                let term_length_bytes = vbyte_encode(term_bytes.len());
                // Encode id
                let id_bytes = vbyte_encode(*vocab_entry.1 as usize);

                // Append id, term length and term to byte_buffer
                byte_buffer.extend_from_slice(&id_bytes);
                byte_buffer.extend_from_slice(&term_length_bytes);
                byte_buffer.extend_from_slice(&term_bytes);

                // Write if buffer is full
                if byte_buffer.len() > CHUNKSIZE {
                    try!(vocab_file.write(&byte_buffer));
                    byte_buffer.clear();
                }
            }
            if !byte_buffer.is_empty() {
                // If some rests are in the buffer write them to file
                try!(vocab_file.write(&byte_buffer));
            }
            Ok(())
        } else {
            Err(Error::PersistPathNotSpecified)
        }
    }

    fn load_vocabulary(path: &Path) -> Result<BTreeMap<TTerm, u64>> {
        // Open file
        let mut vocab_file = try!(OpenOptions::new().read(true).open(path.join(VOCAB_FILENAME)));
        let metadata = try!(vocab_file.metadata());
        // Read bytes into Vector
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        try!(vocab_file.read_to_end(&mut bytes));
        // Create a decoder from that vector
        let mut decoder = VByteDecoder::new(bytes.into_iter());
        let mut result = BTreeMap::new();
        while let Some(id) = decoder.next() {
            if let Some(term_len) = decoder.next() {
                if let Ok(term) = TTerm::decode(decoder.underlying_iterator().take(term_len)) {
                    result.insert(term, id as u64);
                } else {
                    // Error while decoding a term. TODO: Propagate Error
                }
            } else {
                // Term len could not be decoded. TODO: Error Handling
            }
        }
        Ok(result)
    }

    fn save_statistics(&self) -> Result<()> {
        // Open file
        if let Some(filename) = self.persist_path.as_ref().map(|p| p.join(STATISTICS_FILENAME)) {
            let mut statistics_file = try!(OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(filename));
            try!(statistics_file.write(&vbyte_encode(self.document_count)));
            Ok(())
        } else {
            Err(Error::PersistPathNotSpecified)
        }
    }

    fn load_statistics(path: &Path) -> Result<usize> {
        let mut statistics_file =
            try!(OpenOptions::new().read(true).open(path.join(STATISTICS_FILENAME)));
        let mut bytes = Vec::new();
        try!(statistics_file.read_to_end(&mut bytes));
        if let Some(doc_count) = VByteDecoder::new(bytes.into_iter()).next() {
            Ok(doc_count)
        } else {
            Err(Error::CorruptedIndexFile)
        }
    }
}

impl<TTerm: Ord> BooleanIndex<TTerm> {
    /// Creates a new volatile `BooleanIndex`. Not intended for public use.
    /// Please use `IndexBuilder` instead
    fn new<TDocsIterator, TDocIterator, TStorage>(storage: TStorage,
                                                  documents: TDocsIterator)
                                                  -> Result<Self>
        where TDocsIterator: Iterator<Item = TDocIterator>,
              TDocIterator: Iterator<Item = TTerm>,
              TStorage: Storage<Listing> + 'static
    {
        let mut index = BooleanIndex {
            document_count: 0,
            term_ids: BTreeMap::new(),
            postings: Box::new(storage),
            persist_path: None,
        };
        try!(index.index_documents(documents));
        Ok(index)
    }


    fn from_parts(inverted_index: Box<Storage<Listing>>,
                  vocabulary: BTreeMap<TTerm, u64>,
                  document_count: usize)
                  -> Result<Self> {
        Ok(BooleanIndex {
            document_count: document_count,
            term_ids: vocabulary,
            postings: inverted_index,
            persist_path: None,
        })
    }

    /// Indexes a document collection for later retrieval
    /// Returns the number of documents indexed
    // First Shot. TODO: Needs serious improvement!
    fn index_documents<TDocsIterator, TDocIterator>(&mut self,
                                                    documents: TDocsIterator)
                                                    -> Result<(usize)>
        where TDocsIterator: Iterator<Item = TDocIterator>,
              TDocIterator: Iterator<Item = TTerm>
    {
        let mut inv_index: BTreeMap<u64, Vec<Posting>> = BTreeMap::new();
        // For every document in the collection
        for document in documents {
            // Determine its id. consecutively numbered
            let new_doc_id = self.document_count as u64;
            // Enumerate over its terms
            for (term_position, term) in document.into_iter().enumerate() {
                // Has term already been seen? Is it already in the vocabulary?
                if let Some(term_id) = self.term_ids.get(&term) {
                    // Get its listing from the temporary. And add doc_id and/or position to it
                    let listing = inv_index.get_mut(term_id).unwrap();
                    match listing.binary_search_by(|&(doc_id, _)| doc_id.cmp(&new_doc_id)) {
                        Ok(term_doc_index) => {
                            // Document already had that term.
                            // Look for where to put the current term in the positions list
                            let term_doc_positions =
                                &mut listing.get_mut(term_doc_index).unwrap().1;
                            if let Err(index) =
                                   term_doc_positions.binary_search(&(term_position as u32)) {
                                term_doc_positions.insert(index, term_position as u32)
                            }
                            // Two terms at the same position. Should at least be possible
                            // so do nothing if term_position already exists
                        }
                        Err(term_doc_index) => {
                            listing.insert(term_doc_index,
                                           (new_doc_id as u64, vec![term_position as u32]))
                        }
                    }
                    // Term is indexed. Continue with the next one
                    continue;
                };
                // Term was not yet indexed. Add it
                let term_id = self.term_ids.len() as u64;
                self.term_ids.insert(term, term_id);
                inv_index.insert(term_id, vec![(new_doc_id, vec![term_position as u32])]);
            }
            self.document_count += 1;
        }
        
        // everything is now indexed. Hand it to our storage.
        // We do not care where it saves our data.
        for (term_id, listing) in inv_index {
            try!(self.postings.store(term_id, listing));
        }

        Ok(self.document_count)
    }


    fn run_query(&self, query: &BooleanQuery<TTerm>) -> QueryResultIterator {
        match *query {
            BooleanQuery::Atom(ref atom) => self.run_atom(atom.relative_position, &atom.query_term),
            BooleanQuery::NAry(ref operator, ref operands) => {
                self.run_nary_query(operator, operands)
            }
            BooleanQuery::Positional(ref operator, ref operands) => {
                self.run_positional_query(operator, operands)
            }
            BooleanQuery::Filter(ref operator, ref sand, ref sieve) => {
                self.run_filter(operator, sand.as_ref(), sieve.as_ref())
            }

        }

    }

    fn run_nary_query(&self,
                      operator: &BooleanOperator,
                      operands: &[BooleanQuery<TTerm>])
                      -> QueryResultIterator {
        let mut ops = Vec::new();
        for operand in operands {
            ops.push(self.run_query(operand))
        }
        QueryResultIterator::NAry(NAryQueryIterator::new(*operator, ops))
    }

    fn run_positional_query(&self,
                            operator: &PositionalOperator,
                            operands: &[QueryAtom<TTerm>])
                            -> QueryResultIterator {
        let mut ops = Vec::new();
        for operand in operands {
            ops.push(self.run_atom(operand.relative_position, &operand.query_term));
        }
        QueryResultIterator::NAry(NAryQueryIterator::new_positional(*operator, ops))
    }

    fn run_filter(&self,
                  operator: &FilterOperator,
                  sand: &BooleanQuery<TTerm>,
                  sieve: &BooleanQuery<TTerm>)
                  -> QueryResultIterator {
        QueryResultIterator::Filter(FilterIterator::new(*operator,
                                                        Box::new(self.run_query(sand)),
                                                        Box::new(self.run_query(sieve))))
    }


    fn run_atom(&self, relative_position: usize, atom: &TTerm) -> QueryResultIterator {
        if let Some(result) = self.term_ids.get(atom) {
            QueryResultIterator::Atom(relative_position,
                                      ArcIter::new(self.postings.get(*result).unwrap()))
        } else {
            QueryResultIterator::Empty
        }
    }
}



// --- Tests

#[cfg(test)]
mod tests {

    use std::fs::create_dir_all;
    use std::path::Path;

    use super::*;
    use index::boolean_index::boolean_query::*;

    use index::Index;
    use storage::{FsStorage, RamStorage};


    pub fn prepare_index() -> BooleanIndex<usize> {
        let index = IndexBuilder::<_, RamStorage<_>>::new()
            .create(vec![(0..10).collect::<Vec<_>>().into_iter(),
                         (0..10).map(|i| i * 2).collect::<Vec<_>>().into_iter(),
                         vec![5, 4, 3, 2, 1, 0].into_iter()]
                .into_iter());
        index.unwrap()
    }

    #[test]
    fn empty_query() {
        let index = prepare_index();
        assert!(index.execute_query(&BooleanQuery::Atom(QueryAtom::new(0, 15)))
            .collect::<Vec<_>>() == vec![]);

    }



    #[test]
    fn indexing() {
        let index = prepare_index();
        // Check number of docs
        assert!(index.document_count == 3);
        // Check number of terms (0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 14, 16, 18)
        assert!(index.term_ids.len() == 15);
        assert!(*index.postings.get(*index.term_ids.get(&0).unwrap()).unwrap() ==
                vec![(0, vec![0]), (1, vec![0]), (2, vec![5])]);
    }

    #[test]
    fn query_atom() {
        let index = prepare_index();

        assert!(index.execute_query(&BooleanQuery::Atom(QueryAtom::new(0, 7)))
            .collect::<Vec<_>>() == vec![0]);
        assert!(index.execute_query(&BooleanQuery::Atom(QueryAtom::new(0, 5)))
            .collect::<Vec<_>>() == vec![0, 2]);
        assert!(index.execute_query(&BooleanQuery::Atom(QueryAtom::new(0, 0)))
            .collect::<Vec<_>>() == vec![0, 1, 2]);
        assert!(index.execute_query(&BooleanQuery::Atom(QueryAtom::new(0, 16)))
            .collect::<Vec<_>>() == vec![1]);
    }

    #[test]
    fn nary_query() {
        let index = prepare_index();

        assert!(index.execute_query(&BooleanQuery::NAry(BooleanOperator::And,
                                               vec![BooleanQuery::Atom(QueryAtom::new(0, 5)),
                                                    BooleanQuery::Atom(QueryAtom::new(0, 0))]))
            .collect::<Vec<_>>() == vec![0, 2]);
        assert!(index.execute_query(&BooleanQuery::NAry(BooleanOperator::And,
                                               vec![BooleanQuery::Atom(QueryAtom::new(0, 0)),
                                                    BooleanQuery::Atom(QueryAtom::new(0, 5))]))
            .collect::<Vec<_>>() == vec![0, 2]);
    }

    #[test]
    fn and_query() {
        let index = prepare_index();
        assert!(index.execute_query(&BooleanQuery::NAry(BooleanOperator::And,
                                               vec![BooleanQuery::Atom(QueryAtom::new(0, 3)),
                                                    BooleanQuery::Atom(QueryAtom::new(0,
                                                                                      12))]))
            .collect::<Vec<_>>() == vec![]);
        assert!(index.execute_query(&BooleanQuery::NAry(BooleanOperator::And,
                                               vec![BooleanQuery::Atom(QueryAtom::new(0,
                                                                                      14)),
                                                    BooleanQuery::Atom(QueryAtom::new(0,
                                                                                      12))]))
            .collect::<Vec<_>>() == vec![1]);
        assert!(index.execute_query(&BooleanQuery::NAry(BooleanOperator::And,
                                               vec![BooleanQuery::NAry(BooleanOperator::And,
                    vec![BooleanQuery::Atom(QueryAtom::new(0, 3)),
                        BooleanQuery::Atom(QueryAtom::new(0, 9))]
                    ),
                    BooleanQuery::Atom(QueryAtom::new(0, 12))]))
            .collect::<Vec<_>>() == vec![]);
        assert!(index.execute_query(&BooleanQuery::NAry(BooleanOperator::And,
                                               vec![BooleanQuery::NAry(BooleanOperator::And,
                    vec![BooleanQuery::Atom(QueryAtom::new(0, 2)),
                        BooleanQuery::Atom(QueryAtom::new(0, 4))]
                    ),
                    BooleanQuery::Atom(QueryAtom::new(0, 16))]))
            .collect::<Vec<_>>() == vec![1]);
    }

    #[test]
    fn or_query() {
        let index = prepare_index();
        assert_eq!(index.execute_query(&BooleanQuery::NAry(BooleanOperator::Or,
                                               vec![BooleanQuery::Atom(QueryAtom::new(0, 3)),
                                                    BooleanQuery::Atom(QueryAtom::new(0,
                                                                                      12))]))
            .collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(index.execute_query(&BooleanQuery::NAry(BooleanOperator::Or,
                                                          vec![BooleanQuery::Atom(QueryAtom::new(0,
                                                                                      14)),
                                                    BooleanQuery::Atom(QueryAtom::new(0,
                                                                                      12))]))
                       .collect::<Vec<_>>(),
                   vec![1]);
        assert_eq!(index.execute_query(&BooleanQuery::NAry(BooleanOperator::Or,
                                               vec![BooleanQuery::NAry(BooleanOperator::Or,
                    vec![BooleanQuery::Atom(QueryAtom::new(0, 3)),
                        BooleanQuery::Atom(QueryAtom::new(0, 9))]
                    ),
                    BooleanQuery::Atom(QueryAtom::new(0, 16))]))
            .collect::<Vec<_>>(), vec![0, 1, 2]);
    }

    #[test]
    fn inorder_query() {
        let index = prepare_index();
        assert!(index.execute_query(&BooleanQuery::Positional(PositionalOperator::InOrder,
                                                     vec![QueryAtom::new(0, 0),
                                                          QueryAtom::new(1, 1)]))
            .collect::<Vec<_>>() == vec![0]);
        assert!(index.execute_query(&BooleanQuery::Positional(PositionalOperator::InOrder,
                                                     vec![QueryAtom::new(1, 0),
                                                          QueryAtom::new(0, 1)]))
            .collect::<Vec<_>>() == vec![2]);
        assert!(index.execute_query(&BooleanQuery::Positional(PositionalOperator::InOrder,
                                                     vec![QueryAtom::new(0, 0),
                                                          QueryAtom::new(1, 2)]))
            .collect::<Vec<_>>() == vec![1]);

        assert!(index.execute_query(&BooleanQuery::Positional(PositionalOperator::InOrder,
                                                     vec![QueryAtom::new(2, 2),
                                                          QueryAtom::new(1, 1),
                                                          QueryAtom::new(0, 0)]))
            .collect::<Vec<_>>() == vec![0]);
        assert!(index.execute_query(&BooleanQuery::Positional(PositionalOperator::InOrder,
                                                     vec![QueryAtom::new(0, 2),
                                                          QueryAtom::new(1, 1),
                                                          QueryAtom::new(2, 0)]))
            .collect::<Vec<_>>() == vec![2]);
        assert!(index.execute_query(&BooleanQuery::Positional(PositionalOperator::InOrder,
                                                     vec![QueryAtom::new(0, 2),
                                                          QueryAtom::new(1, 1),
                                                          QueryAtom::new(3, 0)]))
            .collect::<Vec<_>>() == vec![]);
    }

    #[test]
    fn query_filter() {
        let index = prepare_index();
        assert!(index.execute_query(
            &BooleanQuery::Filter(FilterOperator::Not,
            Box::new(BooleanQuery::NAry(
                BooleanOperator::And,
                vec![BooleanQuery::Atom(QueryAtom::new(0, 2)),
                     BooleanQuery::Atom(QueryAtom::new(0, 0))])),
                      Box::new(BooleanQuery::Atom(
                          QueryAtom::new(0, 16))))).collect::<Vec<_>>() == vec![0,2]);
    }


    #[test]
    fn persistence() {
        assert!(create_dir_all(Path::new("/tmp/persistent_index_test")).is_ok());
        {
            let index = IndexBuilder::<u32, FsStorage<_>>::new()
                .persist(Path::new("/tmp/persistent_index_test"))
                .create_persistent(vec![(0..10).collect::<Vec<_>>().into_iter(),
                                        (0..10).map(|i| i * 2).collect::<Vec<_>>().into_iter(),
                                        vec![5, 4, 3, 2, 1, 0].into_iter()]
                    .into_iter())
                .unwrap();

            assert!(index.execute_query(&BooleanQuery::Atom(QueryAtom::new(0, 7)))
                .collect::<Vec<_>>() == vec![0]);
            assert!(index.execute_query(&BooleanQuery::Atom(QueryAtom::new(0, 5)))
                .collect::<Vec<_>>() == vec![0, 2]);
            assert!(index.execute_query(&BooleanQuery::Atom(QueryAtom::new(0, 0)))
                .collect::<Vec<_>>() == vec![0, 1, 2]);
            assert!(index.execute_query(&BooleanQuery::Atom(QueryAtom::new(0, 16)))
                .collect::<Vec<_>>() == vec![1]);
        }

        {
            let index = IndexBuilder::<usize, FsStorage<_>>::new()
                .persist(Path::new("/tmp/persistent_index_test"))
                .load()
                .unwrap();
            assert!(index.execute_query(&BooleanQuery::Atom(QueryAtom::new(0, 7)))
                .collect::<Vec<_>>() == vec![0]);
            assert!(index.execute_query(&BooleanQuery::Atom(QueryAtom::new(0, 5)))
                .collect::<Vec<_>>() == vec![0, 2]);
            assert!(index.execute_query(&BooleanQuery::Atom(QueryAtom::new(0, 0)))
                .collect::<Vec<_>>() == vec![0, 1, 2]);
            assert!(index.execute_query(&BooleanQuery::Atom(QueryAtom::new(0, 16)))
                .collect::<Vec<_>>() == vec![1]);
        }
    }
}
