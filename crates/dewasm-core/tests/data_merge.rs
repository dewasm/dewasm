//! The adjacent active-data-segment merging pass: assert the `module.datas` shape the pass produces from small wat inputs.
//! The cases cover merged runs, zero-filled gaps, the gap threshold, and every bail condition (bulk-memory ops, overlapping/descending offsets, `global.get` offsets).

use dewasm_core::build_module;
use dewasm_core::ir::{DataSegment, Expr, Module};

fn module(wat: &str) -> Module {
    let bytes = wat::parse_str(wat).expect("wat parses");
    build_module(&bytes).expect("module builds")
}

/// The constant offset of an active segment, or `None` for passive / non-const.
fn const_offset(seg: &DataSegment) -> Option<u64> {
    match &seg.offset {
        Some(Expr::I32Const(off)) => Some(u64::from(*off)),
        _ => None,
    }
}

#[test]
fn adjacent_segments_merge_with_zero_filled_gap() {
    // Two active segments at 0 (`ab`) and 4 (`cd`) with a 2-byte hole: they sit within the 64-byte threshold, so they collapse into one blob at offset 0 whose gap is zero-filled.
    let m = module(
        r#"(module (memory 1)
            (data (i32.const 0) "ab")
            (data (i32.const 4) "cd"))"#,
    );
    assert_eq!(m.datas.len(), 1, "the two segments merge into one");
    assert_eq!(const_offset(&m.datas[0]), Some(0));
    assert_eq!(m.datas[0].data, b"ab\x00\x00cd");
}

#[test]
fn touching_segments_merge_without_a_gap() {
    let m = module(
        r#"(module (memory 1)
            (data (i32.const 0) "ab")
            (data (i32.const 2) "cd"))"#,
    );
    assert_eq!(m.datas.len(), 1);
    assert_eq!(const_offset(&m.datas[0]), Some(0));
    assert_eq!(m.datas[0].data, b"abcd");
}

#[test]
fn gap_at_or_above_threshold_keeps_segments_separate() {
    // The second segment starts 64 bytes past the end of the first (exactly the threshold, which is exclusive), so no merge happens.
    let m = module(
        r#"(module (memory 1)
            (data (i32.const 0) "ab")
            (data (i32.const 66) "cd"))"#,
    );
    assert_eq!(m.datas.len(), 2, "a >= 64 gap is not bridged");
    assert_eq!(const_offset(&m.datas[0]), Some(0));
    assert_eq!(m.datas[0].data, b"ab");
    assert_eq!(const_offset(&m.datas[1]), Some(66));
    assert_eq!(m.datas[1].data, b"cd");
}

#[test]
fn global_get_offset_bails_the_whole_pass() {
    // A `global.get` offset writes to a runtime-unknown address, so its presence anywhere bails the pass: nothing merges, everything passes through unchanged.
    let m = module(
        r#"(module
            (import "env" "base" (global $base i32))
            (memory 1)
            (data (i32.const 0) "ab")
            (data (global.get $base) "cd")
            (data (i32.const 4) "ef"))"#,
    );
    assert_eq!(
        m.datas.len(),
        3,
        "a global.get offset keeps all three apart"
    );
    assert_eq!(const_offset(&m.datas[0]), Some(0));
    assert_eq!(m.datas[0].data, b"ab");
    assert!(
        matches!(m.datas[1].offset, Some(Expr::GlobalGet(_))),
        "the global.get segment passes through unchanged: {:?}",
        m.datas[1].offset
    );
    assert_eq!(m.datas[1].data, b"cd");
    assert_eq!(const_offset(&m.datas[2]), Some(4));
    assert_eq!(m.datas[2].data, b"ef");
}

