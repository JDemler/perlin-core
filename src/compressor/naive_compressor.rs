use std::mem;

use utils::ring_buffer::{BiasedRingBuffer};
use utils::Baseable;
use index::posting::{Posting, DocId};
use page_manager::{BLOCKSIZE, Block};
use compressor::Compressor;


pub struct NaiveCompressor;

impl Compressor for NaiveCompressor {
    fn compress(data: &mut BiasedRingBuffer<Posting>) -> Option<Block>
        where Posting: for<'x> Baseable<&'x Posting>
    {
        if data.count() >= BLOCKSIZE / 4 {
            // Enough in there to fill the block
            let mut block = [0u8; BLOCKSIZE];
            for i in 0..BLOCKSIZE / 4 {
                block[i * 4..(i * 4) + 4].copy_from_slice(unsafe {
                    &mem::transmute::<Posting, [u8; 4]>(data.pop_front_biased().unwrap())
                });
            }
            Some(Block(block))
        } else {
            None
        }
    }

    fn force_compress(data: &mut BiasedRingBuffer<Posting>) -> Block {
        let mut block = [0u8; BLOCKSIZE];
        for i in 0..BLOCKSIZE / 4 {
            let posting = data.pop_front_biased().unwrap_or_else(|| Posting(DocId::none()));
            block[i * 4..(i * 4) + 4]
                .copy_from_slice(unsafe { &mem::transmute::<Posting, [u8; 4]>(posting) });
        }
        Block(block)
    }

    fn decompress(data: Block, target: &mut BiasedRingBuffer<Posting>) {
        let nums: [u32; BLOCKSIZE / 4] = unsafe { mem::transmute(data) };
        for num in &nums {
            let did = DocId(*num);
            if did != DocId::none() {
                target.push_back_biased(Posting(did));
            } else {
                return;
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use utils::ring_buffer::BiasedRingBuffer;
    use index::posting::{DocId, Posting};
    use page_manager::{BLOCKSIZE};
    use compressor::Compressor;

    use super::NaiveCompressor;

    #[test]
    fn compress() {
        let mut buffer = BiasedRingBuffer::<Posting>::new();
        assert_eq!(NaiveCompressor::compress(&mut buffer), None);
        for i in 0..BLOCKSIZE / 4 {
            buffer.push_back(Posting(DocId(i as u32)));
        }
        assert!(NaiveCompressor::compress(&mut buffer).is_some());
        assert_eq!(buffer.count(), 0);
    }

    #[test]
    fn decompress() {
        let mut buffer = BiasedRingBuffer::<Posting>::new();
        assert_eq!(NaiveCompressor::compress(&mut buffer), None);
        for i in 0..BLOCKSIZE / 4 {
            buffer.push_back(Posting(DocId(i as u32)));
        }
        let block = NaiveCompressor::compress(&mut buffer).unwrap();
        assert_eq!(buffer.count(), 0);
        NaiveCompressor::decompress(block, &mut buffer);
        for i in 0..BLOCKSIZE / 4 {
            assert_eq!(buffer.pop_front().unwrap(), Posting(DocId(i as u32)));
        }
    }

    #[test]
    fn force_compress() {
        let mut buffer = BiasedRingBuffer::<Posting>::new();
        assert_eq!(NaiveCompressor::compress(&mut buffer), None);
        buffer.push_back(Posting(DocId(0)));
        buffer.push_back(Posting(DocId(1)));
        assert_eq!(NaiveCompressor::compress(&mut buffer), None);
        let block = NaiveCompressor::force_compress(&mut buffer);
        assert_eq!(buffer.count(), 0);
        NaiveCompressor::decompress(block, &mut buffer);
        assert_eq!(buffer.pop_front().unwrap(), Posting(DocId(0)));
        assert_eq!(buffer.pop_front().unwrap(), Posting(DocId(1)));
        assert_eq!(buffer.pop_front(), None);
    }

}
