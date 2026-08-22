# requires: rt/sext
sub i64_extend16_s {
    return Rt::sext($_[0], 16, 0xFFFFFFFFFFFFFFFF);
}
