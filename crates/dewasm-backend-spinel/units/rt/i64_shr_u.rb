# `n` is in `0..63`.
# The host's `>>` copies the sign bit, and the mask clears the copies.
def i64_shr_u(x, n) = n == 0 ? x : (x >> n) & (0x7fff_ffff_ffff_ffff >> (n - 1))
