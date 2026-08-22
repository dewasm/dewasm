sub i64_load16_u {
    my ($self, $a) = @_;
    $self->check($a, 2);
    return unpack('v', substr($self->{data}, $a, 2));
}
