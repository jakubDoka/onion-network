pub fn verify(a: &[u8], b: &[u8]) -> bool {
    a.iter().zip(b).fold(0, |acc, (&a, &b)| acc | (a ^ b)) == 0
}
