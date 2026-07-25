//! Component-model e2e (ADR-20/ADR-21): committed `.wat` component
//! fixtures converted with `generate_component` and run under real ruby —
//! one with a custom host provider (string lowering through memory), one
//! against the bundled `Rt::WASIP2` (resources + list<u8> + result).
//!
//! The component model is an extension badge, not part of the wasm-1.0 +
//! WASI-p1 tier ladder (ADR-23): it's gated on the `ComponentModel`
//! feature declaration rather than a tier, which is why these tests
//! assert the badge directly instead of going through a case table.

use dewasmify_backend::{Backend, GenOptions, Mode, RuntimeLinkage, SupportStatus};
use dewasmify_backend_ruby::RubyBackend;
use dewasmify_core::feature::Feature;

use crate::support::{examples_dir, run_ruby};

fn require_component_model_badge() {
    assert_ne!(
        RubyBackend.feature_status(Feature::ComponentModel),
        SupportStatus::Unsupported,
        "component e2e requires the ComponentModel badge (ADR-23)"
    );
}

fn convert_component(fixture: &str, mode: Mode, name: &str, default_wasi: bool) -> String {
    let bytes = wat::parse_file(examples_dir().join(fixture)).expect("parse wat");
    let component = dewasmify_core::build_component(&bytes).expect("build component");
    dewasmify_backend_ruby::generate_component(
        &component,
        &GenOptions {
            mode,
            module_name: name.to_string(),
            runtime: RuntimeLinkage::Embedded,
            default_wasi,
        },
    )
    .expect("generate component")
    .remove(0)
    .contents
}

#[test]
fn component_custom_host_string_roundtrip() {
    require_component_model_badge();
    let class = convert_component(
        "component_host_log.wat",
        Mode::Library,
        "demo",
        false, // custom host, no bundled WASI p2
    );
    let glue = r#"
class Host
  attr_reader :seen
  def import(name)
    raise "unexpected import #{name}" unless name == "example:host/log@0.2.0#log"
    ->(msg) { (@seen ||= []) << msg; msg.bytesize }
  end
  def resource_drop(id, h) = nil
end

host = Host.new
inst = Demo.new(host)
r = inst.invoke("run")
raise "bad result #{r.inspect}" unless r == 5
raise "bad host log #{host.seen.inspect}" unless host.seen == ["hi p2"]
puts "component-host-ok"
"#;
    let out = run_ruby(&format!("{class}\n{glue}"), &[]);
    assert_eq!(out.trim(), "component-host-ok");
}

#[test]
fn component_bundled_wasi_p2_stdout() {
    require_component_model_badge();
    let class = convert_component(
        "component_p2_hello.wat",
        Mode::Library,
        "hello",
        true, // bundled Rt::WASIP2
    );
    let glue = r#"
inst = Hello.new
r = inst.invoke("run")
raise "bad result #{r.inspect}" unless r == 0
"#;
    let out = run_ruby(&format!("{class}\n{glue}"), &[]);
    assert_eq!(out, "hello from p2\n");
}
