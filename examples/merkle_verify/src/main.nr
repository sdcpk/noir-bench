global MAX_DEPTH: u32 = 32;

fn merkle_parent(current: Field, sibling: Field, index_bit: bool) -> Field {
    let (left, right) = if index_bit { (sibling, current) } else { (current, sibling) };
    std::hash::pedersen_hash([left, right])
}

fn main(leaf: Field, path: [Field; MAX_DEPTH], index_bits: [bool; MAX_DEPTH], depth: u32, root: pub Field) {
    assert(depth <= MAX_DEPTH);

    let mut current = leaf;
    for i in 0..MAX_DEPTH {
        let is_active = (i as u32) < depth;

        if !is_active {
            assert(path[i] == 0);
            assert(!index_bits[i]);
        }

        let candidate = merkle_parent(current, path[i], index_bits[i]);
        current = if is_active { candidate } else { current };
    }

    assert(current == root);
}








