# requires: rt/trap
sub call {
    my ($self, $i, $type_key, @args) = @_;
    Rt::trap('undefined element') if $i >= scalar @{$self->{slots}};
    my $slot = $self->{slots}[$i];
    Rt::trap('uninitialized element') unless defined $slot;
    Rt::trap('indirect call type mismatch') unless $slot->[0] eq $type_key;
    return $slot->[1]->(@args);
}
