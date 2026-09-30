# Python backend

`--target python`.
A plain importable module with the full WASI Preview 1 surface.

## Output shape

A single `.py` module holds the generated module as a class.
The runtime sits at **module top level** under the name `<Class>Rt`, so `Add` gets `AddRt`.
Python method scopes cannot see an outer class scope.
So the runtime cannot nest inside the class as it does for Ruby.
Naming it after the class is what lets two artifacts share one namespace.
Only wasm loops become real `while True`.
Forward branches use a per-function branch register `_br`.
That keeps them within Python's static-nesting limits.

In **library** mode, `--module-name` is required and is taken unchanged as the class name.
It is one identifier matching `[A-Za-z_][A-Za-z0-9_]*`.
Anything else is a conversion-time error, nothing is rewritten.
In **standalone** mode the class is always `Program` and `--module-name` is rejected.

## Requirements

`python3` **3.9 or newer** on `PATH`.
No third-party packages: the output uses only the standard library.

> [!WARNING]
> Avoid CPython 3.12.0 through 3.12.3.
> A static-block limit in those releases breaks large generated modules with deeply nested loops.
> See issue #21.
> 3.12.4 and newer are unaffected.

## Running it

```console
$ dewasm prog.wasm --target python --mode standalone -o prog.py
$ python3 prog.py --dir ./data::/data arg1 arg2
```

Standalone programs share one runtime interface: [`docs/standalone-interface.md`](../standalone-interface.md).
It covers `argv`, `--dir` preopens, environment, and exit/trap.

Library mode:

```python
from add import Add
inst = Add()
print(inst.invoke("add", 2, 3))   # 5
inst.memory                       # linear memory
```

`proc_exit` raises `<Class>Rt.Exit` (with `.code`), spelled `AddRt.Exit` here.
In library mode, catch it around `invoke("_start")`.

## Capabilities

Full wasm core 1.0 plus the universal baseline.
**Full WASI Preview 1 including the file system**, adopting the Ruby file system model.
Non-function imports, multiple tables, and table bulk operations are supported.
The final exception-handling proposal is supported:

- A thrown wasm exception is a native exception carrying its tag.
- `catch_all` cannot observe traps.
- C programs based on `setjmp`/`longjmp` convert and run; `mruby` is the covered app case.

The official support table: [`docs/support.md`](../support.md).

## Providers and library usage

Any unprovided WASI import falls back to a bundled WASI (`--no-default-wasi` turns it off).
Override an import by passing an imports table to the constructor.
Unprovided entries still fall back:

```python
_captured = bytearray()
_holder = {}

def _fd_write(fd, iovs, iovs_len, out_ptr):
    mem = _holder["inst"].memory
    ptr = mem.iwl(iovs)       # iwl = i32 load, iws = i32 store
    length = mem.iwl(iovs + 4)
    _captured.extend(mem.read_string(ptr, length))
    mem.iws(out_ptr, length)
    return 0

inst = Prog({"wasi_snapshot_preview1": {"fd_write": _fd_write}})
_holder["inst"] = inst
inst.invoke("_start")   # random_get falls back to the bundled WASI
```

Preopen host directories for file system access via the constructor's `preopens` argument.
The e2e glue in `crates/dewasm-backend-python/tests/e2e.rs` is the worked reference.

## Limits

- **Recursion / thread-stack depth.**
  Deeply recursive wasm (or deep call chains) can hit Python's recursion limit.
  Heavy programs may need to raise it (`sys.setrecursionlimit`) and the thread stack size.
- Float division goes through the runtime's `fdiv` because Python raises on `x / 0.0`.
- Numeric conventions are the shared masked-unsigned model.
