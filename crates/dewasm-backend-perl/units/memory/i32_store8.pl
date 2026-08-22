sub i32_store8 {
    my ($self, $a, $v) = @_;
    $self->check($a, 1);
    vec($self->{data}, $a, 8) = $v & 0xFF;
}
