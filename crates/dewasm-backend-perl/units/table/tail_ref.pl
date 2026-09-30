# requires: rt/trap
# `call`'s checks, raised at the same point in execution order.
# It returns the slot's code reference for the trampoline instead of calling it.
# A slot's optional third element is a code reference for the body of a tail-calling function.
# A slot without one completes in a single frame anyway.
sub tail_ref {
    my ($self, $i, $type_key) = @_;
    Rt::trap('undefined element') if $i >= scalar @{$self->{slots}};
    my $slot = $self->{slots}[$i];
    Rt::trap('uninitialized element') unless defined $slot;
    Rt::trap('indirect call type mismatch') unless $slot->[0] eq $type_key;
    return $slot->[2] // $slot->[1];
}
