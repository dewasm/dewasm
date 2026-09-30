# requires: rt/trap
# Linear memory is one Perl byte string changed in place.
# Its accesses are these, measured fastest:
# - 4-argument `substr` for multi-byte stores;
# - `vec` for single bytes;
# - `unpack` of a `substr` for loads.
# `vec` is big-endian only beyond 8 bits.
sub new {
    my ($class, $min_pages, $max_pages) = @_;
    $max_pages = 65536 if !defined($max_pages) || $max_pages > 65536;
    return bless({
        wasm_kind => 'memory',
        data      => "\0" x ($min_pages * 65536),
        max_pages => $max_pages,
    }, $class);
}

sub check {
    my ($self, $a, $n) = @_;
    Rt::trap('out of bounds memory access') if $a + $n > length($self->{data});
}
