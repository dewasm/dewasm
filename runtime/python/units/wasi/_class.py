ERRNO_SUCCESS = 0
ERRNO_BADF = 8
ERRNO_INVAL = 28
ERRNO_IO = 29
ERRNO_NOSYS = 52
ERRNO_SPIPE = 70

# A directory descriptor (ADR-14 / ADR-28): either a preopen (`preopen_name`
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
    # The stdio special-cases (SPIPE on seek/tell/pread/pwrite, no close) key
    # on the objects captured here, in lockstep with the fd table.
    self.std_ios = (sys.stdin.buffer, sys.stdout.buffer, sys.stderr.buffer)
    self.memory = None
    next_fd = 3
    for guest, host in (preopens or {}).items():
        real = os.path.realpath(host)
        if not os.path.isdir(real):
            raise ValueError("preopen %r => %r: not a directory" % (guest, host))
        name = guest if isinstance(guest, bytes) else str(guest).encode("utf-8")
        self.fds[next_fd] = self.WasiDir(real, name, None)
        next_fd += 1
    self.next_fd = next_fd

# Import-provider object (ADR-7): a custom WASI runtime can replace this
# class wholesale by implementing wasm_import(name) and attach(instance).
def wasm_import(self, name):
    return getattr(self, "wasi_" + name, None)

def attach(self, instance):
    self.memory = instance.memory
