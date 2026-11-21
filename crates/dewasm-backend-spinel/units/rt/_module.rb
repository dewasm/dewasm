M32 = 0xffff_ffff
# An i64 is a signed 64-bit word, so the word with every bit set is -1.
M64 = -1
I64_MIN = -9223372036854775808

# Every i64 operation relies on the host wrapping `+`, `-`, `*` and `<<` at 64 bits.
# Spinel does so under `--int-overflow=wrap` only.
raise "this program needs `spinel --int-overflow=wrap`" unless 0x7fff_ffff_ffff_ffff + 1 == I64_MIN

module_function