#[test]
fn global_get_before_mergeable_consts_bails_the_whole_pass() {
    // Issue #28 regression.
    // With `base = 4` at instantiation, "XX" lands at 4..6, inside the 2..8 gap that merging the two const segments would zero-fill.
    // The merged blob is emitted *after* the global.get segment, so its zeros would clobber "XX".
    // The pass must therefore leave every segment untouched, keeping the final memory image identical.
    let m = module(
        r#"(module
            (import "env" "base" (global $base i32))
            (memory 1)
            (data (global.get $base) "XX")
            (data (i32.const 0) "ab")
            (data (i32.const 8) "cd"))"#,
    );
    assert_eq!(
        m.datas.len(),
        3,
        "no merge with a global.get segment present"
    );
    assert!(
        matches!(m.datas[0].offset, Some(Expr::GlobalGet(_))),
        "the global.get segment passes through unchanged: {:?}",
        m.datas[0].offset
    );
    assert_eq!(m.datas[0].data, b"XX");
    assert_eq!(const_offset(&m.datas[1]), Some(0));
    assert_eq!(m.datas[1].data, b"ab", "no zero fill appended");
    assert_eq!(const_offset(&m.datas[2]), Some(8));
    assert_eq!(m.datas[2].data, b"cd");
}

#[test]
fn descending_offsets_bail_the_whole_pass() {
    // The second segment starts before the first ends: not globally ascending, so the pass leaves every segment untouched (no merge anywhere).
    let m = module(
        r#"(module (memory 1)
            (data (i32.const 4) "ab")
            (data (i32.const 0) "cd")
            (data (i32.const 6) "ef"))"#,
    );
    assert_eq!(m.datas.len(), 3, "a descending offset bails the pass");
    assert_eq!(const_offset(&m.datas[0]), Some(4));
    assert_eq!(const_offset(&m.datas[1]), Some(0));
    assert_eq!(const_offset(&m.datas[2]), Some(6));
}

#[test]
fn overlapping_offsets_bail_the_whole_pass() {
    // The second segment (start 1) overlaps the first (0..2): bail entirely.
    let m = module(
        r#"(module (memory 1)
            (data (i32.const 0) "ab")
            (data (i32.const 1) "cd"))"#,
    );
    assert_eq!(m.datas.len(), 2, "overlap bails the pass");
    assert_eq!(m.datas[0].data, b"ab");
    assert_eq!(m.datas[1].data, b"cd");
}

#[test]
fn memory_init_or_data_drop_bails_the_whole_pass() {
    // Bulk-memory ops reference segments by index; merging would renumber them, so their presence bails the pass even for the mergeable active segments. (Mirrors the shape of the CLI data_file fixture.)
    let m = module(
        r#"(module (memory 1)
            (data (i32.const 0) "ab")
            (data (i32.const 4) "cd")
            (data "passive")
            (func
              (memory.init 2 (i32.const 32) (i32.const 0) (i32.const 7))
              (data.drop 2)))"#,
    );
    assert_eq!(m.datas.len(), 3, "bulk-memory presence bails the pass");
    assert_eq!(const_offset(&m.datas[0]), Some(0));
    assert_eq!(m.datas[0].data, b"ab");
    assert_eq!(const_offset(&m.datas[1]), Some(4));
    assert_eq!(m.datas[1].data, b"cd");
    assert_eq!(const_offset(&m.datas[2]), None, "passive stays passive");
    assert_eq!(m.datas[2].data, b"passive");
}

#[test]
fn passive_interleaved_between_actives_lets_them_merge() {
    // A passive segment carries no standalone memory effect (no `memory.init` here), so it does not close the active run: the actives on either side still merge, and the passive survives in the output.
    let m = module(
        r#"(module (memory 1)
            (data (i32.const 0) "ab")
            (data "passive")
            (data (i32.const 4) "cd"))"#,
    );
    assert_eq!(m.datas.len(), 2, "the two actives merge around the passive");

    let active: Vec<_> = m.datas.iter().filter(|s| s.offset.is_some()).collect();
    let passive: Vec<_> = m.datas.iter().filter(|s| s.offset.is_none()).collect();
    assert_eq!(active.len(), 1, "exactly one merged active segment");
    assert_eq!(const_offset(active[0]), Some(0));
    assert_eq!(active[0].data, b"ab\x00\x00cd");
    assert_eq!(passive.len(), 1, "the passive segment is preserved");
    assert_eq!(passive[0].data, b"passive");
}
