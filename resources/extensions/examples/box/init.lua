-- SPDX-License-Identifier: GPL-3.0-or-later
-- SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
--
-- A parametric box with a lid that fits it: the lid's lip sits inside the
-- box's walls with `clearance` all around. Box and lid are built side by
-- side, each as its own group. Lengths are in the document's unit.

-- The faces of the context being edited, by handle text.
local function face_set(doc)
  local set = {}
  for _, f in ipairs(doc:faces()) do
    set[tostring(f)] = true
  end
  return set
end

-- The faces built since `before` was taken.
local function new_faces(doc, before)
  local out = {}
  for _, f in ipairs(doc:faces()) do
    if not before[tostring(f)] then
      out[#out + 1] = f
    end
  end
  return out
end

-- A rectangle on the plane z, from (x0, y0) to (x1, y1).
local function rect(x0, y0, x1, y1, z)
  return {
    xtrude.vec(x0, y0, z), xtrude.vec(x1, y0, z),
    xtrude.vec(x1, y1, z), xtrude.vec(x0, y1, z),
  }
end

-- Pushes `face` until its plane is at height `z`: push/pull goes along the
-- face's normal, which may point up or down.
local function push_to(doc, face, z)
  local here = face:vertices()[1].z
  local up = face:normal().z
  return doc:push_pull(face, (z - here) * up)
end

local function build_box(doc, p)
  local w, d, h, t = p.width, p.depth, p.height, p.wall
  local before = face_set(doc)
  local base = doc:add_face(rect(0, 0, w, d, 0))
  local top = push_to(doc, base, h)
  -- the hollow: a rectangle inside the top face, pushed down to the floor
  local opening = doc:add_face(rect(t, t, w - t, d - t, h))
  push_to(doc, opening, t)
  return doc:make_group(new_faces(doc, before), "Box"), top
end

local function build_lid(doc, p)
  local w, d, t, c = p.width, p.depth, p.wall, p.clearance
  local x = w + 10 -- beside the box
  local before = face_set(doc)
  local plate = doc:add_face(rect(x, 0, x + w, d, 0))
  push_to(doc, plate, t)
  -- the lip: inside the walls, `c` away from them
  local inset = t + c
  local lip = doc:add_face(rect(x + inset, inset, x + w - inset, d - inset, t))
  push_to(doc, lip, t + p.lip)
  return doc:make_group(new_faces(doc, before), "Lid")
end

return {
  id = "box",
  name = "Box with lid",
  version = "1.0.0",
  api = 1,
  commands = {
    {
      id = "box",
      label = "Box with lid…",
      params = {
        { key = "width", label = "Width", type = "length", default = 60, min = 5 },
        { key = "depth", label = "Depth", type = "length", default = 40, min = 5 },
        { key = "height", label = "Height", type = "length", default = 25, min = 2 },
        { key = "wall", label = "Wall", type = "length", default = 2, min = 0.4 },
        { key = "lip", label = "Lid lip", type = "length", default = 3, min = 0.5 },
        { key = "clearance", label = "Clearance", type = "length", default = 0.2, min = 0 },
        { key = "lid", label = "Make the lid", type = "bool", default = true },
      },
      run = function(doc, p)
        if 2 * (p.wall + p.clearance) >= math.min(p.width, p.depth) then
          error("the walls leave no room inside the box", 0)
        end
        if p.wall >= p.height then
          error("the wall is thicker than the box is high", 0)
        end
        local box = build_box(doc, p)
        local made = { box }
        if p.lid then
          made[#made + 1] = build_lid(doc, p)
        end
        doc:select(made)
      end,
    },
  },
}
