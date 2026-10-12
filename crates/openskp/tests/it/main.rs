// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2024-2026 Ahsan Mehmood
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The integration tests of `openskp`, as one test binary: one binary
//! per test file made the test build link the dependencies hundreds of
//! times.

mod common;

mod test_container;
mod test_edge_surfacing;
mod test_golden;
mod test_legacy_pre2014_instance_guid;
mod test_legacy_single_material;
mod test_legacy_v20_filler;
mod test_loose_edge_runs_public_api;
mod test_openskp_compat;
mod test_pages_dimensions;
mod test_parser;
mod test_robustness;
