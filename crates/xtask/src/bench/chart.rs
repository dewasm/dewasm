//! The static SVG charts that ride above the tables in `docs/benchmarks.md`.
//!
//! Why files rather than a Mermaid block: the measured span is a factor of ~23000, so the value axis has to be log10, and Mermaid's `xychart` has no log scale. Why two files per chart rather than one that switches on `prefers-color-scheme`: GitHub's markdown sanitizer is unreliable about CSS inside an SVG, so each mode's colors are baked into their own file and a `<picture>` element picks between them.
//!
//! The form is a horizontal lollipop — a hairline rule from the axis minimum out to a dot, one row per runner, fastest at the top. Deliberately not bars: a bar encodes length measured from zero, and a log axis has no zero, so a bar's length would state a quantity that does not exist. The dot is the value; the rule only ties it back to the axis. Color carries the runner *family* (baseline / dewasm / interpreter), never the rank, and every row is direct-labelled as well, so nothing depends on telling two hues apart.

use std::fmt::Write as _;

use crate::bench::report::{Outcome, Report};

/// Overall image width. Height is derived from the row count.
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
    pub workload: &'static str,
    /// File stem under `docs/benchmarks/`; the dark file is `<stem>-dark.svg`.
    pub stem: &'static str,
    pub alt: String,
    pub light: String,
    pub dark: String,
}

/// What the x axis measures.
#[derive(Clone, Copy, PartialEq)]
enum Metric {
    /// Slowdown against wasmtime on the same workload — the same quantity as the table's `vs wasmtime` column.
    Ratio,
    /// Wall time in seconds (rendered in whatever unit reads naturally).
    Wall,
}

struct Spec {
    workload: &'static str,
    stem: &'static str,
    metric: Metric,
    title: &'static str,
    /// Names the quantity in the alt text.
    quantity: &'static str,
}

/// The three charts, chosen for the three things the suite actually answers: raw compute, startup, and a real application. A workload that is absent from the record simply produces no chart.
const SPECS: &[Spec] = &[
    Spec {
        workload: "kernel/i32_alu",
        stem: "i32-alu",
        metric: Metric::Ratio,
        title: "kernel/i32_alu — compute cost relative to wasmtime (log scale)",
        quantity: "compute cost relative to wasmtime",
    },
    Spec {
        workload: "app/cowsay",
        stem: "cowsay",
        metric: Metric::Wall,
        title: "app/cowsay — wall time, fastest of the timed runs (log scale)",
        quantity: "wall time",
    },
    Spec {
        workload: "app/sqlite3_query",
        stem: "sqlite3-query",
        metric: Metric::Wall,
        title: "app/sqlite3_query — wall time, fastest of the timed runs (log scale)",
        quantity: "wall time",
    },
];

/// Which of the three colors a runner wears. Assigned by family so that a filter or a re-sort never repaints a runner.
#[derive(Clone, Copy, PartialEq)]
enum Family {
    Baseline,
    Dewasm,
    Interpreter,
}

fn family(runner: &str) -> Family {
    if runner.starts_with("dewasm-") {
        Family::Dewasm
    } else if runner.starts_with("pywasm") || runner.starts_with("wardite") {
        Family::Interpreter
    } else {
        // wasmtime, and anything later added beside it as a reference runtime.
        Family::Baseline
    }
}

/// Slots 1-3 of the validated categorical palette, plus the surface and text tokens. Dark is a selected variant — its own steps for the dark surface, not an inversion of the light one.
struct Theme {
    surface: &'static str,
    text_primary: &'static str,
    text_secondary: &'static str,
    baseline: &'static str,
    dewasm: &'static str,
    interpreter: &'static str,
}

const LIGHT: Theme = Theme {
    surface: "#fcfcfb",
    text_primary: "#0b0b0b",
    text_secondary: "#52514e",
    baseline: "#2a78d6",
    dewasm: "#008300",
    interpreter: "#e87ba4",
};

const DARK: Theme = Theme {
    surface: "#1a1a19",
    text_primary: "#ffffff",
    text_secondary: "#c3c2b7",
    baseline: "#3987e5",
    dewasm: "#008300",
    interpreter: "#d55181",
};

impl Theme {
    fn color(&self, family: Family) -> &'static str {
        match family {
            Family::Baseline => self.baseline,
            Family::Dewasm => self.dewasm,
            Family::Interpreter => self.interpreter,
        }
    }
}

/// One plotted runner.
struct Row {
    label: String,
    value: f64,
    family: Family,
}

/// Every chart the record has the data for, in document order. A workload the sweep did not run is skipped rather than fatal — `--render` has to work on an old or filtered record too.
pub fn charts(report: &Report) -> Vec<Chart> {
    SPECS
        .iter()
        .filter_map(|spec| build(report, spec))
        .collect()
}

fn build(report: &Report, spec: &Spec) -> Option<Chart> {
    let rows = rows(report, spec)?;
    let alt = alt_text(spec, &rows);
    Some(Chart {
        workload: spec.workload,
        stem: spec.stem,
        light: svg(spec, &rows, &LIGHT, &alt),
        dark: svg(spec, &rows, &DARK, &alt),
        alt,
    })
}

