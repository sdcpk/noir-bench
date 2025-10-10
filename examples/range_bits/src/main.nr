fn main(x: Field) {
	// basic arithmetic to produce constraints
	let y = x * x + x + 1;
	assert(y - (x * x + x + 1) == 0);
}
