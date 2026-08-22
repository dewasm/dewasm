# pack 'd' is a byte copy of the NV, bit-exact even for NaN payloads
# (measured).
sub f64_bits {
    return unpack('Q<', pack('d<', $_[0]));
}
