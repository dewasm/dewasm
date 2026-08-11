//! The static SVG charts that ride above the tables in `docs/benchmarks/results.md`.
//!
//! SVG files, not Mermaid: the span forces a log10 axis, which Mermaid's `xychart` lacks.
//! Two files per chart, not CSS `prefers-color-scheme`: GitHub's sanitizer is unreliable about CSS inside an SVG, so a `<picture>` picks between baked-in modes.
//!
//! The form is a horizontal lollipop, fastest at the top, not bars, because a bar encodes length from a zero that a log axis does not have.
//! Color carries the runner family, never the rank, and every row is direct-labelled.
//!
//! The axis is always seconds (per *iteration* for a microbenchmark, per *run* for an app, the title says which), never a ratio, so charts can be read against each other.
//! The plotted statistic is the median: the minimum is the better estimator (noise is one-sided) but the median is what a user experiences, and here they differ by well under 1%.
//!
//! [`lollipop`] and the theme around it are the drawing, with the unit left open; `cargo xtask size` draws its byte figures with the same function so the two records look like one family of charts.

use std::fmt::Write as _;

use crate::bench::report::{ordered_workloads, Outcome, Report};

/// Overall image width.
/// Height is derived from the row count.
const WIDTH: f64 = 820.0;
/// Outer padding used for the title and legend.
const PAD: f64 = 14.0;
/// Right edge of the runner-label column.
const LABEL_RIGHT: f64 = 152.0;
/// The plot area: gridlines, rules and dots live between these two.
const PLOT_LEFT: f64 = 168.0;
const PLOT_RIGHT: f64 = 726.0;
/// Right edge of the value column, which sits outside the plot so a label can never collide with a dot.
const VALUE_RIGHT: f64 = WIDTH - PAD;
/// First row's top edge, below the title and the legend.
const PLOT_TOP: f64 = 74.0;
const ROW_H: f64 = 24.0;
const FONT: f64 = 12.0;

/// One workload's chart, rendered in both modes plus the alt text they share.
pub struct Chart {
    /// The workload label this illustrates, used to place it above the matching table.
    pub workload: String,
    /// File stem under `docs/benchmarks/`; the dark file is `<stem>-dark.svg`.
    pub stem: String,
    pub alt: String,
    pub light: String,
    pub dark: String,
}

/// What one point on the axis is a duration *of*.
/// Both arms are seconds: this only names the denominator, which the title and alt text have to state or the two kinds of chart would look comparable when they are not.
#[derive(Clone, Copy, PartialEq)]
enum Quantity {
    /// A microbenchmark: compute time `t(N) - t(0)` divided by the calibrated iteration count.
    PerIteration,
    /// An app: the whole wall time of one run, startup included.
    PerRun,
}

impl Quantity {
    /// How the axis names itself, in the title and in the alt text.
    fn phrase(self) -> &'static str {
        match self {
            Quantity::PerIteration => "seconds per iteration",
            Quantity::PerRun => "seconds per run",
        }
    }
}

/// Which of the four colors a row wears.
/// Assigned by family so that a filter or a re-sort never repaints a runner.
#[derive(Clone, Copy, PartialEq)]
pub enum Family {
    /// wasmtime here; the wasm binary itself in the size record.
    Baseline,
    Dewasm,
    /// A wasm runtime executing the module natively: wasmer, wasmedge, wazero, wasm3.
    /// Split out from [`Family::Baseline`] so a reader does not have to know which of them is the reference, and from [`Family::Interpreter`] because "an interpreter written in Go" and "an interpreter written in Ruby" are not the same class of thing.
    Native,
    /// A wasm interpreter written in a host language: pywasm, wardite.
    /// The comparison dewasm actually cares about.
    Interpreter,
}

fn family(runner: &str) -> Family {
    match runner {
        "wasmtime" => Family::Baseline,
        "wasmer" | "wasmedge" | "wazero" | "wasm3" => Family::Native,
        r if r.starts_with("dewasm-") => Family::Dewasm,
        r if r.starts_with("pywasm") || r.starts_with("wardite") => Family::Interpreter,
        // Anything later added beside wasmtime as a reference runtime.
        _ => Family::Baseline,
    }
}

/// Slots 1-4 of the validated categorical palette, plus the surface and text tokens.
/// Dark is a selected variant: its own steps for the dark surface, not an inversion of the light one.
pub struct Theme {
    surface: &'static str,
    text_primary: &'static str,
    text_secondary: &'static str,
    baseline: &'static str,
    dewasm: &'static str,
    native: &'static str,
    interpreter: &'static str,
}

