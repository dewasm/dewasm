# requires: rt/trap, table/check_range
# `$elem` is an arrayref of table slot values ([type_key, coderef] pairs, or undef for a ref.null item), built once at instantiation/table.init time.
sub init {
    my ($self, $dst, $elem, $src, $len) = @_;
    Rt::trap('out of bounds table access') if $src + $len > scalar @$elem;
    $self->check_range($dst, $len);
    return if $len == 0;
    @{$self->{slots}}[$dst .. $dst + $len - 1] = @{$elem}[$src .. $src + $len - 1];
}
