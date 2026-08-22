# requires: rt/trap
sub check_range {
    my ($self, $offset, $count) = @_;
    Rt::trap('out of bounds table access') if $offset + $count > scalar @{$self->{slots}};
}
