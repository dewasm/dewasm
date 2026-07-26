ERRNO_SUCCESS = 0
ERRNO_BADF = 8
ERRNO_INVAL = 28
ERRNO_IO = 29
ERRNO_NOSYS = 52

# Import-provider object (ADR-7): a custom WASI runtime can replace this
# class wholesale by implementing wasm_import(name) and attach(instance).
def __init__(self, args=None, env=None, preopens=None):
    self.args = [a if isinstance(a, bytes) else str(a).encode("utf-8") for a in (args or [])]
    self.env = [("%s=%s" % (k, v)).encode("utf-8") for k, v in (env or {}).items()]
    self.fds = {0: sys.stdin.buffer, 1: sys.stdout.buffer, 2: sys.stderr.buffer}
    self.memory = None

def wasm_import(self, name):
    return getattr(self, "wasi_" + name, None)

def attach(self, instance):
    self.memory = instance.memory
