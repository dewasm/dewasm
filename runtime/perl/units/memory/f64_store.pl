sub f64_store {
    my ($self, $a, $v) = @_;
    $self->check($a, 8);
    substr($self->{data}, $a, 8, pack('d<', $v));
}
