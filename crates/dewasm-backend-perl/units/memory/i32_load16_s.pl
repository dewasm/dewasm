# requires: memory/i32_load16_u, rt/sext
sub i32_load16_s {
    my ($self, $a) = @_;
    return Rt::sext($self->i32_load16_u($a), 16, 0xFFFFFFFF);
}