pub const LIGHT: Theme = Theme {
    surface: "#fcfcfb",
    text_primary: "#0b0b0b",
    text_secondary: "#52514e",
    baseline: "#2a78d6",
    dewasm: "#008300",
    native: "#eda100",
    interpreter: "#e87ba4",
};

pub const DARK: Theme = Theme {
    surface: "#1a1a19",
    text_primary: "#ffffff",
    text_secondary: "#c3c2b7",
    baseline: "#3987e5",
    dewasm: "#008300",
    native: "#c98500",
    interpreter: "#d55181",
};

impl Theme {
    pub fn color(&self, family: Family) -> &'static str {
        match family {
            Family::Baseline => self.baseline,
            Family::Dewasm => self.dewasm,
            Family::Native => self.native,
            Family::Interpreter => self.interpreter,
        }
    }
}

pub struct Row {
    pub label: String,
    /// In whatever unit the chart's [`Units`] spells; seconds here, bytes in the size record.
    pub value: f64,
    pub family: Family,
}

/// How a chart spells a plotted value and a power-of-ten gridline label.
/// The renderer is unit-agnostic; these two functions are the whole difference between a seconds chart and a bytes chart.
pub struct Units {
    pub value: fn(f64) -> String,
    pub tick: fn(f64) -> String,
}

const SECONDS: Units = Units {
    value: fmt_time,
    tick: fmt_tick,
};

const BENCH_LEGEND: [(Family, &str); 4] = [
    (Family::Baseline, "wasmtime (baseline)"),
    (Family::Dewasm, "dewasm backends"),
    (Family::Native, "native runtimes"),
    (Family::Interpreter, "wasm interpreters in a host language"),
];

/// A chart for every workload the record has at least two measurements for, in document order.
/// A workload the run did not cover simply produces none: `--render` has to work on an old or filtered record too.
pub fn charts(report: &Report) -> Vec<Chart> {
    ordered_workloads(report)
        .into_iter()
        .filter_map(|workload| build(report, &workload))
        .collect()
}

fn build(report: &Report, workload: &str) -> Option<Chart> {
    // The same split the tables use, so the chart and the numbers under it cannot disagree: an app has no iteration parameter, so its comparable quantity is whole wall time.
    let quantity = if workload.starts_with("app/") {
        Quantity::PerRun
    } else {
        Quantity::PerIteration
    };
    let rows = rows(report, workload, quantity)?;
    let title = format!(
        "{workload}: {}, median of the timed runs (log scale)",
        quantity.phrase()
    );
    let alt = alt_text(workload, quantity, &rows);
    Some(Chart {
        workload: workload.to_string(),
        stem: stem(workload),
        light: lollipop(&title, &rows, &LIGHT, &alt, &SECONDS, &BENCH_LEGEND),
        dark: lollipop(&title, &rows, &DARK, &alt, &SECONDS, &BENCH_LEGEND),
        alt,
    })
}

/// A workload label as a file stem: `wat/i32_alu` becomes `wat-i32-alu`.
/// One flat directory, and the family stays visible in the filename.
fn stem(workload: &str) -> String {
    workload.replace(['/', '_'], "-")
}

/// The measured runners for one workload in seconds, sorted fastest first.
/// `None` when fewer than two runners produced a number, which is a value rather than a chart.
fn rows(report: &Report, workload: &str, quantity: Quantity) -> Option<Vec<Row>> {
    let mut rows: Vec<Row> = report
        .results
        .iter()
        .filter(|cell| cell.workload == workload)
        .filter_map(|cell| match &cell.outcome {
            Outcome::Ok(m) => {
                let seconds = match quantity {
                    // ns/op is what the table publishes; seconds is what the axis needs.
                    Quantity::PerIteration => m.ns_per_op_median.map(|ns| ns / 1e9),
                    Quantity::PerRun => Some(m.total.median_s),
                };
                seconds.filter(|v| *v > 0.0).map(|value| Row {
                    label: cell.runner.clone(),
                    value,
                    family: family(&cell.runner),
                })
            }
            _ => None,
        })
        .collect();
    if rows.len() < 2 {
        return None;
    }
    rows.sort_by(|a, b| a.value.total_cmp(&b.value));
    Some(rows)
}

