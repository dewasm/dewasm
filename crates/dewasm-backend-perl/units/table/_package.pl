# One slot per element.
# A slot holds a `[type_key, coderef]` pair for `funcref` tables, or `undef` when null.
# A tail-calling function's pair carries a code reference for its body as an optional third element.
# `table/tail_ref` reads that element.
# `call_indirect` compares type keys, not module-local indices.
# A shared table then stays consistent across modules.
sub new {
    my ($class, $size, $max) = @_;
    return bless({
        wasm_kind => 'table',
        slots     => [(undef) x $size],
        max       => $max,
    }, $class);
}

sub size {
    return scalar @{$_[0]->{slots}};
}
