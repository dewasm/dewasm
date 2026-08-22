# requires: memory/size
sub grow {
    my ($self, $delta) = @_;
    my $old = $self->size();
    return 0xFFFFFFFF if $old + $delta > $self->{max_pages};
    $self->{data} .= "\0" x ($delta * 65536);
    return $old;
}
