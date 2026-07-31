# requires: rt/link_error
# A present-but-wrong-kind import is a link error, distinct from a missing
# one (which still falls through to the caller's fallback via `//`).
# Function values are plain coderefs; the runtime's own Global/Table/Memory
# wrappers self-report via their wasm_kind field.
sub check_import_kind {
    my ($value, $kind, $mod, $name) = @_;
    return undef unless defined $value;
    my $ok;
    if ($kind eq 'func') {
        $ok = ref($value) eq 'CODE';
    }
    else {
        my $r = ref($value);
        $ok = $r && $r ne 'HASH' && $r ne 'ARRAY' && $r ne 'CODE'
            && ($value->{wasm_kind} // '') eq $kind;
    }
    return $value if $ok;
    Rt::link_error("incompatible import type for $mod.$name");
}
