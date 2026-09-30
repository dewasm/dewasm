# Perl's UV/IV -> NV conversion is a correctly-rounded C cast.
# But `0.0 +` on an integral operand takes the integer fast path.
# That path keeps integer exactness beyond 2^53 (measured).
# The pack 'd' round-trip forces the IEEE-rounded double wasm's convert requires.
sub cvt_f64_i {
    return unpack('d<', pack('d<', 0.0 + $_[0]));
}
