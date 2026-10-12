// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
// The STEP reader on OpenCascade: STEPCAFControl reads the file into an
// XCAF document, which keeps part names, colours, the assembly tree and
// the placements; BRepMesh meshes each part once, however often it is
// placed. OpenCascade throws Standard_Failure, which is not a
// std::exception, so every entry point turns it into one for cxx.
#include "xtrude-step/src/reader.hpp"
#include "xtrude-step/src/occt.rs.h"

#include <array>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

#include <BRepAdaptor_Surface.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRep_Tool.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Poly_Triangulation.hxx>
#include <Quantity_Color.hxx>
#include <STEPCAFControl_Reader.hxx>
#include <Standard_Failure.hxx>
#include <TCollection_AsciiString.hxx>
#include <TDF_Label.hxx>
#include <TDF_LabelSequence.hxx>
#include <TDataStd_Name.hxx>
#include <TDocStd_Document.hxx>
#include <TopExp_Explorer.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Shape.hxx>
#include <UnitsMethods_LengthUnit.hxx>
#include <XCAFApp_Application.hxx>
#include <XCAFDoc_ColorTool.hxx>
#include <XCAFDoc_DocumentTool.hxx>
#include <XCAFDoc_ShapeTool.hxx>
#include <gp_Trsf.hxx>

namespace xtrude_step {

namespace {

// Assemblies nest; a file whose tree loops back on itself must not recurse
// forever.
constexpr int MAX_DEPTH = 64;

std::runtime_error failure(const Standard_Failure &e) {
  const char *msg = e.GetMessageString();
  return std::runtime_error(std::string("OpenCascade: ") +
                            (msg && *msg ? msg : e.DynamicType()->Name()));
}

rust::String label_name(const TDF_Label &label) {
  Handle(TDataStd_Name) name;
  if (!label.FindAttribute(TDataStd_Name::GetID(), name)) {
    return rust::String();
  }
  // Converted to UTF-8 (no replacement character given).
  TCollection_AsciiString utf8(name->Get());
  return rust::String::lossy(utf8.ToCString());
}

// The colour in the file's own sRGB values (OpenCascade keeps linear RGB).
bool color_of(const Quantity_Color &c, std::array<double, 3> &out) {
  double r, g, b;
  c.Values(r, g, b, Quantity_TOC_sRGB);
  out = {r, g, b};
  return true;
}

class Walker {
public:
  Walker(const Handle(TDocStd_Document) & doc, double chord, double angle)
      : shapes_(XCAFDoc_DocumentTool::ShapeTool(doc->Main())),
        colors_(XCAFDoc_DocumentTool::ColorTool(doc->Main())), chord_(chord),
        angle_(angle) {}

  Doc run() {
    TDF_LabelSequence free;
    shapes_->GetFreeShapes(free);
    for (int i = 1; i <= free.Length(); i++) {
      Instance root;
      root.part = part(free(i), 0);
      root.name = label_name(free(i));
      root.matrix = identity();
      doc_.roots.push_back(std::move(root));
    }
    return std::move(doc_);
  }

private:
  static std::array<double, 12> identity() {
    return {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0};
  }

  static std::array<double, 12> matrix(const TopLoc_Location &loc) {
    const gp_Trsf t = loc.Transformation();
    std::array<double, 12> m{};
    for (int r = 1; r <= 3; r++) {
      for (int c = 1; c <= 4; c++) {
        m[(r - 1) * 4 + (c - 1)] = t.Value(r, c);
      }
    }
    return m;
  }

  bool label_color(const TDF_Label &label, std::array<double, 3> &out) {
    Quantity_Color c;
    if (colors_->GetColor(label, XCAFDoc_ColorSurf, c) ||
        colors_->GetColor(label, XCAFDoc_ColorGen, c)) {
      return color_of(c, out);
    }
    return false;
  }

  bool shape_color(const TopoDS_Shape &shape, std::array<double, 3> &out) {
    Quantity_Color c;
    if (colors_->GetColor(shape, XCAFDoc_ColorSurf, c) ||
        colors_->GetColor(shape, XCAFDoc_ColorGen, c)) {
      return color_of(c, out);
    }
    return false;
  }

