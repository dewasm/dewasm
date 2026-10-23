// A reactor around CPython for Pygments: `pyg_init` starts the interpreter, imports Pygments,
// and runs the common lexers once; `pyg_highlight` turns code into HTML.
// Wizer runs `pyg_init` ahead of time and writes the initialized memory into the module, so a
// converted copy starts with Pygments imported.
#include <Python.h>
#include <stdlib.h>
#include <string.h>

static PyObject *globals;
static char *result;
static Py_ssize_t result_length;

static int run(const char *source) {
  PyObject *r = PyRun_String(source, Py_file_input, globals, globals);
  if (!r) {
    PyErr_Print();
    return -1;
  }
  Py_DECREF(r);
  return 0;
}

// `[] -> []`, the shape Wizer requires of an initialization function; a failure is reported on
// `stderr`.
// An unknown language name is refused from the alias table: `get_lexer_by_name` would otherwise
// import every lexer module looking for it.
__attribute__((export_name("pyg_init")))
void pyg_init(void) {
  PyConfig config;
  PyConfig_InitIsolatedConfig(&config);
  config.install_signal_handlers = 0;
  config.buffered_stdio = 0;
  config.write_bytecode = 0;
  config.module_search_paths_set = 1;
  PyWideStringList_Append(&config.module_search_paths, L"/lib/python3.14");
  PyWideStringList_Append(&config.module_search_paths, L"/site");
  PyConfig_SetString(&config, &config.program_name, L"python");
  PyStatus status = Py_InitializeFromConfig(&config);
  PyConfig_Clear(&config);
  if (PyStatus_Exception(status)) {
    fprintf(stderr, "pyg_init: %s\n", status.err_msg ? status.err_msg : "Py_InitializeFromConfig failed");
    return;
  }
  globals = PyModule_GetDict(PyImport_AddModule("__main__"));
  Py_INCREF(globals);
  run(
      "import platform\n"
      "import pygments\n"
      "from pygments import highlight\n"
      "from pygments.lexers import get_lexer_by_name\n"
      "from pygments.lexers._mapping import LEXERS\n"
      "from pygments.formatters import HtmlFormatter\n"
      "_formatter = HtmlFormatter(nowrap=True)\n"
      "_aliases = {alias for entry in LEXERS.values() for alias in entry[2]}\n"
      "_version = f'Pygments {pygments.__version__} on CPython {platform.python_version()}'\n"
      "def _highlight(code, lang):\n"
      "    if lang not in _aliases:\n"
      "        return None\n"
      "    return highlight(code, get_lexer_by_name(lang), _formatter)\n"
      "for _lang in ['html', 'python', 'ruby', 'javascript', 'c', 'bash', 'json', 'sql', 'erb']:\n"
      "    _highlight('x = 1\\n', _lang)\n");
}

__attribute__((export_name("pyg_result_length")))
int pyg_result_length(void) { return (int)result_length; }

static const char *keep(PyObject *string) {
  Py_ssize_t length;
  const char *utf8 = PyUnicode_AsUTF8AndSize(string, &length);
  result = malloc(length + 1);
  memcpy(result, utf8, length + 1);
  result_length = length;
  return result;
}

static void reset(void) {
  free(result);
  result = NULL;
  result_length = 0;
}

// The HTML for `code` in language `lang`, or NULL for an unknown language or an error.
// The string stays valid until the next call.
__attribute__((export_name("pyg_highlight")))
const char *pyg_highlight(const char *code, int code_len, const char *lang, int lang_len) {
  reset();
  PyObject *function = PyDict_GetItemString(globals, "_highlight");
  PyObject *py_code = PyUnicode_DecodeUTF8(code, code_len, "replace");
  PyObject *py_lang = PyUnicode_DecodeUTF8(lang, lang_len, "replace");
  PyObject *r = PyObject_CallFunctionObjArgs(function, py_code, py_lang, NULL);
  Py_XDECREF(py_code);
  Py_XDECREF(py_lang);
  if (!r) {
    PyErr_Print();
    return NULL;
  }
  if (r == Py_None) {
    Py_DECREF(r);
    return NULL;
  }
  keep(r);
  Py_DECREF(r);
  return result;
}

// `Pygments 2.21.0 on CPython 3.14.6`.
__attribute__((export_name("pyg_version")))
const char *pyg_version(void) {
  reset();
  return keep(PyDict_GetItemString(globals, "_version"));
}
