fn main(x: [Field; 4]) -> pub Field {
	std::hash::pedersen_hash(x)
}
