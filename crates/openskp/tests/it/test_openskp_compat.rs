// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Derived from tests/test_openskp_compat.py: classic dimensions anchored
//! inside groups (upstream iamahsanmehmood/openskp#384). The Python code
//! patched the pinned reader at run time; here the fix is the reader's
//! own behaviour (`legacy::connection_paths`), so the third test, on the
//! patch mechanism, has no counterpart.

use openskp::legacy::{connection_paths, Archive, Cursor};

/// A big-tag (escaped) reference to `slot`.
fn big(slot: u32) -> Vec<u8> {
    [
        0x7FFFu16.to_le_bytes().to_vec(),
        slot.to_le_bytes().to_vec(),
    ]
    .concat()
}

#[test]
fn test_the_patch_is_installed_and_reads_instance_paths() {
    let data = [
        big(0x0F69CA),
        2u32.to_le_bytes().to_vec(),
        big(0x166157),
        big(0x165BBC),
        0u32.to_le_bytes().to_vec(),
        b"TAIL".to_vec(),
    ]
    .concat();
    let mut ar = Archive::new(&[], 17);
    let mut r = Cursor::new(&data, 0);
    let (extra, p1, p2) = connection_paths(&mut ar, &mut r).unwrap();
    assert_eq!(extra, Some(0x0F69CA));
    assert_eq!(p1, [Some(0x166157), Some(0x165BBC)]);
    assert!(p2.is_empty());
    assert_eq!(&data[r.pos..], b"TAIL");
}

/// Loose-geometry dimensions keep their exact length: a null ref and two
/// empty lists are 10 bytes.
#[test]
fn test_an_empty_path_is_the_old_fixed_layout() {
    let data = [vec![0u8; 10], b"TAIL".to_vec()].concat();
    let mut ar = Archive::new(&[], 17);
    let mut r = Cursor::new(&data, 0);
    let (extra, p1, p2) = connection_paths(&mut ar, &mut r).unwrap();
    assert_eq!(extra, None);
    assert!(p1.is_empty() && p2.is_empty());
    assert_eq!(r.pos, 10);
}
