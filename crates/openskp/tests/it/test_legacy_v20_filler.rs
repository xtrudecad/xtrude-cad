// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Derived from OpenSKP `packages/python/tests/test_legacy_v20_filler.py`: the
//! SketchUp 2020 filler probe on synthetic records.

use openskp::legacy::{retry_count_after_v20_filler, Cursor};

/// A filler record, then `count`, then a class-record header.
fn filler(count: u32, pad: usize) -> Vec<u8> {
    let mut b = vec![0xff, 0xfe, 0xff, 0x00];
    b.extend(std::iter::repeat_n(0u8, pad));
    b.extend(count.to_le_bytes());
    b.extend([0xff, 0xff, 0x0b, 0x00]);
    b
}

fn retry(data: &[u8], limit: u32) -> Option<u32> {
    retry_count_after_v20_filler(&mut Cursor::new(data, 0), 0, limit, None)
}

#[test]
fn test_reads_the_counts_and_paddings_seen_in_real_v20_files() {
    assert_eq!(retry(&filler(20, 9), 5_000_000), Some(20));
    assert_eq!(retry(&filler(5425, 13), 5_000_000), Some(5425));
}

#[test]
fn test_reads_a_count_that_is_an_exact_multiple_of_256() {
    for count in [256, 512, 1024, 65536, 16_777_216 / 16] {
        for pad in [9, 13] {
            assert_eq!(
                retry(&filler(count, pad), 5_000_000),
                Some(count),
                "count={count} pad={pad}"
            );
        }
    }
}

#[test]
fn test_ignores_a_non_empty_string_record() {
    let data = [0xff, 0xfe, 0xff, 0x05, 0, 0, 0, 0, 0, 0, 0, 0, 20, 0, 0, 0];
    assert_eq!(retry(&data, 5_000_000), None);
}

#[test]
fn test_returns_none_when_there_is_no_marker_ahead() {
    assert_eq!(retry(&[0u8; 32], 5_000_000), None);
}

#[test]
fn test_does_not_run_past_the_end_of_the_buffer() {
    assert_eq!(retry(&[0xff, 0xfe, 0xff, 0x00, 0, 0], 5_000_000), None);
}

#[test]
fn test_rejects_a_candidate_above_the_callers_own_limit() {
    let count = 200_000;
    assert_eq!(retry(&filler(count, 9), 5_000_000), Some(count));
    assert_eq!(retry(&filler(count, 9), 100_000), None);
}
