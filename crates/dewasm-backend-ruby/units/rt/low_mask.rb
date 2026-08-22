# Masking a left-shift operand with LOW_MASK[width - shift] before the shift keeps the intermediate within the wasm width, so MRI never allocates a bignum wider than the result.
# Kept out of the always-bundled rt/_module prelude so a module without rotates does not build the table.
LOW_MASK = Array.new(65) { |n| (1 << n) - 1 }
