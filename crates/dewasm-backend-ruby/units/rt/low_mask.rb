# A left-shift operand is masked with LOW_MASK[width - shift] before the shift.
# That keeps the intermediate within the wasm width.
# So MRI never allocates a bignum wider than the result.
# Kept out of the always-bundled `rt/_module` prelude.
# So a module without rotates does not build the table.
LOW_MASK = Array.new(65) { |n| (1 << n) - 1 }
