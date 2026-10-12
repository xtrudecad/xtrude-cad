-- SPDX-License-Identifier: GPL-3.0-or-later
-- SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
--
-- Plane outlines for the gears: lists of {x, y} points, counter-clockwise.

local M = {}

local PRESSURE = math.rad(20)
-- points per flank, from the root (or base) circle to the tip
local FLANK_STEPS = 5

local function involute_angle(a)
  return math.tan(a) - a
end

function M.root_radius(teeth, module)
  return module * teeth / 2 - 1.25 * module
end

-- A circle of `n` points.
function M.circle(r, n)
  local out = {}
  for i = 0, n - 1 do
    local a = 2 * math.pi * i / n
    out[#out + 1] = { r * math.cos(a), r * math.sin(a) }
  end
  return out
end

-- The outline of a spur gear: involute flanks from the base circle (or the
-- root circle, if larger) to the tip circle, radial below the base circle.
function M.spur(teeth, module)
  local pitch = module * teeth / 2
  local tip = pitch + module
  local root = M.root_radius(teeth, module)
  local base = pitch * math.cos(PRESSURE)
  -- half a tooth's angular thickness on the base circle
  local half = math.pi / (2 * teeth) + involute_angle(PRESSURE)
  local start = math.max(base, root)
  local function flank_angle(r)
    if r <= base then
      return 0
    end
    return involute_angle(math.acos(base / r))
  end
  local function at(r, a)
    return { r * math.cos(a), r * math.sin(a) }
  end
  local out = {}
  for i = 0, teeth - 1 do
    local centre = 2 * math.pi * i / teeth
    -- the rising flank, root to tip
    if root < start then
      out[#out + 1] = at(root, centre - half)
    end
    for s = 0, FLANK_STEPS do
      local r = start + (tip - start) * s / FLANK_STEPS
      out[#out + 1] = at(r, centre - half + flank_angle(r))
    end
    -- the falling flank, tip to root
    for s = FLANK_STEPS, 0, -1 do
      local r = start + (tip - start) * s / FLANK_STEPS
      out[#out + 1] = at(r, centre + half - flank_angle(r))
    end
    if root < start then
      out[#out + 1] = at(root, centre + half)
    end
  end
  return out
end

return M
