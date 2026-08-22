sub f64_from_bits {
    return unpack('d<', pack('Q<', $_[0]));
}
