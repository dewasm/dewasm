sub i64_load8_u {
    my ($self, $a) = @_;
    $self->check($a, 1);
    return vec($self->{data}, $a, 8);
}
