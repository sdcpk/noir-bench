global MAX_DEPTH: u32 = 32;

fn merkle_parent(left: Field, right: Field) -> Field {
    std::hash::poseidon2_permutation([left, right])[0]
}

fn main(
    leaf_value: Field,
    path: [Field; MAX_DEPTH],
    index_bits: [bool; MAX_DEPTH],
    depth: u32,
    root: pub Field,
) {
    assert(depth <= MAX_DEPTH);
    let mut current = leaf_value;
    for i in 0..MAX_DEPTH {
        let is_active = (i as u32) < depth;
        let (left, right) = if index_bits[i] {
            (path[i], current)
        } else {
            (current, path[i])
        };
        let candidate = merkle_parent(left, right);
        current = if is_active { candidate } else { current };
    }
    assert(current == root);
}
