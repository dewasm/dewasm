# requires: memory/init
# `/dev/urandom` is the source of random bytes that core Perl can read.
# The `rand()` tail is a fallback for hosts without it, and it is not cryptographic.
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
