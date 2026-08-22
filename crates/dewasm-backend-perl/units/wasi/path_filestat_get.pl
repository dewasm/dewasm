# requires: memory/read_string, memory/init, wasi/resolve_path, wasi/pack_filestat, wasi/errno_fs
use Time::HiRes ();

sub wasi_path_filestat_get {
    my ($self, $dirfd, $flags, $path_ptr, $path_len, $buf_ptr) = @_;
    my $rel = $self->{memory}->read_string($path_ptr, $path_len);
    my $follow = ($flags & 0x1) != 0;  # lookupflags::SYMLINK_FOLLOW
    my ($host, $err) = $self->resolve_path($dirfd, $rel, $follow);
    return $err if defined $err;
    my @st = $follow ? Time::HiRes::stat($host) : Time::HiRes::lstat($host);
    return $self->fs_errno(0 + $!) unless @st;
    $self->{memory}->init($buf_ptr, $self->pack_filestat(\@st), 0, 64);
    return ERRNO_SUCCESS;
}
