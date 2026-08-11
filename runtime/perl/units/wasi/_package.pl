# WASI preview 1 runtime state (mirroring the Ruby/Python runtimes): a fd table plus a parallel per-fd capability map, seeded from the constructor's preopens.
# Per-fd rights are modelled after wasmtime's wasi-common: a directory and a file each carry a different default set, path_open narrows the requested rights against the parent's inheriting set (then per-filetype), and fd_fdstat_set_rights can only drop bits.
# Kept in the always-bundled prelude because new() seeds the fd -> [base, inheriting, fdflags] meta map for every preopen and for stdio, so the constants must exist whenever any WASI import is used.
use Cwd ();

use constant {
    ERRNO_SUCCESS => 0,
    ERRNO_BADF => 8,
    ERRNO_INVAL => 28,
    ERRNO_IO => 29,
    ERRNO_NOSYS => 52,
    ERRNO_SPIPE => 70,
    ERRNO_NOTCAPABLE => 76,
};

use constant {
    RIGHTS_FD_DATASYNC => 1 << 0,
    RIGHTS_FD_READ => 1 << 1,
    RIGHTS_FD_SEEK => 1 << 2,
    RIGHTS_FD_FDSTAT_SET_FLAGS => 1 << 3,
    RIGHTS_FD_SYNC => 1 << 4,
    RIGHTS_FD_TELL => 1 << 5,
    RIGHTS_FD_WRITE => 1 << 6,
    RIGHTS_FD_ADVISE => 1 << 7,
    RIGHTS_FD_ALLOCATE => 1 << 8,
    RIGHTS_PATH_CREATE_DIRECTORY => 1 << 9,
    RIGHTS_PATH_CREATE_FILE => 1 << 10,
    RIGHTS_PATH_LINK_SOURCE => 1 << 11,
    RIGHTS_PATH_LINK_TARGET => 1 << 12,
    RIGHTS_PATH_OPEN => 1 << 13,
    RIGHTS_FD_READDIR => 1 << 14,
    RIGHTS_PATH_READLINK => 1 << 15,
    RIGHTS_PATH_RENAME_SOURCE => 1 << 16,
    RIGHTS_PATH_RENAME_TARGET => 1 << 17,
    RIGHTS_PATH_FILESTAT_GET => 1 << 18,
    RIGHTS_PATH_FILESTAT_SET_SIZE => 1 << 19,
    RIGHTS_PATH_FILESTAT_SET_TIMES => 1 << 20,
    RIGHTS_FD_FILESTAT_GET => 1 << 21,
    RIGHTS_FD_FILESTAT_SET_SIZE => 1 << 22,
    RIGHTS_FD_FILESTAT_SET_TIMES => 1 << 23,
    RIGHTS_PATH_SYMLINK => 1 << 24,
    RIGHTS_PATH_REMOVE_DIRECTORY => 1 << 25,
    RIGHTS_PATH_UNLINK_FILE => 1 << 26,
    RIGHTS_POLL_FD_READWRITE => 1 << 27,
};

# The rights a directory descriptor carries (base) and the rights it may pass to things opened beneath it (inheriting = directory rights plus every file right).
# Mirrors wasmtime's DIR_RIGHTS / FILE_RIGHTS.
use constant DIR_RIGHTS_BASE =>
    RIGHTS_FD_FDSTAT_SET_FLAGS | RIGHTS_FD_SYNC | RIGHTS_FD_ADVISE
    | RIGHTS_PATH_CREATE_DIRECTORY | RIGHTS_PATH_CREATE_FILE
    | RIGHTS_PATH_LINK_SOURCE | RIGHTS_PATH_LINK_TARGET | RIGHTS_PATH_OPEN
    | RIGHTS_FD_READDIR | RIGHTS_PATH_READLINK | RIGHTS_PATH_RENAME_SOURCE
    | RIGHTS_PATH_RENAME_TARGET | RIGHTS_PATH_FILESTAT_GET
    | RIGHTS_PATH_FILESTAT_SET_SIZE | RIGHTS_PATH_FILESTAT_SET_TIMES
    | RIGHTS_FD_FILESTAT_GET | RIGHTS_FD_FILESTAT_SET_TIMES
    | RIGHTS_PATH_SYMLINK | RIGHTS_PATH_REMOVE_DIRECTORY
    | RIGHTS_PATH_UNLINK_FILE | RIGHTS_POLL_FD_READWRITE;
