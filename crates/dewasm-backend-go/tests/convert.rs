//! Go side of the whole-cache convert suite (ADR-54): converts every cached
//! real-world app with the Go backend and requires the conversion to complete
//! with non-empty source, without compiling or running it. The generic harness
//! lives in `dewasm-test-helper`.
//!
//! The harness names each conversion after the cache stem, and library-mode Go
//! output declares `package <name lowercased>` — so a dashed stem
//! (`sqlite3-binding`) is a conversion error now rather than something the
//! backend silently rewrites. The suite therefore runs through a thin wrapper
//! that derives a conforming name first, exactly as the e2e suite's
//! `convert_app` override does.

use anyhow::Result;
use dewasm_backend::{Backend, GenOptions, OutputFile, SupportStatus};
use dewasm_backend_go::GoBackend;
use dewasm_core::feature::Feature;
use dewasm_core::ir::Module;

mod common;

struct GoConvert;

impl Backend for GoConvert {
    fn name(&self) -> &str {
        GoBackend.name()
    }

    fn file_extension(&self) -> &str {
        GoBackend.file_extension()
    }

    fn has_wasi_p1(&self, name: &str) -> bool {
        GoBackend.has_wasi_p1(name)
    }

    fn feature_status(&self, feature: Feature) -> SupportStatus {
        GoBackend.feature_status(feature)
    }

    fn generate(&self, module: &Module, opts: &GenOptions) -> Result<Vec<OutputFile>> {
        let mut opts = opts.clone();
        opts.module_name = common::go_module_name(&opts.module_name);
        GoBackend.generate(module, &opts)
    }
}

dewasm_test_helper::apps_convert_suite!(GoConvert);
