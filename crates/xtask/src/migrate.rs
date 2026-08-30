//! `cargo xtask migrate-records`: upgrade every record under `records/` to its kind's current schema, in place.
//!
//! All cross-version knowledge lives here.
//! Every reader (`render-speed`, `render-size`, and any future record-consuming command) supports only the current schema and names this command when it meets an older record, so compatibility never spreads across readers.

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::bench::report::SkipKind;
use crate::bench::{self, records_dir, write_file, SIZE_SUFFIX, SPEED_SUFFIX};
use crate::size;

pub fn run() -> Result<()> {
    let dir = records_dir();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .with_context(|| format!("failed to read {}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort();
    for path in entries {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .with_context(|| format!("{} has no file name", path.display()))?
            .to_string();
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        #[derive(Deserialize)]
        struct SchemaProbe {
            schema: u32,
        }
        let probe: SchemaProbe = serde_json::from_str(&text)
            .with_context(|| format!("{} is not a record", path.display()))?;
        if name.ends_with(SPEED_SUFFIX) {
            match probe.schema {
                1 => {
                    let report = migrate_speed_v1(&text)
                        .with_context(|| format!("failed to migrate {name}"))?;
                    write_file(&path, &report.to_json()?)?;
                    println!("{name}: schema 1 -> {}", bench::report::SCHEMA);
                }
                bench::report::SCHEMA => println!("{name}: current"),
                other => bail!("{name}: unknown speed-record schema {other}"),
            }
        } else if name.ends_with(SIZE_SUFFIX) {
            match probe.schema {
                size::report::SCHEMA => println!("{name}: current"),
                other => bail!("{name}: unknown size-record schema {other}"),
            }
        } else {
            bail!("{name}: neither a speed nor a size record");
        }
    }
    Ok(())
}

/// The v1 speed record: identical to the current one except that a skipped cell carried its class inside the reason string.
mod v1 {
    use serde::Deserialize;

    use crate::bench::report;

    #[derive(Deserialize)]
    pub struct Report {
        pub schema: u32,
        pub generated_at: String,
        pub host: report::Host,
        pub settings: report::Settings,
        pub runtimes: Vec<report::Runtime>,
        pub results: Vec<Cell>,
    }

    #[derive(Deserialize)]
    pub struct Cell {
        pub workload: String,
        pub runner: String,
        #[serde(flatten)]
        pub outcome: Outcome,
    }

    #[derive(Deserialize)]
    #[serde(tag = "status", rename_all = "snake_case")]
    pub enum Outcome {
        Ok(report::Measurement),
        Skipped { reason: String },
        Failed { reason: String },
    }
}

fn migrate_speed_v1(text: &str) -> Result<bench::report::Report> {
    let old: v1::Report = serde_json::from_str(text).context("not a schema-1 speed record")?;
    anyhow::ensure!(old.schema == 1, "expected schema 1, found {}", old.schema);
    Ok(bench::report::Report {
        schema: bench::report::SCHEMA,
        generated_at: old.generated_at,
        host: old.host,
        settings: old.settings,
        runtimes: old.runtimes,
        results: old
            .results
            .into_iter()
            .map(|cell| bench::report::Cell {
                workload: cell.workload,
                runner: cell.runner,
                outcome: match cell.outcome {
                    v1::Outcome::Ok(m) => bench::report::Outcome::Ok(m),
                    v1::Outcome::Skipped { reason } => {
                        let (kind, reason) = classify_v1_reason(&reason);
                        bench::report::Outcome::Skipped { kind, reason }
                    }
                    v1::Outcome::Failed { reason } => bench::report::Outcome::Failed { reason },
                },
            })
            .collect(),
    })
}

/// A v1 skipped reason's class, and the reason with the class boilerplate stripped.
/// One legacy reason states a cost gap without the "on cost" phrasing (bash on the SQL cases, "do not finish in a practical time"), so it is matched on that content; a reason with no exclusion prefix is a host setup gap, kept verbatim.
fn classify_v1_reason(reason: &str) -> (SkipKind, String) {
    if let Some(rest) = reason.strip_prefix("excluded on cost, not capability: ") {
        (SkipKind::Cost, rest.to_string())
    } else if let Some(rest) = reason.strip_prefix("excluded: ") {
        let kind = if rest.contains("do not finish in a practical time") {
            SkipKind::Cost
        } else {
            SkipKind::Capability
        };
        (kind, rest.to_string())
    } else {
        (SkipKind::Setup, reason.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_v1_reason_shapes_classify() {
        let (kind, reason) = classify_v1_reason("excluded on cost, not capability: too slow");
        assert!(matches!(kind, SkipKind::Cost));
        assert_eq!(reason, "too slow");

        let (kind, reason) = classify_v1_reason("excluded: cannot load the tag section");
        assert!(matches!(kind, SkipKind::Capability));
        assert_eq!(reason, "cannot load the tag section");

        let (kind, _) =
            classify_v1_reason("excluded: 100k SQL inserts do not finish in a practical time");
        assert!(matches!(kind, SkipKind::Cost));

        let (kind, reason) = classify_v1_reason("pypy3 not found on PATH");
        assert!(matches!(kind, SkipKind::Setup));
        assert_eq!(reason, "pypy3 not found on PATH");
    }
}
