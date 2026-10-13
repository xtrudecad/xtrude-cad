# Lua extensions

Xtrude CAD runs extensions written in [Lua 5.4](https://www.lua.org/manual/5.4/).
An extension adds commands to the **Extensions** menu. Each command runs
a Lua function that reads and edits the document and then returns, for
example a parametric generator (gears, boxes, standoffs, hinges), a batch
edit, an export or a check of the model.

Every run is **one undo step**. If the script fails, the document goes
back to exactly what it was before the run.

Two examples ship with the source, in
[`resources/extensions/examples/`](../resources/extensions/examples/): a
box with a lid (`box/`) and an involute spur gear (`gears/`). Copy a
folder into your extensions folder to try it.

## Where extensions live

Each extension is a folder in the extensions folder:

```
$XDG_CONFIG_HOME/xtrude/extensions/<id>/init.lua
```

If `XDG_CONFIG_HOME` is not set, the folder is `~/.config/xtrude/extensions/`.
**Extensions ▸ Open Extensions Folder** creates the folder and opens it.

Extensions are loaded when the app starts, in folder-name order.
**Extensions ▸ Reload Extensions** loads them again, so you can edit an
extension and try it without restarting. An extension that fails to load
is skipped, and the others still load. The Extensions menu then says how
many failed, and **Show Loading Errors…** gives each folder and the
reason.

A folder is skipped when:

- it has no `init.lua`, or `init.lua` does not run;
- `init.lua` does not return a manifest table, or a field of the manifest
  is missing or wrong;
- the manifest's `id` is not the folder's name;
- the manifest's `api` is newer than the app supports.

Folders whose name starts with a dot are ignored.

## The manifest

`init.lua` returns a table that describes the extension:

```lua
-- SPDX-License-Identifier: GPL-3.0-or-later
return {
  id = "gears",            -- the folder's name; letters, digits, _ and -
  name = "Gears",          -- shown in the menu
  version = "1.0.0",       -- optional
  api = 1,                 -- the API version the extension was written for
  commands = {
    {
      id = "spur",         -- letters, digits, _ and -
      label = "Spur gear…",
      params = {
        { key = "teeth",  label = "Teeth",  type = "int",    default = 20, min = 6, max = 200 },
        { key = "module", label = "Module", type = "length", default = 1.0 },
        { key = "width",  label = "Width",  type = "length", default = 5.0 },
      },
      run = function(doc, p)
        -- p.teeth, p.module, p.width
      end,
    },
  },
}
```

An extension with one command puts it straight in the Extensions menu. An
extension with several commands gets a submenu with its `name`. The undo
step is named after the command's label, without a trailing ellipsis
("Undo Spur gear").

An extension may split its code into modules. `require "a.b"` loads
`a/b.lua` from the extension's own folder, once. `require` reaches no
other folder.

## Parameters

When a command has `params`, choosing it opens a form with one entry per
parameter. **Cancel** runs nothing. The values reach `run` as its second
argument, a table indexed by each parameter's `key`. The form offers the
values the user gave last time, if they are still valid.

| Field | Meaning |
|---|---|
| `key` | The name of the value in the table `run` receives. Required. |
| `label` | The entry's label. Defaults to `key`. |
| `type` | `int`, `number`, `length`, `bool` or `choice`. Required. |
| `default` | The value the form first offers. |
| `min`, `max` | Bounds for `int`, `number` and `length`, both optional. |
| `choices` | For `choice`: the list of strings to choose from. Required. |

| Type | Lua value | Default when omitted |
|---|---|---|
| `int` | an integer (`math.type(v) == "integer"`) | `min`, else 0 |
| `number` | a number | `min`, else 0 |
| `length` | a number in the document's unit | `min`, else 0 |
| `bool` | `true` or `false` | `false` |
| `choice` | the chosen string | the first choice |

A length is typed as in the value box: a bare number is in the
document's unit, and a number with a unit (`12mm`, `1.5cm`, `3/4"`) is
converted. A default outside its bounds makes the manifest invalid. The
form does not accept a value outside the bounds.

## Running a single script

**Extensions ▸ Run Lua Script…** runs a `.lua` file once, without a
manifest. The file is the body of the command. It receives `doc` as its
first argument (`local doc = ...`) and may `require` modules from its own
folder. The run is one undo step named "Run <file name>". A one-off
script cannot keep data in the document (`doc:data()`).

## Units

Lengths cross the API in the **document's unit** (Model Info ▸ Units:
millimetres by default). This covers points, distances, bounds, areas
(in square units) and `length` parameters. A script that draws a
`xtrude.vec(20, 0)` point puts it 20 mm from the origin in a millimetre
document and 20 cm from it in a centimetre document. `xtrude.unit()`
returns the unit's code, so a script can convert when it needs a fixed
size.

Angles are in degrees.

## API version 1

### `xtrude`

| | |
|---|---|
| `xtrude.api` | The API version: `1`. |
| `xtrude.version()` | The app's version, as a string. |
| `xtrude.unit()` | The document's length unit: `"mm"`, `"cm"`, `"m"`, `"in"`, `"ft"`, … |
| `xtrude.vec(x, y, z)` | A vector. `z` defaults to 0. |
| `xtrude.ask_save_path(title, filter)` | Asks the user for a file to write. Returns the path, or `nil` if the user cancels. `filter` is like `"STL mesh (*.stl)"`. |

