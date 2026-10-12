# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
# Writes the generated STEP fixtures of this folder with FreeCAD (1.1,
# OpenCascade 7.9), into the current directory:
#
#     cd tests/data/step && freecadcmd -c "exec(open('generate.py').read())"
#
# The fixtures are frozen once committed; this script records how they were
# made. FreeCAD's command-line export writes no colours, so
# assembly_colours.step gets AP214 colour entities added afterwards, in the
# form OpenCascade writes them; plate_inch.step gets its length unit
# replaced by a conversion-based INCH.
import math
import re

import FreeCAD as App
import Import
import Part
from FreeCAD import Placement, Rotation, Vector

doc = App.newDocument("fixtures")


def feature(name, shape):
    obj = doc.addObject("Part::Feature", name)
    obj.Shape = shape
    return obj


def export(objs, name):
    doc.recompute()
    Import.export(objs, name)


# A 10 mm cube.
export([feature("Cube", Part.makeBox(10, 10, 10))], "cube.step")

# A 40 × 30 × 5 plate with a Ø10 through-hole at (20, 15).
plate = Part.makeBox(40, 30, 5).cut(Part.makeCylinder(5, 5, Vector(20, 15, 0)))
export([feature("Plate", plate)], "plate_hole.step")

# A Ø20 × 30 cylinder with a 2 mm fillet on its top edge (a torus).
cyl = Part.makeCylinder(10, 30)
top = [e for e in cyl.Edges if abs(e.BoundBox.ZMin - 30) < 1e-6][0]
export([feature("FilletedCylinder", cyl.makeFillet(2, [top]))], "cylinder_fillet.step")


# A loft through three periodic B-spline sections: B-spline surfaces.
def section(z, rx, ry, k):
    pts = []
    for i in range(12):
        t = i * 2 * math.pi / 12
        w = 1 + k * math.cos(3 * t)
        pts.append(Vector(rx * math.cos(t) * w, ry * math.sin(t) * w, z))
    curve = Part.BSplineCurve()
    curve.interpolate(pts, PeriodicFlag=True)
    return Part.Wire([curve.toShape()])


loft = Part.makeLoft(
    [section(0, 20, 12, 0.15), section(15, 14, 14, 0.0), section(30, 10, 18, 0.2)],
    True,
    False,
)
export([feature("SplineLoft", loft)], "bspline_loft.step")

# An assembly: one bracket placed twice (the second turned 90° about Z and
# moved 50 mm along X), and a base plate.
bracket = doc.addObject("App::Part", "Bracket")
body = feature(
    "BracketBody",
    Part.makeBox(20, 10, 4).cut(Part.makeCylinder(2, 4, Vector(10, 5, 0))),
)
bracket.addObject(body)
asm = doc.addObject("App::Part", "Assembly")
left = doc.addObject("App::Link", "BracketLeft")
left.LinkedObject = bracket
right = doc.addObject("App::Link", "BracketRight")
right.LinkedObject = bracket
right.Placement = Placement(Vector(50, 0, 0), Rotation(Vector(0, 0, 1), 90))
base = feature("Base", Part.makeBox(80, 30, 2, Vector(-10, -10, -2)))
asm.addObjects([left, right, base])
export([asm], "assembly.step")

# A 2 × 1.5 × 0.25 plate with a Ø0.5 hole, the numbers meant as inches.
inch = Part.makeBox(2, 1.5, 0.25).cut(Part.makeCylinder(0.25, 0.25, Vector(1, 0.75, 0)))
export([feature("InchPlate", inch)], "plate_inch.step")


# ---- Text edits on the written files ------------------------------------------

def entities(text):
    """The DATA section's entities as {id: text}."""
    data = text.split("DATA;", 1)[1].split("ENDSEC;", 1)[0]
    out = {}
    for raw in data.split(";"):
        m = re.match(r"\s*#(\d+)\s*=\s*(.*)", raw, re.S)
        if m:
            out[int(m.group(1))] = " ".join(m.group(2).split())
    return out


def add_entities(text, lines):
    return text.replace("ENDSEC;\nEND-ISO", "\n".join(lines) + "\nENDSEC;\nEND-ISO")


def style(n, target, rgb):
    """A surface colour on entity `target`: the ids n … n+7."""
    r, g, b = rgb
    return [
        f"#{n} = COLOUR_RGB('',{r},{g},{b});",
        f"#{n + 1} = FILL_AREA_STYLE_COLOUR('',#{n});",
        f"#{n + 2} = FILL_AREA_STYLE('',(#{n + 1}));",
        f"#{n + 3} = SURFACE_STYLE_FILL_AREA(#{n + 2});",
        f"#{n + 4} = SURFACE_SIDE_STYLE('',(#{n + 3}));",
        f"#{n + 5} = SURFACE_STYLE_USAGE(.BOTH.,#{n + 4});",
        f"#{n + 6} = PRESENTATION_STYLE_ASSIGNMENT((#{n + 5}));",
        f"#{n + 7} = STYLED_ITEM('color',(#{n + 6}),#{target});",
    ]


# The bracket red with its hole green, the base blue.
text = open("assembly.step").read()
ents = entities(text)
solids = sorted(i for i, e in ents.items() if e.startswith("MANIFOLD_SOLID_BREP"))
cylinders = {i for i, e in ents.items() if e.startswith("CYLINDRICAL_SURFACE")}
hole = min(
    i
    for i, e in ents.items()
    if e.startswith("ADVANCED_FACE") and int(re.findall(r"#(\d+)", e)[-1]) in cylinders
)


def context(solid):
    rep = next(
        e
        for e in ents.values()
        if e.startswith("ADVANCED_BREP_SHAPE_REPRESENTATION") and re.search(rf"#{solid}[,)]", e)
    )
    return int(re.findall(r"#(\d+)", rep)[-1])


n = max(ents) + 1
lines = style(n, solids[0], (0.8, 0.1, 0.1))
lines += style(n + 8, hole, (0.1, 0.7, 0.2))
lines += style(n + 16, solids[1], (0.1, 0.2, 0.8))
lines += [
    f"#{n + 24} = MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION("
    f"'',(#{n + 7},#{n + 15}),#{context(solids[0])});",
    f"#{n + 25} = MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION("
    f"'',(#{n + 23}),#{context(solids[1])});",
]
open("assembly_colours.step", "w").write(add_entities(text, lines))

# The plate's length unit becomes the inch.
text = open("plate_inch.step").read()
ents = entities(text)
unit = next(i for i, e in ents.items() if "LENGTH_UNIT()" in e and "SI_UNIT(.MILLI.,.METRE.)" in e)
n = max(ents) + 1
text = re.sub(
    rf"#{unit}\s*=\s*\(\s*LENGTH_UNIT\(\)\s*NAMED_UNIT\(\*\)\s*SI_UNIT\(\.MILLI\.,\.METRE\.\)\s*\);",
    f"#{unit} = ( CONVERSION_BASED_UNIT('INCH',#{n}) LENGTH_UNIT() NAMED_UNIT(#{n + 1}) );",
    text,
)
text = add_entities(
    text,
    [
        f"#{n} = LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#{n + 2});",
        f"#{n + 1} = DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);",
        f"#{n + 2} = ( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );",
    ],
)
open("plate_inch.step", "w").write(text)