use constant FILE_RIGHTS_BASE =>
    RIGHTS_FD_DATASYNC | RIGHTS_FD_READ | RIGHTS_FD_SEEK
    | RIGHTS_FD_FDSTAT_SET_FLAGS | RIGHTS_FD_SYNC | RIGHTS_FD_TELL
    | RIGHTS_FD_WRITE | RIGHTS_FD_ADVISE | RIGHTS_FD_ALLOCATE
    | RIGHTS_FD_FILESTAT_GET | RIGHTS_FD_FILESTAT_SET_SIZE
    | RIGHTS_FD_FILESTAT_SET_TIMES | RIGHTS_POLL_FD_READWRITE;
use constant DIR_RIGHTS_INHERITING => DIR_RIGHTS_BASE | FILE_RIGHTS_BASE;

# An fd-table entry is a plain hashref of one of three shapes:
# * stdio:  { fh => glob ref, std => 0|1|2 } (SPIPE on seek/tell/pread/
# pwrite, never closed; keyed by the `std` field, in lockstep with the
# fd table, not by whatever the globals point at when a syscall runs);
# * file:   { fh => handle, path => host path }, sysopen'd, unbuffered
# (sysread/syswrite/sysseek only), so pread/pwrite emulation and
# read/write/seek stay coherent on one fd (sqlite mixes both);
# * dir:    { dir => 1, path => realpath'd host path, preopen => guest
# name (undef when the guest opened it itself via path_open),
# entries => lazily built fd_readdir cache }.
sub new {
    my ($class, %opts) = @_;
    my $env = $opts{env} // {};
    my $self = bless({
        args => [map { "$_" } @{$opts{args} // []}],
        env => [map { "$_=$env->{$_}" } sort keys %$env],
        memory => undef,
    }, $class);
    binmode(STDIN);
    binmode(STDOUT);
    binmode(STDERR);
    $self->{fds} = {
        0 => { fh => \*STDIN, std => 0 },
        1 => { fh => \*STDOUT, std => 1 },
        2 => { fh => \*STDERR, std => 2 },
    };
    # stdio gets the full file-right set (a stream can read/write/etc.);
    # preopens get the directory base and the directory-plus-file inheriting set.
    $self->{meta} = {
        0 => [FILE_RIGHTS_BASE, 0, 0],
        1 => [FILE_RIGHTS_BASE, 0, 0],
        2 => [FILE_RIGHTS_BASE, 0, 0],
    };
    my $next_fd = 3;
    my $preopens = $opts{preopens} // {};
    for my $guest (sort keys %$preopens) {
        # The host path must resolve, but need not be a directory: like the
        # Ruby runtime, a single-file preopen (e.g. '/dev/null' for the zeroperl reactor's init probe) is accepted: the guest resolves it as the preopen root itself.
        my $real = Cwd::realpath($preopens->{$guest});
        die "preopen '$guest' => '$preopens->{$guest}': does not exist\n"
            unless defined $real;
        $self->{fds}{$next_fd} = { dir => 1, path => $real, preopen => "$guest", entries => undef };
        $self->{meta}{$next_fd} = [DIR_RIGHTS_BASE, DIR_RIGHTS_INHERITING, 0];
        $next_fd++;
    }
    $self->{next_fd} = $next_fd;
    return $self;
}

# Import-provider protocol: a custom WASI runtime can replace this package wholesale by implementing wasm_import($name) and attach($instance).
sub wasm_import {
    my ($self, $name) = @_;
    my $method = "wasi_$name";
    return undef unless $self->can($method);
    return sub { return $self->$method(@_); };
}

sub attach {
    my ($self, $instance) = @_;
    $self->{memory} = $instance->{memory};
}
