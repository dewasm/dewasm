# pack/unpack 'd' is a byte copy, so NaN payloads survive (ADR-55).
sub f64_load {
    my ($self, $a) = @_;
    $self->check($a, 8);
    return unpack('d<', substr($self->{data}, $a, 8));
}
