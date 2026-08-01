# requires: memory/init
# /dev/urandom is the core-perl entropy source; the rand() tail is a
# non-cryptographic fallback for hosts without it.
sub wasi_random_get {
    my ($self, $buf_ptr, $len) = @_;
    my $bytes = '';
    if (open(my $fh, '<:raw', '/dev/urandom')) {
        while (length($bytes) < $len) {
            last unless sysread($fh, $bytes, $len - length($bytes), length($bytes));
        }
        close($fh);
    }
    $bytes .= chr(int(rand(256))) while length($bytes) < $len;
    $self->{memory}->init($buf_ptr, $bytes, 0, $len);
    return ERRNO_SUCCESS;
}