/// Alt text derived from the data rather than written by hand, so it states the finding and cannot go stale when the suite is re-run.
fn alt_text(workload: &str, quantity: Quantity, rows: &[Row]) -> String {
    let fastest = &rows[0];
    let slowest = &rows[rows.len() - 1];
    let span = slowest.value / fastest.value;
    format!(
        "{workload}: {} for {} runners on a log scale, fastest first. {} is fastest at {}, then {} at {}; {} is slowest at {}, a span of {}. The table below carries every number.",
        quantity.phrase(),
        rows.len(),
        fastest.label,
        fmt_time(fastest.value),
        rows[1].label,
        fmt_time(rows[1].value),
        slowest.label,
        fmt_time(slowest.value),
        fmt_ratio(span),
    )
}

/// The drawing: a horizontal lollipop on a log10 axis, one row each, `rows` given smallest first.
/// `units` names the axis, `legend` names the colors.
pub fn lollipop(
    title: &str,
    rows: &[Row],
    theme: &Theme,
    alt: &str,
    units: &Units,
    legend: &[(Family, &str)],
) -> String {
    let plot_bottom = PLOT_TOP + ROW_H * rows.len() as f64;
    let height = plot_bottom + 30.0;

    // The axis starts at the power of ten at or below the fastest value, so the fastest row still gets a readable rule, and ends at the slowest value itself: rounding that end up to the next power of ten would leave most of the plot empty.
    let min = rows[0].value;
    let max = rows[rows.len() - 1].value;
    let first_tick = min.log10().floor() as i32;
    let domain_min = 10f64.powi(first_tick);
    let domain_max = if max > domain_min * 1.05 {
        max
    } else {
        domain_min * 10.0
    };
    let span = domain_max.log10() - domain_min.log10();
    // The dot's ring must stay inside the plot, so the value axis stops short of the right edge.
    let plot_end = PLOT_RIGHT - 8.0;
    let x_of = |value: f64| {
        let clamped = value.max(domain_min).min(domain_max);
        PLOT_LEFT + (clamped.log10() - domain_min.log10()) / span * (plot_end - PLOT_LEFT)
    };

    let mut out = String::new();
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH:.0}" height="{height:.0}" viewBox="0 0 {WIDTH:.0} {height:.0}" font-family="system-ui, -apple-system, Segoe UI, sans-serif" role="img" aria-label="{}">"#,
        escape(alt)
    );
    let _ = writeln!(out, "<title>{}</title>", escape(alt));
    let _ = writeln!(
        out,
        r#"<rect width="{WIDTH:.0}" height="{height:.0}" fill="{}"/>"#,
        theme.surface
    );

    // Gridlines and their labels are recessive: hairline, secondary ink, low opacity.
    let mut tick = first_tick;
    while 10f64.powi(tick) <= domain_max * 1.000001 {
        let value = 10f64.powi(tick);
        let x = x_of(value);
        let _ = writeln!(
            out,
            r#"<line x1="{x:.1}" y1="{:.1}" x2="{x:.1}" y2="{plot_bottom:.1}" stroke="{}" stroke-width="1" stroke-opacity="0.25"/>"#,
            PLOT_TOP - 6.0,
            theme.text_secondary
        );
        let _ = writeln!(
            out,
            r#"<text x="{x:.1}" y="{:.1}" text-anchor="middle" font-size="11" fill="{}">{}</text>"#,
            plot_bottom + 16.0,
            theme.text_secondary,
            escape(&(units.tick)(value))
        );
        tick += 1;
    }
    let _ = writeln!(
        out,
        r#"<line x1="{PLOT_LEFT:.1}" y1="{plot_bottom:.1}" x2="{PLOT_RIGHT:.1}" y2="{plot_bottom:.1}" stroke="{}" stroke-width="1" stroke-opacity="0.4"/>"#,
        theme.text_secondary
    );

    for (index, row) in rows.iter().enumerate() {
        let cy = PLOT_TOP + ROW_H * (index as f64 + 0.5);
        let x = x_of(row.value);
        let color = theme.color(row.family);
        let _ = writeln!(
            out,
            r#"<text x="{LABEL_RIGHT:.1}" y="{:.1}" text-anchor="end" font-size="{FONT}" fill="{}">{}</text>"#,
            cy + 4.0,
            theme.text_primary,
            escape(&row.label)
        );
        if x - PLOT_LEFT > 1.0 {
            let _ = writeln!(
                out,
                r#"<line x1="{PLOT_LEFT:.1}" y1="{cy:.1}" x2="{x:.1}" y2="{cy:.1}" stroke="{color}" stroke-width="2" stroke-opacity="0.45"/>"#
            );
        }
        // A 2px surface ring keeps the dot from merging into a gridline it happens to land on.
        let _ = writeln!(
            out,
            r#"<circle cx="{x:.1}" cy="{cy:.1}" r="5.5" fill="{color}" stroke="{}" stroke-width="2"/>"#,
            theme.surface
        );
        let _ = writeln!(
            out,
            r#"<text x="{VALUE_RIGHT:.1}" y="{:.1}" text-anchor="end" font-size="{FONT}" font-weight="600" fill="{}">{}</text>"#,
            cy + 4.0,
            theme.text_primary,
            escape(&(units.value)(row.value))
        );
    }

    let _ = writeln!(
        out,
        r#"<text x="{PAD:.1}" y="24" font-size="15" font-weight="600" fill="{}">{}</text>"#,
        theme.text_primary,
        escape(title)
    );
    render_legend(&mut out, theme, legend);
    out.push_str("</svg>\n");
    out
}