/// The measured runners for one workload, sorted fastest first. `None` when the workload is missing, when the ratio has no wasmtime baseline to divide by, or when there is only one row (which is a number, not a chart).
fn rows(report: &Report, spec: &Spec) -> Option<Vec<Row>> {
    // The same rule the table uses, so the chart and the numbers under it cannot disagree: an app has no iteration parameter, so its comparable quantity is whole wall time.
    let is_app = spec.workload.starts_with("app/");
    let measured: Vec<(&str, f64)> = report
        .results
        .iter()
        .filter(|cell| cell.workload == spec.workload)
        .filter_map(|cell| match &cell.outcome {
            Outcome::Ok(m) => {
                let value = if is_app {
                    Some(m.total.min_s)
                } else {
                    m.ns_per_op_min
                };
                value
                    .filter(|v| *v > 0.0)
                    .map(|v| (cell.runner.as_str(), v))
            }
            _ => None,
        })
        .collect();

    // A ratio needs the baseline the table divides by; without it there is no chart, not a chart of raw times.
    let scale = match spec.metric {
        Metric::Ratio => measured
            .iter()
            .find(|(runner, _)| *runner == "wasmtime")
            .map(|(_, value)| *value)?,
        Metric::Wall => 1.0,
    };

    let mut rows: Vec<Row> = measured
        .into_iter()
        .map(|(runner, value)| Row {
            label: runner.to_string(),
            value: value / scale,
            family: family(runner),
        })
        .collect();
    if rows.len() < 2 {
        return None;
    }
    rows.sort_by(|a, b| a.value.total_cmp(&b.value));
    Some(rows)
}

/// Alt text derived from the data rather than written by hand, so it states the finding and cannot go stale when the suite is re-run.
fn alt_text(spec: &Spec, rows: &[Row]) -> String {
    let fastest = &rows[0];
    let slowest = &rows[rows.len() - 1];
    let span = slowest.value / fastest.value;
    format!(
        "{}: {} for {} runners on a log scale, fastest first. {} is fastest at {}, then {} at {}; {} is slowest at {} — a span of {}. The table below carries every number.",
        spec.workload,
        spec.quantity,
        rows.len(),
        fastest.label,
        fmt_value(spec.metric, fastest.value),
        rows[1].label,
        fmt_value(spec.metric, rows[1].value),
        slowest.label,
        fmt_value(spec.metric, slowest.value),
        fmt_ratio(span),
    )
}

fn svg(spec: &Spec, rows: &[Row], theme: &Theme, alt: &str) -> String {
    let plot_bottom = PLOT_TOP + ROW_H * rows.len() as f64;
    let height = plot_bottom + 30.0;

    // The axis starts at the power of ten at or below the fastest value, so the fastest row still gets a readable rule, and ends at the slowest value itself — rounding that end up to the next power of ten would leave most of the plot empty.
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
            escape(&fmt_tick(spec.metric, value))
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
            escape(&fmt_value(spec.metric, row.value))
        );
    }

    let _ = writeln!(
        out,
        r#"<text x="{PAD:.1}" y="24" font-size="15" font-weight="600" fill="{}">{}</text>"#,
        theme.text_primary,
        escape(spec.title)
    );
    render_legend(&mut out, theme);
    out.push_str("</svg>\n");
    out
}

/// The legend is required even though every row is labelled: it is what names the three families, which the row labels only imply.
fn render_legend(out: &mut String, theme: &Theme) {
    let entries = [
        (theme.baseline, "wasmtime (baseline)"),
        (theme.dewasm, "dewasm backends"),
        (theme.interpreter, "wasm interpreters"),
    ];
    let y = 48.0;
    let mut x = PAD;
    for (color, label) in entries {
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
        x += 17.0 + text_width(label, FONT) + 22.0;
    }
}

/// A rough advance width, enough to lay out the legend without measuring text we cannot measure.
fn text_width(text: &str, font_size: f64) -> f64 {
    text.chars().count() as f64 * font_size * 0.55
}

fn fmt_value(metric: Metric, value: f64) -> String {
    match metric {
        Metric::Ratio => fmt_ratio(value),
        Metric::Wall => fmt_time(value),
    }
}

/// A slowdown factor. Spelled exactly as the table spells it below 1000x, and rounded to three significant figures above it — a chart label reads `23100x`, not `23102x`.
fn fmt_ratio(ratio: f64) -> String {
    if ratio < 10.0 {
        format!("{ratio:.2}x")
    } else if ratio < 1000.0 {
        format!("{ratio:.0}x")
    } else {
        format!("{}x", sig3(ratio))
    }
}

/// Wall time at three significant figures, in the unit a reader would say out loud.
fn fmt_time(seconds: f64) -> String {
    if seconds < 1.0 {
        format!("{} ms", sig3(seconds * 1000.0))
    } else {
        format!("{} s", sig3(seconds))
    }
}

/// A power-of-ten gridline label.
fn fmt_tick(metric: Metric, value: f64) -> String {
    match metric {
        Metric::Ratio => {
            if value >= 1.0 {
                format!("{value:.0}x")
            } else {
                format!("{value}x")
            }
        }
        Metric::Wall => {
            if value < 1.0 {
                format!("{:.0} ms", value * 1000.0)
            } else {
                format!("{value:.0} s")
            }
        }
    }
}

/// `value` at three significant figures, with no trailing zeros beyond them.
fn sig3(value: f64) -> String {
    if !value.is_finite() || value <= 0.0 {
        return format!("{value:.0}");
    }
    let exponent = value.log10().floor() as i32;
    let step = 10f64.powi(exponent - 2);
    let rounded = (value / step).round() * step;
    let decimals = (2 - exponent).max(0) as usize;
    format!("{rounded:.decimals$}")
}

/// XML text escaping. Runner labels are safe, titles and generated alt text are not guaranteed to be.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
