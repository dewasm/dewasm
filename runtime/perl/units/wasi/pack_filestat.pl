# requires: wasi/wasi_filetype
# Packs a Time::HiRes::stat result (arrayref) into a WASI filestat (64
# bytes): dev, ino, filetype (+7 pad), nlink, size, atim/mtim/ctim (all
# u64, times in nanoseconds — quantized to what NV seconds can carry,
# roughly 400ns granularity at the current epoch).
sub pack_filestat {
    my ($self, $st) = @_;
    return pack('Q<Q<Cx7Q<Q<Q<Q<Q<',
        $st->[0], $st->[1], $self->wasi_filetype($st->[2]), $st->[3], $st->[7],
        map { int($_ * 1e9 + 0.5) } @{$st}[8, 9, 10]);
}
