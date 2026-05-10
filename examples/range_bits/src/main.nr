global MAX_BITS: u32 = 64;

fn main(x: Field, bit_width: u32) {
    assert(bit_width <= MAX_BITS);

    let bits: [bool; MAX_BITS] = x.to_le_bits();
    for i in 0..MAX_BITS {
        if (i as u32) >= bit_width {
            assert(!bits[i]);
        }
    }
}
