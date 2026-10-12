-- SPDX-License-Identifier: GPL-3.0-or-later
-- SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
--
-- Spur gears with involute teeth (20° pressure angle), built as one group
-- centred on the origin. Lengths are in the document's unit.

local profile = require "profile"

-- The faces of the context being edited, by handle text.
local function face_set(doc)
  local set = {}
  for _, f in ipairs(doc:faces()) do
    set[tostring(f)] = true
  end
  return set
end

local function new_faces(doc, before)
  local out = {}
  for _, f in ipairs(doc:faces()) do
    if not before[tostring(f)] then
      out[#out + 1] = f
    end
  end
  return out
end

local function spur_gear(doc, p)
  local outline = profile.spur(p.teeth, p.module)
  local root = profile.root_radius(p.teeth, p.module)
  local holes = {}
  if p.bore > 0 then
    if p.bore / 2 >= root - p.module then
      error("the bore leaves no rim under the teeth", 0)
    end
    holes[1] = profile.circle(p.bore / 2, 48)
  end
  local points = {}
  for i, xy in ipairs(outline) do
    points[i] = xtrude.vec(xy[1], xy[2], 0)
  end
  local hole_points = {}
  for i, hole in ipairs(holes) do
    hole_points[i] = {}
    for j, xy in ipairs(hole) do
      hole_points[i][j] = xtrude.vec(xy[1], xy[2], 0)
    end
  end
  local before = face_set(doc)
  local face = doc:add_face(points, { holes = hole_points })
  local up = face:normal().z
  doc:push_pull(face, p.width * up)
  local name = string.format("Spur gear %d×%g", p.teeth, p.module)
  local gear = doc:make_group(new_faces(doc, before), name)
  doc:select({ gear })
end

return {
  id = "gears",
  name = "Gears",
  version = "1.0.0",
  api = 1,
  commands = {
    {
      id = "spur",
      label = "Spur gear…",
      params = {
        { key = "teeth", label = "Teeth", type = "int", default = 20, min = 6, max = 200 },
        { key = "module", label = "Module", type = "length", default = 1.0, min = 0.2 },
        { key = "width", label = "Width", type = "length", default = 5.0, min = 0.5 },
        { key = "bore", label = "Bore diameter", type = "length", default = 5.0, min = 0 },
      },
      run = spur_gear,
    },
  },
}
