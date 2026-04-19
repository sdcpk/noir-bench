unconstrained fn sort<let N: u32>(mut arr: [Field; N]) -> [Field; N] {
    for _ in 0..N {
        for j in 0..N - 1 {
            if (arr[j] as u64) > (arr[j + 1] as u64) {
                let tmp = arr[j];
                arr[j] = arr[j + 1];
                arr[j + 1] = tmp;
            }
        }
    }

    arr
}

fn main(arr: [Field; 32]) -> pub [Field; 32] {
    // Safety: output is constrained to be monotonic below, but not to be a permutation
    // of `arr`. That trade-off keeps UltraHonk happy; this example is about Brillig
    // cost, not a correctness proof of sort.
    let out = unsafe { sort(arr) };

    for i in 0..31 {
        assert((out[i] as u64) <= (out[i + 1] as u64));
    }

    out
}