/// The legend is required even though every row is labelled: it is what names the families, which the row labels only imply.
fn render_legend(out: &mut String, theme: &Theme, entries: &[(Family, &str)]) {
    let y = 48.0;
    let mut x = PAD;
    for (family, label) in entries.iter().copied() {
        let color = theme.color(family);
        let _ = writeln!(
            out,
            r#"<circle cx="{:.1}" cy="{y:.1}" r="5.5" fill="{color}" stroke="{}" stroke-width="2"/>"#,
            x + 5.5,
            theme.surface
        );
        let _ = writeln!(
            out,
            r#"<text x="{:.1}" y="{:.1}" font-size="{FONT}" fill="{}">{}</text>"#,
            x + 17.0,
            y + 4.0,
            theme.text_secondary,
            escape(label)
        );
        x += 17.0 + text_width(label, FONT) + 18.0;
    }
}

/// A rough advance width, enough to lay out the legend without measuring text we cannot measure.
fn text_width(text: &str, font_size: f64) -> f64 {
    text.chars().count() as f64 * font_size * 0.55
}

/// A factor between two plotted values, used for the span sentence in the alt text.
/// Rounded to three significant figures above 1000x: `23100x`, not `23102x`.
pub fn fmt_ratio(ratio: f64) -> String {
    if ratio < 10.0 {
        format!("{ratio:.2}x")
    } else if ratio < 1000.0 {
        format!("{ratio:.0}x")
    } else {
        format!("{}x", sig3(ratio))
    }
}

/// The SI prefix a duration reads best in: the largest unit that still leaves a mantissa of at least 1.
/// The `0.999999` slack is there because `10f64.powi(-6)` is not exactly `1e-6`, and a tick that landed one ULP low would otherwise be labelled `1000 ns` instead of `1 µs`.
fn si_unit(seconds: f64) -> (f64, &'static str) {
    const UNITS: [(f64, &str); 4] = [(1.0, "s"), (1e-3, "ms"), (1e-6, "µs"), (1e-9, "ns")];
    UNITS
        .into_iter()
        .find(|(scale, _)| seconds >= scale * 0.999999)
        .unwrap_or((1e-9, "ns"))
}

/// A duration at three significant figures with an SI prefix: `2.41 ns`, `55.0 µs`, `1.23 s`.
/// The unit is chosen from the *rounded* value, so 999.6 ns reads `1.00 µs` rather than `1000 ns`.
fn fmt_time(seconds: f64) -> String {
    let rounded = round3(seconds);
    let (scale, unit) = si_unit(rounded);
    let mantissa = rounded / scale;
    let decimals = if mantissa < 10.0 {
        2
    } else if mantissa < 100.0 {
        1
    } else {
        0
    };
    format!("{mantissa:.decimals$} {unit}")
}

/// A power-of-ten gridline label: `1 ns`, `10 ns`, `1 µs`, `1 ms`, `1 s`.
/// Bare mantissa and unit, no decimals: an exponent (`1e-9`) is a notation a reader has to decode, and these labels are read at a glance.
fn fmt_tick(seconds: f64) -> String {
    let (scale, unit) = si_unit(seconds);
    format!("{:.0} {unit}", seconds / scale)
}

/// `value` rounded to three significant figures.
pub fn round3(value: f64) -> f64 {
    if !value.is_finite() || value <= 0.0 {
        return value;
    }
    let step = 10f64.powi(value.log10().floor() as i32 - 2);
    (value / step).round() * step
}

/// `value` at three significant figures, with no trailing zeros beyond them.
fn sig3(value: f64) -> String {
    if !value.is_finite() || value <= 0.0 {
        return format!("{value:.0}");
    }
    let decimals = (2 - value.log10().floor() as i32).max(0) as usize;
    format!("{:.decimals$}", round3(value))
}

/// XML text escaping.
/// Runner labels are safe, titles and generated alt text are not guaranteed to be.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
