# A pending tail call: a tail-calling function's body returns one of these instead of calling, and its entry wrapper unwraps them in a loop, so a chain of any length runs in constant stack space.
# An arrayref rather than `goto &sub`, perl's real tail call, because the body runs inside an eval block and perl refuses to goto out of one.
sub tail_call {
    my ($target, @args) = @_;
    return [$target, \@args];
}

# A body returns its results as a list, so a thunk is recognized by being the whole of a one-element list holding an arrayref: a wasm value is a number or a coderef, never an arrayref, so nothing else can look like one.
sub trampoline {
    my @r = @_;
    while (@r == 1 && ref($r[0]) eq 'ARRAY') {
        @r = $r[0][0]->(@{$r[0][1]});
    }
    return wantarray ? @r : $r[0];
}
