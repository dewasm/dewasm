# requires: rt/trap, table/check_range
# `$elem` is an array reference of table slot values, built once at instantiation or `table.init`.
# A slot value is a `[type_key, coderef]` pair, or `undef` for a `ref.null` item.
sub init {
    my ($self, $dst, $elem, $src, $len) = @_;
    Rt::trap('out of bounds table access') if $src + $len > scalar @$elem;
    $self->check_range($dst, $len);
    return if $len == 0;
    @{$self->{slots}}[$dst .. $dst + $len - 1] = @{$elem}[$src .. $src + $len - 1];
}
