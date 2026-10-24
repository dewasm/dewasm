#include <Python.h>
#include <stdlib.h>
#include <string.h>

static PyObject *globals;
static char *result;
static Py_ssize_t result_length;
static int result_error;

__attribute__((export_name("pyg_result_length")))
int pyg_result_length(void) { return (int)result_length; }

__attribute__((export_name("pyg_result_error")))
int pyg_result_error(void) { return result_error; }

static void keep(PyObject *string, int error) {
  Py_ssize_t length;
  const char *utf8 = PyUnicode_AsUTF8AndSize(string, &length);
  free(result);
  result = malloc(length + 1);
  memcpy(result, utf8, length + 1);
  result_length = length;
  result_error = error;
}

static const char *SETUP =
    "import io, json, sys, traceback\n"
    "import mentos\n"
    "_mentos = mentos.Mentos()\n"
    "def _dispatch(header_json, text):\n"
    "    header = json.loads(header_json)\n"
    "    method, args, kwargs, lexer = _mentos._parse_header(header)\n"
    "    if lexer:\n"
    "        lexer = str(lexer)\n"
    "    captured = io.TextIOWrapper(io.BytesIO(), encoding='utf-8')\n"
    "    saved = sys.stdout\n"
    "    sys.stdout = captured\n"
    "    try:\n"
    "        res = _mentos.get_data(method, lexer, args, kwargs, text)\n"
    "    except Exception:\n"
    "        return (True, traceback.format_exc())\n"
    "    finally:\n"
    "        sys.stdout = saved\n"
    "    if res is None:\n"
    "        captured.flush()\n"
    "        raw = captured.buffer.getvalue()\n"
    "        message = 'no result from mentos'\n"
    "        if len(raw) > 4:\n"
    "            message = json.loads(raw[4:].decode('utf-8', 'replace')).get('error', message)\n"
    "        return (True, message)\n"
    "    return (False, res)\n"
    "def _warm():\n"
    "    for lang in ['html', 'ruby', 'python', 'javascript', 'c', 'bash', 'json', 'sql', 'erb']:\n"
    "        header = {'method': 'highlight', 'args': None,\n"
    "                  'kwargs': {'lexer': lang, 'options': {'nowrap': True, 'outencoding': 'utf-8'}, 'bytes': 5}}\n"
    "        _dispatch(json.dumps(header), 'x = 1')\n"
    "    _dispatch(json.dumps({'method': 'css', 'args': ['html', '.highlight'], 'kwargs': {}}), '')\n"
    "    _dispatch(json.dumps({'method': 'version', 'args': [], 'kwargs': {}}), '')\n"
    "_warm()\n";

__attribute__((export_name("pyg_init")))
void pyg_init(void) {
  PyConfig config;
  PyConfig_InitIsolatedConfig(&config);
  config.install_signal_handlers = 0;
  config.buffered_stdio = 0;
  config.write_bytecode = 0;
  config.module_search_paths_set = 1;
  PyWideStringList_Append(&config.module_search_paths, L"/gem/python3.14");
  PyWideStringList_Append(&config.module_search_paths, L"/gem/lib/pygments");
  PyConfig_SetString(&config, &config.program_name, L"python");
  PyStatus status = Py_InitializeFromConfig(&config);
  PyConfig_Clear(&config);
  if (PyStatus_Exception(status)) {
    fprintf(stderr, "pyg_init: %s\n", status.err_msg ? status.err_msg : "Py_InitializeFromConfig failed");
    return;
  }
  globals = PyModule_GetDict(PyImport_AddModule("__main__"));
  Py_INCREF(globals);
  PyObject *r = PyRun_String(SETUP, Py_file_input, globals, globals);
  if (!r) {
    PyErr_Print();
    return;
  }
  Py_DECREF(r);
}

__attribute__((export_name("pyg_mentos")))
const char *pyg_mentos(const char *header, int header_len, const char *code, int code_len) {
  PyObject *function = PyDict_GetItemString(globals, "_dispatch");
  PyObject *py_header = PyUnicode_DecodeUTF8(header, header_len, "replace");
  PyObject *py_code = PyUnicode_DecodeUTF8(code, code_len, "replace");
  PyObject *r = PyObject_CallFunctionObjArgs(function, py_header, py_code, NULL);
  Py_XDECREF(py_header);
  Py_XDECREF(py_code);
  if (!r) {
    PyObject *type, *value, *trace;
    PyErr_Fetch(&type, &value, &trace);
    PyObject *text = value ? PyObject_Str(value) : PyUnicode_FromString("pyg_mentos failed");
    keep(text, 1);
    Py_DECREF(text);
    Py_XDECREF(type);
    Py_XDECREF(value);
    Py_XDECREF(trace);
    return result;
  }
  PyObject *error = PyTuple_GetItem(r, 0);
  PyObject *text = PyTuple_GetItem(r, 1);
  keep(text, PyObject_IsTrue(error));
  Py_DECREF(r);
  return result;
}
