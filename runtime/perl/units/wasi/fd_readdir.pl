# requires: memory/init, memory/i32_store, wasi/wasi_filetype
sub wasi_fd_readdir {
    my ($self, $fd, $buf_ptr, $buf_len, $cookie, $bufused_ptr) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF unless defined($e) && $e->{dir};
    return ERRNO_NOTCAPABLE unless $self->{meta}{$fd}[0] & RIGHTS_FD_READDIR;
    # cookie 0 starts a fresh enumeration, so re-scan the directory then; a
    # non-zero cookie resumes the snapshot cached from that start (the
    # opaque-resume-point contract).
    if (!defined($e->{entries}) || $cookie == 0) {
        $e->{entries} = $self->readdir_entries($e->{path});
        return ERRNO_IO unless defined $e->{entries};
    }
    my $entries = $e->{entries};
    my $out = '';
    my $i = $cookie;
    while ($i < @$entries && length($out) < $buf_len) {
        my ($name, $filetype, $ino) = @{$entries->[$i]};
        # dirent: d_next (u64, resume cookie) + d_ino (u64) + d_namlen
        # (u32) + d_type (u8) + 3 pad, followed by the (unpadded) name.
        $out .= pack('Q<Q<VCxxx', $i + 1, $ino, length($name), $filetype) . $name;
        $i++;
    }
    # A dirent may be legally truncated at the tail once buf_len runs out.
    $out = substr($out, 0, $buf_len) if length($out) > $buf_len;
    $self->{memory}->init($buf_ptr, $out, 0, length($out));
    $self->{memory}->i32_store($bufused_ptr, length($out));
    return ERRNO_SUCCESS;
}

sub readdir_entries {
    my ($self, $path) = @_;
    opendir(my $dh, $path) or return undef;
    my @names = sort grep { $_ ne '.' && $_ ne '..' } readdir($dh);
    closedir($dh);
    my @self_st = lstat($path);
    my @up_st = lstat("$path/..");
    my @entries = (['.', 3, $self_st[1] // 0], ['..', 3, $up_st[1] // 0]);
    for my $name (@names) {
        my @st = lstat("$path/$name") or next;
        push @entries, [$name, $self->wasi_filetype($st[2]), $st[1]];
    }
    return \@entries;
}
