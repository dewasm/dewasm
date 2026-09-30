# requires: wasi/wasi_filetype
# Packs a `Time::HiRes::stat` result (an array reference) into a WASI `filestat` (64 bytes).
# Its fields are `dev`, `ino`, `filetype` (+7 bytes of padding), `nlink`, and `size`.
# Then come `atim`, `mtim` and `ctim`, and every field is u64.
# Times are in nanoseconds, rounded to what NV seconds can carry.
# That is a resolution of roughly 400 ns at the current epoch.
sub pack_filestat {
    my ($self, $st) = @_;
    return pack('Q<Q<Cx7Q<Q<Q<Q<Q<',
        $st->[0], $st->[1], $self->wasi_filetype($st->[2]), $st->[3], $st->[7],
        map { int($_ * 1e9 + 0.5) } @{$st}[8, 9, 10]);
}
