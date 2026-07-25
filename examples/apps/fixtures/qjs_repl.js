// Phase 5a QuickJS scripted-REPL fixture (our own committed code, ADR-9).
//
// QuickJS's built-in interactive REPL (qjs -i / bare qjs) drives a terminal
// line editor over stdin: piped in, it emits ANSI escape sequences and never
// terminates on EOF (see docs/apps-audit.md). This fixture is the closest
// byte-stable equivalent — a read-eval-print loop over piped stdin that
// exercises the same stdin-read + evalScript path a REPL would, without the
// tty-only line editor. Run as `qjs /work/qjs_repl.js` with stdin piped.
import * as std from "qjs:std";

let line;
while ((line = std.in.getline()) !== null) {
  line = line.trim();
  if (line === "" || line === "\\q") continue;
  try {
    std.out.puts("=> " + std.evalScript(line) + "\n");
  } catch (e) {
    std.out.puts("!! " + e + "\n");
  }
}
