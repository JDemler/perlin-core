use std::hash::Hash;
use std::collections::HashMap;

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub struct TermId(pub u64);

pub trait Vocabulary<TTerm> {
    fn get_or_add(&mut self, TTerm) -> TermId;
    fn get(&self, &TTerm) -> Option<TermId>;
}

impl<TTerm> Vocabulary<TTerm> for HashMap<TTerm, TermId> where TTerm: Hash + Eq{
    fn get_or_add(&mut self, term: TTerm) -> TermId {
        if !self.contains_key(&term) {
            let t_id = TermId(self.len() as u64);
            self.insert(term, t_id);
            return t_id;
        }
        *self.get(&term).unwrap()
    }

    #[inline]
    fn get(&self, term: &TTerm) -> Option<TermId> {
        self.get(term).map(|t| *t)
    }
}