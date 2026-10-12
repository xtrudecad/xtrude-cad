// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The integration tests of `xtrude-render`, as one test binary: one binary
//! per test file made the test build link the dependencies hundreds of
//! times.

mod test_glinfo;
mod test_gpu_frame;
mod test_guides_behind_geometry;
