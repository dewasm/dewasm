import * as std from "qjs:std";

const path = "/work/io_out.txt";

const w = std.open(path, "w");
w.puts("hello from qjs file io\n");
w.close();

const r = std.open(path, "r");
const content = r.readAsString();
r.close();

std.out.puts("read back: " + content);
