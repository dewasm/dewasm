ERRNO_SUCCESS = 0
ERRNO_BADF = 8
ERRNO_INVAL = 28
ERRNO_IO = 29
ERRNO_NOSYS = 52
ERRNO_SPIPE = 70
ERRNO_NOTCAPABLE = 76

# WASI rights bits (fs_rights_base / fs_rights_inheriting). Per-fd capabilities
# are modelled after wasmtime's wasi-common: a directory and a file each carry a
# different default set, path_open narrows the requested rights against the
# parent's inheriting set (then per-filetype), and fd_fdstat_set_rights can only
# drop bits. Kept in the always-bundled prelude because __init__ seeds
# the parallel fd -> [base, inheriting, fdflags] meta map for every preopen and
# for stdio, so the constants must exist whenever any WASI import is used.
RIGHTS_FD_DATASYNC = 1 << 0
RIGHTS_FD_READ = 1 << 1
RIGHTS_FD_SEEK = 1 << 2
RIGHTS_FD_FDSTAT_SET_FLAGS = 1 << 3
RIGHTS_FD_SYNC = 1 << 4
RIGHTS_FD_TELL = 1 << 5
RIGHTS_FD_WRITE = 1 << 6
RIGHTS_FD_ADVISE = 1 << 7
RIGHTS_FD_ALLOCATE = 1 << 8
RIGHTS_PATH_CREATE_DIRECTORY = 1 << 9
RIGHTS_PATH_CREATE_FILE = 1 << 10
RIGHTS_PATH_LINK_SOURCE = 1 << 11
RIGHTS_PATH_LINK_TARGET = 1 << 12
RIGHTS_PATH_OPEN = 1 << 13
RIGHTS_FD_READDIR = 1 << 14
RIGHTS_PATH_READLINK = 1 << 15
RIGHTS_PATH_RENAME_SOURCE = 1 << 16
RIGHTS_PATH_RENAME_TARGET = 1 << 17
RIGHTS_PATH_FILESTAT_GET = 1 << 18
RIGHTS_PATH_FILESTAT_SET_SIZE = 1 << 19
RIGHTS_PATH_FILESTAT_SET_TIMES = 1 << 20
RIGHTS_FD_FILESTAT_GET = 1 << 21
RIGHTS_FD_FILESTAT_SET_SIZE = 1 << 22
RIGHTS_FD_FILESTAT_SET_TIMES = 1 << 23
RIGHTS_PATH_SYMLINK = 1 << 24
RIGHTS_PATH_REMOVE_DIRECTORY = 1 << 25
RIGHTS_PATH_UNLINK_FILE = 1 << 26
RIGHTS_POLL_FD_READWRITE = 1 << 27

# The rights a directory descriptor carries (base) and the rights it may pass to
# things opened beneath it (inheriting = directory rights plus every file
# right). Mirrors wasmtime's DIR_RIGHTS / FILE_RIGHTS.
DIR_RIGHTS_BASE = (
    RIGHTS_FD_FDSTAT_SET_FLAGS | RIGHTS_FD_SYNC | RIGHTS_FD_ADVISE
    | RIGHTS_PATH_CREATE_DIRECTORY | RIGHTS_PATH_CREATE_FILE
    | RIGHTS_PATH_LINK_SOURCE | RIGHTS_PATH_LINK_TARGET | RIGHTS_PATH_OPEN
    | RIGHTS_FD_READDIR | RIGHTS_PATH_READLINK | RIGHTS_PATH_RENAME_SOURCE
    | RIGHTS_PATH_RENAME_TARGET | RIGHTS_PATH_FILESTAT_GET
    | RIGHTS_PATH_FILESTAT_SET_SIZE | RIGHTS_PATH_FILESTAT_SET_TIMES
    | RIGHTS_FD_FILESTAT_GET | RIGHTS_FD_FILESTAT_SET_TIMES | RIGHTS_PATH_SYMLINK
    | RIGHTS_PATH_REMOVE_DIRECTORY | RIGHTS_PATH_UNLINK_FILE
    | RIGHTS_POLL_FD_READWRITE)
FILE_RIGHTS_BASE = (
    RIGHTS_FD_DATASYNC | RIGHTS_FD_READ | RIGHTS_FD_SEEK
    | RIGHTS_FD_FDSTAT_SET_FLAGS | RIGHTS_FD_SYNC | RIGHTS_FD_TELL
    | RIGHTS_FD_WRITE | RIGHTS_FD_ADVISE | RIGHTS_FD_ALLOCATE
    | RIGHTS_FD_FILESTAT_GET | RIGHTS_FD_FILESTAT_SET_SIZE
    | RIGHTS_FD_FILESTAT_SET_TIMES | RIGHTS_POLL_FD_READWRITE)
DIR_RIGHTS_INHERITING = DIR_RIGHTS_BASE | FILE_RIGHTS_BASE

# A directory descriptor: either a preopen (`preopen_name`
# set to the guest-visible path passed in `preopens`) or a directory the guest
# opened itself via path_open (`preopen_name` None). `entries` is the
# fd_readdir listing cache, populated lazily. Nested in the WASI class (so
# methods reach it as `self.WasiDir`) and kept in the prelude because
# `__init__` builds one per preopen unconditionally, so it must be available
# whenever any WASI import is used, not only when a filesystem syscall is.
class WasiDir:
    def __init__(self, host_path, preopen_name, entries):
        self.host_path = host_path
        self.preopen_name = preopen_name
        self.entries = entries

def __init__(self, args=None, env=None, preopens=None):
    self.args = [a if isinstance(a, bytes) else str(a).encode("utf-8") for a in (args or [])]
    self.env = [("%s=%s" % (k, v)).encode("utf-8") for k, v in (env or {}).items()]
    self.fds = {0: sys.stdin.buffer, 1: sys.stdout.buffer, 2: sys.stderr.buffer}
    # Parallel per-fd capability map: fd -> [base, inheriting, fdflags]. stdio
    # gets the full file-right set (a stream can read/write/etc.); preopens get
    # the directory base and the directory-plus-file inheriting set.
    self.fd_meta = {
        0: [self.FILE_RIGHTS_BASE, 0, 0],
        1: [self.FILE_RIGHTS_BASE, 0, 0],
        2: [self.FILE_RIGHTS_BASE, 0, 0],
    }
    # The stdio special-cases (SPIPE on seek/tell/pread/pwrite, no close) key
    # on the objects captured here, in lockstep with the fd table.
    self.std_ios = (sys.stdin.buffer, sys.stdout.buffer, sys.stderr.buffer)
    self.memory = None
    next_fd = 3
    for guest, host in (preopens or {}).items():
        # The host path must resolve, but need not be a directory: like the
        # Ruby/Perl runtimes, a single-file preopen (e.g. '/dev/null' for the
        # zeroperl reactor's init probe) is accepted — the guest resolves it
        # as the preopen root itself.
        real = os.path.realpath(host)
        if not os.path.exists(real):
            raise ValueError("preopen %r => %r: does not exist" % (guest, host))
        name = guest if isinstance(guest, bytes) else str(guest).encode("utf-8")
        self.fds[next_fd] = self.WasiDir(real, name, None)
        self.fd_meta[next_fd] = [self.DIR_RIGHTS_BASE, self.DIR_RIGHTS_INHERITING, 0]
        next_fd += 1
    self.next_fd = next_fd

# Import-provider object: a custom WASI runtime can replace this
# class wholesale by implementing wasm_import(name) and attach(instance).
def wasm_import(self, name):
    return getattr(self, "wasi_" + name, None)

def attach(self, instance):
    self.memory = instance.memory
