# requires: memory/i32_store, rt/f32_bits
sub f32_store {
    my ($self, $a, $v) = @_;
    $self->i32_store($a, Rt::f32_bits($v));
}