  // The index of the part a label stands for, listed on first use.
  uint32_t part(const TDF_Label &label, int depth) {
    for (const auto &seen : seen_) {
      if (seen.first.IsEqual(label)) {
        return seen.second;
      }
    }
    if (depth > MAX_DEPTH) {
      throw std::runtime_error("the assembly tree is nested too deeply");
    }
    Part p;
    p.name = label_name(label);
    p.has_color = label_color(label, p.color);
    p.closed = false;
    p.failed_faces = 0;
    if (shapes_->IsAssembly(label)) {
      TDF_LabelSequence components;
      shapes_->GetComponents(label, components);
      for (int i = 1; i <= components.Length(); i++) {
        const TDF_Label &component = components(i);
        TDF_Label referred;
        if (!shapes_->GetReferredShape(component, referred)) {
          continue;
        }
        Instance child;
        child.part = part(referred, depth + 1);
        child.name = label_name(component);
        if (child.name.empty()) {
          child.name = label_name(referred);
        }
        child.matrix = matrix(shapes_->GetLocation(component));
        p.children.push_back(std::move(child));
      }
    } else {
      mesh(shapes_->GetShape(label), p);
    }
    const auto index = static_cast<uint32_t>(doc_.parts.size());
    doc_.parts.push_back(std::move(p));
    seen_.emplace_back(label, index);
    return index;
  }

  void mesh(const TopoDS_Shape &shape, Part &p) {
    if (shape.IsNull()) {
      return;
    }
    BRepMesh_IncrementalMesh(shape, chord_, false, angle_, true);
    for (TopExp_Explorer ex(shape, TopAbs_FACE); ex.More(); ex.Next()) {
      const TopoDS_Face &face = TopoDS::Face(ex.Current());
      TopLoc_Location loc;
      const Handle(Poly_Triangulation) &tri =
          BRep_Tool::Triangulation(face, loc);
      if (tri.IsNull() || tri->NbTriangles() == 0) {
        p.failed_faces++;
        continue;
      }
      Face f;
      const gp_Trsf trsf = loc.Transformation();
      f.positions.reserve(3 * tri->NbNodes());
      for (int i = 1; i <= tri->NbNodes(); i++) {
        const gp_Pnt n = tri->Node(i).Transformed(trsf);
        f.positions.push_back(n.X());
        f.positions.push_back(n.Y());
        f.positions.push_back(n.Z());
      }
      const bool reversed = face.Orientation() == TopAbs_REVERSED;
      f.triangles.reserve(3 * tri->NbTriangles());
      for (int i = 1; i <= tri->NbTriangles(); i++) {
        int a, b, c;
        tri->Triangle(i).Get(a, b, c);
        if (reversed) {
          std::swap(b, c);
        }
        f.triangles.push_back(a - 1);
        f.triangles.push_back(b - 1);
        f.triangles.push_back(c - 1);
      }
      f.planar = BRepAdaptor_Surface(face, false).GetType() == GeomAbs_Plane;
      f.has_color = shape_color(face, f.color);
      p.faces.push_back(std::move(f));
    }
    // Closed: every shell is closed and no face is left outside a shell.
    int shells = 0;
    bool closed = true;
    for (TopExp_Explorer ex(shape, TopAbs_SHELL); ex.More(); ex.Next()) {
      shells++;
      closed = closed && BRep_Tool::IsClosed(ex.Current());
    }
    TopExp_Explorer loose(shape, TopAbs_FACE, TopAbs_SHELL);
    p.closed = shells > 0 && closed && !loose.More();
  }

  Handle(XCAFDoc_ShapeTool) shapes_;
  Handle(XCAFDoc_ColorTool) colors_;
  double chord_;
  double angle_;
  Doc doc_;
  std::vector<std::pair<TDF_Label, uint32_t>> seen_;
};

} // namespace

std::unique_ptr<Reader> open_step(rust::Str path) {
  try {
    // OpenCascade reports to stdout by default, which carries the result.
    Message::DefaultMessenger()->ChangePrinters().Clear();
    auto reader = std::make_unique<Reader>();
    Handle(XCAFApp_Application) app = XCAFApp_Application::GetApplication();
    app->NewDocument("MDTV-XCAF", reader->doc);
    // The reader converts the file's length unit to the document's.
    XCAFDoc_DocumentTool::SetLengthUnit(reader->doc, 1.0,
                                        UnitsMethods_LengthUnit_Millimeter);
    // Unnamed shapes stay unnamed (no "=>[0:1:1:2]" or "SOLID"), so the
    // import falls back to the part's own name.
    XCAFDoc_ShapeTool::SetAutoNaming(false);
    STEPCAFControl_Reader step;
    step.SetColorMode(true);
    step.SetNameMode(true);
    step.SetLayerMode(false);
    step.SetPropsMode(false);
    const std::string file(path.data(), path.size());
    if (step.ReadFile(file.c_str()) != IFSelect_RetDone) {
      throw std::runtime_error("not a readable STEP file");
    }
    if (!step.Transfer(reader->doc)) {
      throw std::runtime_error("the file holds no shape that can be read");
    }
    return reader;
  } catch (const Standard_Failure &e) {
    throw failure(e);
  }
}

Doc tessellate(Reader &reader, double chord_mm, double angle_rad) {
  try {
    return Walker(reader.doc, chord_mm, angle_rad).run();
  } catch (const Standard_Failure &e) {
    throw failure(e);
  }
}

} // namespace xtrude_step
