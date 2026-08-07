//! The static SVG figures `docs/sizes/results.md` embeds: one for the runtimes, one per app.
//!
//! Drawn by [`crate::bench::chart::lollipop`] with a bytes axis, so a size figure and a benchmark figure are the same picture in different units — same palette, same geometry, same log10 axis, same light/dark pair behind a `<picture>`. Rows are smallest first; color carries what the row *is* (the wasm binary, converted source, a native runtime), never its rank.

use crate::bench::chart::{fmt_ratio, lollipop, Family, Row, Units, DARK, LIGHT};
use crate::size::report::{fmt_byte_tick, fmt_bytes_f, App, Report};

/// One figure, rendered in both modes plus the alt text they share.
pub struct Chart {
    /// What this illustrates — `"runtimes"` or an app's cache stem — used to place it above the matching table.
    pub key: String,
    /// File stem under `docs/sizes/figs/`; the dark file is `<stem>-dark.svg`.
    pub stem: String,
    pub alt: String,
    pub light: String,
    pub dark: String,
}

const BYTES: Units = Units {
    value: fmt_bytes_f,
    tick: fmt_byte_tick,
};

/// Every figure the record supports, in document order. A section the record does not cover simply produces none — `--render` has to work on an older record too.
pub fn charts(report: &Report) -> Vec<Chart> {
    let mut charts: Vec<Chart> = Vec::new();
    charts.extend(runtimes(report));
    charts.extend(report.apps.iter().filter_map(app_chart));
    charts
}

fn runtimes(report: &Report) -> Option<Chart> {
    let mut rows: Vec<Row> = report
        .runtimes
        .iter()
        .filter_map(|runtime| {
            Some(Row {
                label: runtime.runtime.clone(),
                value: runtime.outcome.bytes()? as f64,
                family: Family::Native,
            })
        })
        .collect();
    if rows.len() < 2 {
        return None;
    }
    rows.sort_by(|a, b| a.value.total_cmp(&b.value));
    let title = "Native wasm runtimes: installed size on this host (log scale)";
    let alt = alt_text(
        "Installed size of the native wasm runtimes on this host",
        &rows,
    );
    let legend = [(Family::Native, "native wasm runtimes")];
    Some(Chart {
        key: "runtimes".to_string(),
        stem: "runtimes".to_string(),
        light: lollipop(title, &rows, &LIGHT, &alt, &BYTES, &legend),
        dark: lollipop(title, &rows, &DARK, &alt, &BYTES, &legend),
        alt,
    })
}

fn app_chart(app: &App) -> Option<Chart> {
    let mut rows: Vec<Row> = Vec::new();
    if let Some(bytes) = app.wasm.bytes() {
        rows.push(Row {
            label: "wasm binary".to_string(),
            value: bytes as f64,
            family: Family::Baseline,
        });
    }
    rows.extend(app.targets.iter().filter_map(|cell| {
        Some(Row {
            label: cell.target.clone(),
            value: cell.outcome.bytes()? as f64,
            family: Family::Dewasm,
        })
    }));
    if rows.len() < 2 {
        return None;
    }
    rows.sort_by(|a, b| a.value.total_cmp(&b.value));
    let title = format!("{}: wasm binary and converted source (log scale)", app.file);
    let alt = alt_text(
        &format!(
            "{}: the wasm binary against the source each backend converts it into",
            app.file
        ),
        &rows,
    );
    let legend = [
        (Family::Baseline, "wasm binary"),
        (Family::Dewasm, "converted source"),
    ];
    Some(Chart {
        key: app.app.clone(),
        stem: format!("app-{}", app.app.replace('_', "-")),
        light: lollipop(&title, &rows, &LIGHT, &alt, &BYTES, &legend),
        dark: lollipop(&title, &rows, &DARK, &alt, &BYTES, &legend),
        alt,
    })
}

/// Alt text derived from the data rather than written by hand, so it states the finding and cannot go stale when the record is remade.
fn alt_text(subject: &str, rows: &[Row]) -> String {
    let smallest = &rows[0];
    let largest = &rows[rows.len() - 1];
    format!(
        "{subject}, {} rows on a log scale, smallest first. {} is smallest at {}, then {} at {}; {} is largest at {}, a span of {}. The table below carries every number.",
        rows.len(),
        smallest.label,
        fmt_bytes_f(smallest.value),
        rows[1].label,
        fmt_bytes_f(rows[1].value),
        largest.label,
        fmt_bytes_f(largest.value),
        fmt_ratio(largest.value / smallest.value),
    )
}
