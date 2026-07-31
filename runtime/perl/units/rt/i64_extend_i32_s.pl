# requires: rt/s32
sub i64_extend_i32_s {
    return Rt::s32($_[0]) & 0xFFFFFFFFFFFFFFFF;
}
