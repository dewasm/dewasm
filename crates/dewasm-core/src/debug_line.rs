//! Parse a wasm module's `.debug_line` DWARF program into an address-to-source-position lookup, for the opt-in `--dwarf-line` back-mapping. gimli lives here and here only; backends never see it.

use std::collections::HashMap;

use anyhow::{Context, Result};
use gimli::{EndianSlice, LittleEndian};

use crate::ir::SourcePos;

/// The wasm DWARF code-address convention.
///
/// In a linked wasm binary produced by clang/lld (what `zig cc` emits), a DWARF code address is the byte offset of the instruction **relative to the start of the code section's contents**, not an absolute module-file offset. But `wasmparser`'s `OperatorsReader` reports operator positions as absolute module-file offsets. So to look a resolved operator up in the line table we subtract the code section's content start — that is the address base.
fn address_base(code_section_start: u64) -> u64 {
    code_section_start
}

/// One decoded line-program row: a code address and the source position that begins there, or `None` for an `end_sequence` row (an exclusive upper bound with no mapping).
struct Row {
    address: u64,
    pos: Option<SourcePos>,
}

/// A flattened, address-sorted DWARF line table with interned file paths.
pub struct LineTable {
    rows: Vec<Row>,
    /// Value subtracted from a module file offset to reach DWARF address space.
    base: u64,
    /// Interned source file paths; `SourcePos::file` indexes this.
    pub files: Vec<String>,
}

impl LineTable {
    /// Parse the collected `.debug_*` section blobs (keyed by section name, e.g. `.debug_line`). `code_section_start` is the module file offset of the code section's contents, used only for address-base calibration. Returns `None` when there is no line program to speak of.
    pub fn parse(
        sections: &HashMap<String, Vec<u8>>,
        code_section_start: u64,
    ) -> Result<Option<LineTable>> {
        if !sections.contains_key(".debug_line") {
            return Ok(None);
        }

        let empty: &[u8] = &[];
        let dwarf = gimli::Dwarf::load(
            |id| -> Result<EndianSlice<'_, LittleEndian>, gimli::Error> {
                let data = sections.get(id.name()).map(Vec::as_slice).unwrap_or(empty);
                Ok(EndianSlice::new(data, LittleEndian))
            },
        )
        .context("loading DWARF sections")?;

        let mut files: Vec<String> = Vec::new();
        let mut files_map: HashMap<String, u32> = HashMap::new();
        let mut rows: Vec<Row> = Vec::new();

        let mut units = dwarf.units();
        while let Some(unit_header) = units.next().context("iterating DWARF units")? {
            let unit = dwarf.unit(unit_header).context("parsing a DWARF unit")?;
            let Some(program) = unit.line_program.clone() else {
                continue;
            };
            let mut state = program.rows();
            while let Some((header, row)) = state.next_row().context("reading a line-table row")? {
                // An `end_sequence` boundary, or a row DWARF marks as having no source line (line 0 — a compiler-generated prologue or epilogue), maps to nothing: record it as a gap so a lookup landing there yields no position rather than a bogus `line 0` directive (which Go's `//line` rejects).
                if row.end_sequence() || row.line().is_none() {
                    rows.push(Row {
                        address: row.address(),
                        pos: None,
                    });
                    continue;
                }
                let Some(name) = resolve_file(&dwarf, &unit, header, row.file_index()) else {
                    continue;
                };
                let file = intern(&mut files, &mut files_map, name);
                // `line().is_none()` (line 0) is handled as a gap above, so a real row always has a nonzero line here.
                let line = row.line().map_or(0, |l| l.get() as u32);
                let col = match row.column() {
                    gimli::ColumnType::LeftEdge => 0,
                    gimli::ColumnType::Column(c) => c.get() as u32,
                };
                rows.push(Row {
                    address: row.address(),
                    pos: Some(SourcePos { file, line, col }),
                });
            }
        }

        // Rows come per-sequence (already ascending) but sequences and units are not globally ordered, so sort. At an equal address, order a real row after an `end_sequence` boundary so a lookup landing exactly on a sequence start resolves the mapped row, not the preceding gap.
        rows.sort_by_key(|r| (r.address, r.pos.is_some()));

        Ok(Some(LineTable {
            rows,
            base: address_base(code_section_start),
            files,
        }))
    }

    /// Resolve a module file offset (an operator's `original_position`) to the source position of the greatest line-table row whose address is `<=` it, honoring `end_sequence` rows (which map to `None`).
    pub fn lookup(&self, module_offset: u64) -> Option<SourcePos> {
        let addr = module_offset.checked_sub(self.base)?;
        let i = self.rows.partition_point(|r| r.address <= addr);
        if i == 0 {
            return None;
        }
        self.rows[i - 1].pos
    }
}

/// Intern a file path, returning its index in `files`.
fn intern(files: &mut Vec<String>, map: &mut HashMap<String, u32>, name: String) -> u32 {
    if let Some(&id) = map.get(&name) {
        return id;
    }
    let id = files.len() as u32;
    map.insert(name.clone(), id);
    files.push(name);
    id
}

/// Build a file path string from a line-program file entry: its directory entry joined with the file name (an absolute file name stands alone).
fn resolve_file(
    dwarf: &gimli::Dwarf<EndianSlice<'_, LittleEndian>>,
    unit: &gimli::Unit<EndianSlice<'_, LittleEndian>>,
    header: &gimli::LineProgramHeader<EndianSlice<'_, LittleEndian>>,
    file_index: u64,
) -> Option<String> {
    let file = header.file(file_index)?;
    let name = dwarf.attr_string(unit, file.path_name()).ok()?;
    let name = name.to_string_lossy();
    if name.starts_with('/') {
        return Some(name.into_owned());
    }
    let mut path = String::new();
    if let Some(dir) = file.directory(header) {
        if let Ok(dir) = dwarf.attr_string(unit, dir) {
            let dir = dir.to_string_lossy();
            if !dir.is_empty() {
                path.push_str(&dir);
                if !path.ends_with('/') {
                    path.push('/');
                }
            }
        }
    }
    path.push_str(&name);
    Some(path)
}
