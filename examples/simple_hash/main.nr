fn poseidon_hash(xs: [Field; 4]) -> Field {
	let mut acc = 0;
	for i in 0..4 {
		acc = hash(acc + xs[i]);
	}
	acc
}

fn main(x: [Field; 4]) {
	let _ = poseidon_hash(x);
}










