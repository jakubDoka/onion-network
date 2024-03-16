#![feature(iter_intersperse)]

use {crypto::Hash, std::iter};

pub trait MerkleHash: Default + Copy {
    fn combine(a: Self, b: Self) -> Self;
}

impl MerkleHash for Hash {
    fn combine(a: Self, b: Self) -> Self {
        crypto::hash::combine(a, b)
    }
}

impl MerkleHash for usize {
    fn combine(a: Self, b: Self) -> Self {
        a + b
    }
}

pub struct MerkleTree<T> {
    nodes: Vec<T>,
}

impl<T: MerkleHash> MerkleTree<T> {
    pub fn new(root: T) -> Self {
        Self { nodes: vec![root] }
    }

    pub fn from_base(base: impl IntoIterator<Item = T>) -> Self {
        fn comute_recur<T: MerkleHash>(nodes: &mut [T]) -> T {
            if let &mut [node] = nodes {
                return node;
            }

            let mid = nodes.len() / 2;
            let (left, right) = nodes.split_at_mut(mid);
            let (mid, right) = right.split_first_mut().unwrap();

            *mid = T::combine(comute_recur(left), comute_recur(right));
            *mid
        }

        let mut nodes = base.into_iter().intersperse(Default::default()).collect::<Vec<_>>();

        comute_recur(&mut nodes);

        Self { nodes }
    }

    #[must_use]
    pub fn root(&self) -> &T {
        &self.nodes[(self.nodes.len().next_power_of_two() / 2) - 1]
    }

    pub fn psuh(&mut self, value: T) {
        self.nodes.extend([Default::default(), value]);

        let mut cursor = self.nodes.len() - 1;
        let mut clamp = cursor;
        let mut width = 1;
        let mut direction_mask = self.nodes.len() >> 1;
        for _ in 0..self.nodes.len().ilog2() {
            if direction_mask & 1 == 0 {
                cursor += width;
            } else {
                cursor -= width;
                self.nodes[cursor] =
                    T::combine(self.nodes[cursor - width], self.nodes[(cursor + width).min(clamp)]);
                clamp = cursor;
            }
            width <<= 1;
            direction_mask >>= 1;
        }
    }

    pub fn proof(&self, index: usize) -> impl Iterator<Item = &T> {
        let mut clamp_cursor = self.nodes.len() - 1;
        let mut clamp = clamp_cursor;
        let mut clamp_mask = self.nodes.len() >> 1;

        let mut cursor = index;
        let mut width = 1;
        let mut direction_mask = (index + 1) >> 1;
        let mut fuel = self.nodes.len().ilog2();

        iter::from_fn(move || loop {
            fuel = fuel.checked_sub(1)?;

            let opposite = if direction_mask & 1 == 0 {
                cursor += width;
                cursor + width
            } else {
                cursor -= width;
                cursor - width
            };

            let value = if clamp_mask & 1 == 0 {
                clamp_cursor += width;
                self.nodes.get(opposite)
            } else {
                clamp_cursor -= width;
                let value = &self.nodes[opposite.min(clamp)];
                clamp = clamp_cursor;
                Some(value)
            };

            width <<= 1;
            direction_mask >>= 1;
            clamp_mask >>= 1;

            if let Some(value) = value {
                return Some(value);
            }
        })
    }

    pub fn clear(&mut self, root: T) {
        self.nodes.clear();
        self.nodes.push(root);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_next_insert_index() {
        #[rustfmt::skip]
        let seq = &[
            &[1][..],
            &[1, 2, 1],
            &[1, 2, 1, 3, 1],
            &[1, 2, 1, 4, 1, 2, 1],
            &[1, 2, 1, 4, 1, 2, 1, 5, 1],
            &[1, 2, 1, 4, 1, 2, 1, 6, 1, 2, 1],
            &[1, 2, 1, 4, 1, 2, 1, 7, 1, 2, 1, 3, 1],
            &[1, 2, 1, 4, 1, 2, 1, 8, 1, 2, 1, 4, 1, 2, 1],
            &[1, 2, 1, 4, 1, 2, 1, 8, 1, 2, 1, 4, 1, 2, 1, 9, 1],
            &[1, 2, 1, 4, 1, 2, 1, 8, 1, 2, 1, 4, 1, 2, 1, 10, 1, 2, 1],
            &[1, 2, 1, 4, 1, 2, 1, 8, 1, 2, 1, 4, 1, 2, 1, 11, 1, 2, 1, 3, 1],
            &[1, 2, 1, 4, 1, 2, 1, 8, 1, 2, 1, 4, 1, 2, 1, 12, 1, 2, 1, 4, 1, 2, 1],
            &[1, 2, 1, 4, 1, 2, 1, 8, 1, 2, 1, 4, 1, 2, 1, 13, 1, 2, 1, 4, 1, 2, 1, 5, 1],
            &[1, 2, 1, 4, 1, 2, 1, 8, 1, 2, 1, 4, 1, 2, 1, 14, 1, 2, 1, 4, 1, 2, 1, 6, 1, 2, 1],
            &[1, 2, 1, 4, 1, 2, 1, 8, 1, 2, 1, 4, 1, 2, 1, 15, 1, 2, 1, 4, 1, 2, 1, 7, 1, 2, 1, 3, 1],
            &[1, 2, 1, 4, 1, 2, 1, 8, 1, 2, 1, 4, 1, 2, 1, 16, 1, 2, 1, 4, 1, 2, 1, 8, 1, 2, 1, 4, 1, 2, 1],
        ];

        let mut tree = MerkleTree::new(1);
        for &seq in seq {
            assert_eq!(tree.nodes, seq);
            tree.psuh(1);
        }
    }

    #[test]
    fn fuzz_merkle_tree() {
        let mut tree = MerkleTree::new(1);

        for i in 0..1000 {
            tree.psuh(i);

            for (i, e) in (2..tree.nodes.len()).step_by(2).zip(0..) {
                let proof = tree.proof(i);
                let hash = proof.copied().fold(e, MerkleHash::combine);
                assert_eq!(&hash, tree.root());
            }
        }
    }
}
