// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
// The STEP reader on OpenCascade, called from Rust through `occt.rs`.
#pragma once

#include <memory>

#include <TDocStd_Document.hxx>

#include "rust/cxx.h"

namespace xtrude_step {

struct Doc;

// A read STEP file: the XCAF document the reader filled.
class Reader {
public:
  Handle(TDocStd_Document) doc;
};

std::unique_ptr<Reader> open_step(rust::Str path);
Doc tessellate(Reader &reader, double chord_mm, double angle_rad);

} // namespace xtrude_step
