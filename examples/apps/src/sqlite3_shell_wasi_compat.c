/* wasi-libc declares getpid but ships no definition (WASI preview1 has no
   process ids), and sqlite3's shell.c references it unconditionally on a
   debug path (the SQLITE_DEBUG_BREAK prompt), so the shell link needs this
   stand-in.
   The engine needs none: sqlite3.c defines SQLITE_WASI under __wasi__ and
   maps its own osGetpid to the same constant. */
#include <unistd.h>

pid_t getpid(void) { return 1; }
