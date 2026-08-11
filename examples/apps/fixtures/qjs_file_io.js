// Phase 5a QuickJS file-I/O fixture (our own committed code).
//
// Exercises QuickJS-ng's std/os modules against a WASI preopened scratch
// directory mounted at /work: write a file, read it back, print it. Run as
//   qjs /work/qjs_file_io.js
// with /work preopened (wasmtime: --dir <scratch>::/work; Ruby: preopens:).
import * as std from "qjs:std";

const path = "/work/io_out.txt";

const w = std.open(path, "w");
w.puts("hello from qjs file io\n");
w.close();

const r = std.open(path, "r");
const content = r.readAsString();
r.close();

std.out.puts("read back: " + content);
