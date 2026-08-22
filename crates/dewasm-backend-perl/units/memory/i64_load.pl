sub i64_load {
    my ($self, $a) = @_;
    $self->check($a, 8);
    return unpack('Q<', substr($self->{data}, $a, 8));
}
