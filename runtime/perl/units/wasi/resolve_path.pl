# requires: wasi/errno_fs
use Cwd ();
use Errno ();
use File::Basename ();

sub within {
    my ($self, $base, $path) = @_;
    my $prefix = substr($base, -1) eq '/' ? $base : "$base/";
    return $path eq $base || rindex($path, $prefix, 0) == 0;
}

# Lexical escape guard: expand "." and ".." purely textually and report
# whether the path climbs out of the base. Cwd::realpath below is the
# symlink-aware check, but it returns undef when the escaped-to parent
# does not exist on disk, so a path like "a/../../etc" needs catching here
# to report NOTCAPABLE rather than NOENT (mirrors the Ruby runtime's
# File.expand_path guard).
sub lexical_escape {
    my ($self, $rel) = @_;
    my @stack;
    for my $c (split m{/+}, $rel) {
        next if $c eq '' || $c eq '.';
        if ($c eq '..') {
            return 1 unless @stack;
            pop @stack;
        } else {
            push @stack, $c;
        }
    }
    return 0;
}

# Resolves a guest-relative path against a directory fd to an absolute
# host path, confined to that directory fd's own (already-realpath'd)
# root. Every call re-validates against its own dirfd's root, so nested
# path_opens can't be used to launder an escape one level cheaper.
#
# A non-directory base fd is NOTDIR (a file used as a dirfd); an absent
# one is BADF. A leading "/" is NOTCAPABLE before any join (an absolute
# guest path escapes the preopen). A trailing slash is preserved on the
# returned host path so the underlying host call enforces the POSIX "must
# be a directory" rule (ADR-40).
#
# $follow_last false resolves the parent but leaves the final component
# untouched (the AT_SYMLINK_NOFOLLOW shape), for syscalls that operate on
# a symlink itself (lstat, unlink, rename, rmdir, mkdir, symlink, link).
# A trailing "." or ".." is never a symlink, so those fall back to full
# resolution.
#
# Known limitation (ADR-14): this is a check-then-open, not an atomic
# openat(2)-beneath resolution — a TOCTOU race or a symlink planted inside
# the sandbox between the check and the actual filesystem call could in
# principle escape. Accepted for a single-process research/demo runtime,
# not a multi-tenant sandbox host.
sub resolve_path {
    my ($self, $dirfd, $rel, $follow_last) = @_;
    $follow_last = 1 unless defined $follow_last;
    my $entry = $self->{fds}{$dirfd};
    return (undef, ERRNO_BADF) unless defined $entry;
    return (undef, ERRNO_NOTDIR) unless $entry->{dir};
    return (undef, ERRNO_INVAL) if index($rel, "\0") >= 0;
    return (undef, ERRNO_NOTCAPABLE) if rindex($rel, '/', 0) == 0;
    my $base = $entry->{path};
    # Containment is checked before existence: a path whose "..s" escape
    # the sandbox is NOTCAPABLE even when the escaped-to parent does not
    # exist.
    return (undef, ERRNO_NOTCAPABLE) if $self->lexical_escape($rel);
    my $trailing = length($rel) > 1 && substr($rel, -1) eq '/';
    my $suffix = $trailing ? '/' : '';
    (my $core = $rel) =~ s{/+\z}{};
    my $joined = $core eq '' ? $base : "$base/$core";
    my $last = File::Basename::basename($joined);
    if (!$follow_last && $last ne '.' && $last ne '..') {
        my $real_parent = Cwd::realpath(File::Basename::dirname($joined));
        if (!defined $real_parent) {
            my $e = 0 + $!;
            return (undef, ERRNO_LOOP) if $e == Errno::ELOOP();
            return (undef, ERRNO_NOENT) if $e == Errno::ENOENT();
            return (undef, ERRNO_IO);
        }
        return (undef, ERRNO_NOTCAPABLE) unless $self->within($base, $real_parent);
        return ("$real_parent/$last$suffix", undef);
    }
    my $real = Cwd::realpath($joined);
    if (defined $real) {
        return (undef, ERRNO_NOTCAPABLE) unless $self->within($base, $real);
        return ($real . $suffix, undef);
    }
    my $e = 0 + $!;
    return (undef, ERRNO_LOOP) if $e == Errno::ELOOP();
    return (undef, ERRNO_IO) if $e != Errno::ENOENT();
    # The final component is missing (or a dangling symlink): resolve the
    # parent and re-attach it, so a create (path_open O_CREAT) still gets
    # a sandboxed target path.
    my $real_parent = Cwd::realpath(File::Basename::dirname($joined));
    if (!defined $real_parent) {
        return (undef, 0 + $! == Errno::ENOENT() ? ERRNO_NOENT : ERRNO_IO);
    }
    return (undef, ERRNO_NOTCAPABLE) unless $self->within($base, $real_parent);
    return ("$real_parent/" . File::Basename::basename($joined) . $suffix, undef);
}
