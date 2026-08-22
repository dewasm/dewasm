# requires: rt/sext
sub i32_extend16_s {
    return Rt::sext($_[0], 16, 0xFFFFFFFF);
}
