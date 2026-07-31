# One slot per element: a [type_key, coderef] pair for funcref tables, or
# undef for a null slot (ADR-16). call_indirect compares type keys, not
# module-local indices, so a shared table stays consistent across modules.
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
