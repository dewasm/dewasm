# requires: memory/i32_load8_u, rt/sext
sub i32_load8_s {
    my ($self, $a) = @_;
    return Rt::sext($self->i32_load8_u($a), 8, 0xFFFFFFFF);
}
