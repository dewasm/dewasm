# Python backend

`--target python`. A plain importable module with the full WASI preview 1 surface.

## Output shape

A single `.py` module: the generated module as a class named after the input stem (override with `--module-name`), with the runtime at **module top level** under the name `<Class>Rt` — `Add` gets `AddRt` (Python method scopes cannot see an enclosing class scope, so the runtime cannot nest inside the class as it does for Ruby; naming it after the class is what lets two artifacts share one namespace, [ADR-62](../adr/62-embedded-runtime-isolation.md)). Only wasm loops become real `while True`; forward branches use a per-function branch register `_br` to stay within Python's static-nesting limits. See [ADR-28](../adr/28-python-backend-lowering.md).

## Requirements

`python3` **3.9 or newer** on `PATH` (or `$DEWASM_PYTHON`). No third-party packages — the output uses only the standard library.

## Running it

```console
$ dewasm prog.wasm --target python --mode standalone -o prog.py
$ python3 prog.py --dir ./data::/data arg1 arg2
```

Standalone programs follow the shared runtime interface (argv, `--dir` preopens, env, exit/trap): [docs/standalone-interface.md](../standalone-interface.md).

Library mode:

```python
from add import Add
inst = Add()
print(inst.invoke("add", 2, 3))   # 5
inst.memory                       # linear memory
```

`proc_exit` raises `<Class>Rt.Exit` (with `.code`) — `AddRt.Exit` here; catch it around `invoke("_start")` in library mode.

## Capabilities

Full wasm core 1.0 plus the universal baseline, and **full WASI preview 1 including the filesystem** (adopting the Ruby fs model, [ADR-14](../adr/14-ruby-wasi-filesystem.md)). Non-function imports, multiple tables, and table bulk ops are supported. Authoritative matrix: [docs/support.md](../support.md).

## Providers and library usage

Any unprovided WASI import falls back to a bundled WASI (disable with `--no-default-wasi`). Override an import by passing an imports table to the constructor; unprovided entries still fall back:

```python
_captured = bytearray()
_holder = {}

def _fd_write(fd, iovs, iovs_len, out_ptr):
    mem = _holder["inst"].memory
    ptr = mem.i32_load(iovs)
    length = mem.i32_load(iovs + 4)
    _captured.extend(mem.read_string(ptr, length))
    mem.i32_store(out_ptr, length)
    return 0

inst = Prog({"wasi_snapshot_preview1": {"fd_write": _fd_write}})
_holder["inst"] = inst
inst.invoke("_start")   # random_get falls back to the bundled WASI
```

Preopen host directories for filesystem access via the constructor's `preopens` argument. The e2e glue in `crates/dewasm-backend-python/tests/e2e.rs` is the worked reference.

## Caveats

- **Recursion / thread-stack depth.** Deeply recursive wasm (or deep call chains) can hit Python's recursion limit; raising it (`sys.setrecursionlimit`) and the thread stack size may be necessary for heavy programs.
- Float division goes through the runtime's `fdiv` because Python raises on `x / 0.0` ([ADR-28](../adr/28-python-backend-lowering.md)).
- Numeric conventions are the shared masked-unsigned model ([ADR-2](../adr/2-numeric-semantics.md)).
