# The trampoline a tail-calling function's entry runs.
# A tail call parks its target and arguments on the instance and returns; nothing is allocated per hop, which is what a chain of any length pays otherwise.
# The target is cleared before dispatching, so a callee that does not tail-call ends the chain, and the arguments are read as the call's own operands, before the callee can overwrite them.
# The parked argument list is one array reused across hops, emptied and refilled rather than rebuilt, so the hop costs no allocation.
sub trampoline {
    my ($self, @r) = @_;
    while (defined(my $f = $self->{__tf})) {
        $self->{__tf} = undef;
        @r = $f->(@{$self->{__ta}});
    }
    return wantarray ? @r : $r[0];
}
