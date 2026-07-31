# Map a stat mode word to a WASI filetype tag (S_IFMT bits, avoiding a
# POSIX-constant dependency).
sub wasi_filetype {
    my ($self, $mode) = @_;
    my $fmt = $mode & 0170000;
    return 3 if $fmt == 0040000;  # directory
    return 2 if $fmt == 0020000;  # character device
    return 1 if $fmt == 0060000;  # block device
    return 7 if $fmt == 0120000;  # symbolic link
    return 6 if $fmt == 0140000;  # socket (stream)
    return 4 if $fmt == 0100000;  # regular file
    return 0;  # unknown
}
