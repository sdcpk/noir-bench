global MAX_DEPTH: u32 = 16;

fn merkle_parent(left: Field, right: Field) -> Field {
    std::hash::poseidon2_permutation([left, right, 0, 0])[0]
}

fn compute_root(
    leaf_value: Field,
    path: [Field; MAX_DEPTH],
    index_bits: [bool; MAX_DEPTH],
    depth: u32,
) -> Field {
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
    current
}

fn main(
    leaf_value: Field,
    path: [Field; MAX_DEPTH],
    index_bits: [bool; MAX_DEPTH],
    depth: u32,
    root: pub Field,
) {
    assert(depth <= MAX_DEPTH);
    let computed = compute_root(leaf_value, path, index_bits, depth);
    assert(computed == root);
}

#[test]
fn print_example_root() {
	let leaf_value: Field = 5;
	let mut path: [Field; MAX_DEPTH] = [0; MAX_DEPTH];
	let mut index_bits: [bool; MAX_DEPTH] = [false; MAX_DEPTH];
	let depth: u32 = 3;

	// pick three arbitrary siblings for our depth-3 path
	path[0] = 11;
	path[1] = 22;
	path[2] = 33;

    // position of our leaf: bits are right, left, right (reading bottom-up)
    index_bits[0] = true;
    index_bits[1] = false;
    index_bits[2] = true;
    let root = compute_root(leaf_value, path, index_bits, depth);
    println(root);
}