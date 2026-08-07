# requires: memory/read_string, memory/i32_store, wasi/resolve_path, wasi/errno_fs
use Cwd ();
use Fcntl ();

sub wasi_path_open {
    my ($self, $dirfd, $dirflags, $path_ptr, $path_len, $oflags, $fs_rights_base, $fs_rights_inheriting, $fdflags, $opened_fd_ptr) = @_;
    my $rel = $self->{memory}->read_string($path_ptr, $path_len);
    my $follow = ($dirflags & 0x1) != 0;  # lookupflags::SYMLINK_FOLLOW
    my ($host, $err) = $self->resolve_path($dirfd, $rel, $follow);
    return $err if defined $err;
    # The dirfd must itself carry PATH_OPEN; a rights-narrowed dir
    # fd that dropped it can no longer open beneath itself.
    return ERRNO_NOTCAPABLE unless $self->{meta}{$dirfd}[0] & RIGHTS_PATH_OPEN;
    # OFLAGS_TRUNC needs the PATH_FILESTAT_SET_SIZE right on the dirfd.
    if (($oflags & 0x8) && !($self->{meta}{$dirfd}[0] & RIGHTS_PATH_FILESTAT_SET_SIZE)) {
        return ERRNO_NOTCAPABLE;
    }
    my $read = ($fs_rights_base & RIGHTS_FD_READ) != 0;
    my $write = ($fs_rights_base & RIGHTS_FD_WRITE) != 0;
    my $flags = $read && $write ? Fcntl::O_RDWR() : ($write ? Fcntl::O_WRONLY() : Fcntl::O_RDONLY());
    # Without SYMLINK_FOLLOW a final symlink must not be traversed:
    # O_NOFOLLOW turns that into ELOOP, matching wasmtime's O_NOFOLLOW
    # default.
    $flags |= Fcntl::O_NOFOLLOW() unless $follow;
    $flags |= Fcntl::O_DIRECTORY() if $oflags & 0x2;  # oflags::DIRECTORY
    $flags |= Fcntl::O_CREAT() if $oflags & 0x1;  # oflags::CREAT
    $flags |= Fcntl::O_EXCL() if $oflags & 0x4;  # oflags::EXCL
    $flags |= Fcntl::O_TRUNC() if $oflags & 0x8;  # oflags::TRUNC
    # O_CREAT must not create through a trailing slash (issue #42); per
    # wasmtime: EINVAL on macOS, EISDIR on Linux, plain open
    # ENOENT.
    if (substr($host, -1) eq '/' && !lstat(substr($host, 0, -1))) {
        return ERRNO_NOENT unless $oflags & 0x1;  # no oflags::CREAT
        return $^O eq 'darwin' ? ERRNO_INVAL : ERRNO_ISDIR;
    }
    sysopen(my $fh, $host, $flags, 0644) or return $self->fs_errno(0 + $!);
    my @st = stat($fh);
    if (!@st) {
        my $e = 0 + $!;
        close($fh);
        return $self->fs_errno($e);
    }
    my ($base, $inheriting);
    if (($st[2] & 0170000) == 0040000) {  # a directory
        # Opening a directory: keep a dir entry (sysread on it is not
        # meaningful), narrow to the directory rights, and drop the
        # throwaway OS handle.
        close($fh);
        my $real = Cwd::realpath($host);
        return $self->fs_errno(0 + $!) unless defined $real;
        $self->{fds}{$self->{next_fd}} = { dir => 1, path => $real, preopen => undef, entries => undef };
        $base = $fs_rights_base & $self->{meta}{$dirfd}[1] & DIR_RIGHTS_BASE;
        $inheriting = $fs_rights_inheriting & $self->{meta}{$dirfd}[1] & DIR_RIGHTS_INHERITING;
    } else {
        # Unbuffered by construction: every access goes through sysread/
        # syswrite/sysseek, so pread/pwrite emulation stays coherent with
        # read/write/seek — sqlite mixes both on one fd.
        binmode($fh);
        $self->{fds}{$self->{next_fd}} = { fh => $fh, path => $host };
        $base = $fs_rights_base & $self->{meta}{$dirfd}[1] & FILE_RIGHTS_BASE;
        $inheriting = $fs_rights_inheriting & $self->{meta}{$dirfd}[1] & FILE_RIGHTS_BASE;
    }
    $self->{meta}{$self->{next_fd}} = [$base, $inheriting, $fdflags & 0xFFFF];
    $self->{memory}->i32_store($opened_fd_ptr, $self->{next_fd});
    $self->{next_fd}++;
    return ERRNO_SUCCESS;
}
