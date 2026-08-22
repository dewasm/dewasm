# requires: rt/trap, table/check_range, table/slice
sub copy {
    my ($self, $dst, $other, $src, $len) = @_;
    Rt::trap('out of bounds table access') if $src + $len > $other->size();
    $self->check_range($dst, $len);
    return if $len == 0;
    my @tmp = $other->slice($src, $len);
    @{$self->{slots}}[$dst .. $dst + $len - 1] = @tmp;
}
