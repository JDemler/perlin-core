use std::io::{Read, Result, Write};
use std;

pub mod boolean_index;

pub trait Index<TTerm> {
    type Query;
    type QueryResult;
    
    fn new() -> Self;

    fn index_document<TDocIterator: Iterator<Item=TTerm>>(&mut self, document: TDocIterator) -> u64;

    fn execute_query(&self, query: &Self::Query) -> Self::QueryResult;
}


/// Defines API calls for writing and reading an index from/to binary
/// Can be used for example to persist an Index as a file or send it as `TcpStream`.
pub trait PersistentIndex where Self : Sized {
    
    /// Writes the index as byte to the specified target.
    /// Returns Error or the number of bytes written
    fn write_to<TTarget: Write>(&self, target: &mut TTarget) -> Result<usize>;

    /// Reads an index from the specified source.
    fn read_from<TSource: Read>(source: &mut TSource) -> std::result::Result<Self, String>;
}


pub trait ByteEncodable {
    fn encode(&self) -> Vec<u8>;
}

pub trait ByteDecodable where Self: Sized {
    fn decode(Vec<u8>) -> std::result::Result<Self, String>;
}

