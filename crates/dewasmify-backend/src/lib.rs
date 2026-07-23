//! Backend trait and code emission utilities shared by all language
//! backends.

use dewasmify_core::ir;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Emit a module that is instantiated with an imports object and exposes
    /// its exports to the host language.
    Library,
    /// Emit a runnable program that wires up WASI and calls `_start`.
    Standalone,
}

#[derive(Clone, Debug)]
pub struct GenOptions {
    pub mode: Mode,
    /// Class/package/module name for the generated code.
    pub module_name: String,
}

pub struct OutputFile {
    pub name: String,
    pub contents: String,
}

pub trait Backend {
    fn name(&self) -> &str;
    fn file_extension(&self) -> &str;
    fn generate(&self, module: &ir::Module, opts: &GenOptions) -> anyhow::Result<Vec<OutputFile>>;
}

/// Indentation-aware line writer.
pub struct CodeWriter {
    buf: String,
    indent: usize,
    indent_str: &'static str,
}

impl CodeWriter {
    pub fn new(indent_str: &'static str) -> Self {
        CodeWriter { buf: String::new(), indent: 0, indent_str }
    }

    pub fn line(&mut self, s: impl AsRef<str>) {
        let s = s.as_ref();
        if s.is_empty() {
            self.buf.push('\n');
            return;
        }
        for _ in 0..self.indent {
            self.buf.push_str(self.indent_str);
        }
        self.buf.push_str(s);
        self.buf.push('\n');
    }

    pub fn raw(&mut self, s: impl AsRef<str>) {
        self.buf.push_str(s.as_ref());
    }

    pub fn indent(&mut self) {
        self.indent += 1;
    }

    pub fn dedent(&mut self) {
        self.indent -= 1;
    }

    /// line(open); indent(); f(); dedent(); line(close)
    pub fn block(
        &mut self,
        open: impl AsRef<str>,
        close: impl AsRef<str>,
        f: impl FnOnce(&mut Self),
    ) {
        self.line(open);
        self.indent();
        f(self);
        self.dedent();
        self.line(close);
    }

    pub fn finish(self) -> String {
        self.buf
    }
}
