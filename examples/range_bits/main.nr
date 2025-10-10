fn main(x: Field) {
	// force bit decomposition and range checks
	let b0 = constrain_to_bits(x, 16);
	assert(b0[0] + b0[1] + b0[2] + b0[3] >= 0);
}