### Vectors

`v.x`, `v.y` and `v.z` read the components. `v + w`, `v - w`, `-v`,
`v * k`, `k * v`, `v / k` and `v == w` work as expected. `v:dot(w)`,
`v:cross(w)` and `v:length()` work too. `v:unit()` is the unit vector, or
the zero vector for a vector too short to have a direction.
`tostring(v)` gives `vec(x, y, z)`.

### `doc`, the document

Geometry is built in the context being edited: the model, or the open
group.

| | |
|---|---|
| `doc:add_face(points, { holes = { points, … } })` | Draws a face through `points` (at least 3, in one plane), with optional holes. Returns the face. |
| `doc:add_edge(a, b)` | Draws an edge. Returns the edge. |
| `doc:push_pull(face, distance, { keep_base = false })` | Pushes or pulls a face along its normal. Returns the face at the new position, or `nil` when there is nothing to push (too short a distance, or a hole cut through). Raises an error when the push would break the solid. |
| `doc:make_group(entities, name)` | Groups a list of faces and edges of the context. The group goes into the context: inside the group being edited, if any. Returns the group. |
| `doc:make_component(entities, name)` | The same, as a component. Given one group, makes it a component. |
| `doc:set_paint(face_or_group, "#rrggbb")` | Paints a face or a group. `nil` removes the paint. |
| `doc:faces()` | The faces of the context being edited. |
| `doc:selection()` | The selected faces, edges and groups. |
| `doc:select(list)` | Selects these faces, edges and groups. |
| `doc:bounds()` | The corners `min, max` of the model's bounding box, or `nil, nil` for an empty model. |
| `doc:data()` | The extension's own data in the document (see below), or `nil`. |
| `doc:set_data(table)` | Replaces the extension's data. `nil` removes it. |
| `doc:export_stl(path, { selection_only = false })` | Writes the model (or only the selected groups and loose faces) as a binary STL in millimetres. |
| `doc:export_obj(path)` | Writes the model as a Wavefront OBJ in metres, with its colours in a `.mtl` file of the same name beside it, as **File ▸ Export ▸ OBJ** does. |

### Faces, edges and groups

Faces, edges and groups are **handles**. Push/pull, stitching and
merging replace faces, so a handle can outlive what it names. Using such a
handle raises the error "face no longer exists". `h:exists()` checks
first. Handles compare with `==`, and `tostring(h)` names them.

| | |
|---|---|
| `face:vertices()` | The outline's points. |
| `face:normal()` | The unit normal. |
| `face:area()` | The area, in square units. |
| `edge:vertices()` | The two end points. |
| `group:name()` | The group's name. |
| `group:faces()` | The faces inside the group. |
| `group:move(v)` | Moves the group by `v`. |
| `group:rotate(point, axis, degrees)` | Rotates it about the axis through `point`. |
| `group:scale(origin, k)` | Scales it about `origin`, by a number or by a vector of factors per axis. |

### Data in the document

`doc:data()` and `doc:set_data(t)` keep a table in the document, under
the extension's `id`. The data is saved in the document (`.xtrude`) and follows
undo and redo. The table may hold strings, numbers, booleans and tables
of them. An extension sees only its own data, never the data of other
extensions or of IngeTrazo extensions. Documents keep that data
unchanged.

### Writing files

A script cannot open files by name. It asks the user with
`xtrude.ask_save_path`, and the exports accept only a path the user
chose that way **in the same run**:

```lua
local doc = ...
local path = xtrude.ask_save_path("Export the part", "STL mesh (*.stl)")
if path then
  doc:export_stl(path, { selection_only = true })
end
```

## The sandbox

A script sees only the safe part of Lua: the base library without
`dofile` and `loadfile`, plus `table`, `string`, `math` and `utf8`. There
is no `io`, `os`, `package` or `debug`. `load` takes source text only,
never bytecode. A script cannot reach the network, other files, or other
extensions.

An extension's Lua state may use up to **256 MiB** of memory, and a run
may take up to **10 seconds**. A run that goes over a limit is stopped
with an error. `pcall` cannot catch the time limit.

## Errors

A failing run changes nothing. A Lua error (`error("…")`, a call with
wrong arguments, a stale handle, a limit) stops the run. The document
goes back to what it was before, and no undo step is added. A dialog then
shows the extension's name, the message with its file and line
(`gears/init.lua:42: …`) and the Lua traceback.

## Compatibility

The API described here is **version 1**. It stays compatible: later
versions of Xtrude CAD add functions but do not change or remove these.
A breaking change raises `xtrude.api`, and an extension declares the
version it was written for in its manifest's `api`. The app refuses to
load an extension that needs a newer API than it has.

Extensions are Lua packages written for this API; extensions written for
other applications do not load.

Not in version 1: interactive tools, viewport overlays, panels, events
("on save", "on selection change"), network access, and file access other
than the exports above.
