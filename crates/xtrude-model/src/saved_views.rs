// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Saved views: a named snapshot of the camera and of the
//! layer-visibility state, re-applied with one click. Pure presentation.

use serde_json::{json, Map, Value};

use crate::ids::{next_id, ObjId};

/// The camera a document opens with: the document's `"camera"` object. Every
/// field is optional, and a missing or unreadable one leaves the camera as
/// it is. Keys this version does not know are kept in `extra`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraHome {
    pub target: Option<[f64; 3]>,
    pub distance: Option<f64>,
    pub yaw: Option<f64>,
    pub pitch: Option<f64>,
    pub fov_deg: Option<f64>,
    pub perspective: Option<bool>,
    pub extra: Map<String, Value>,
}

impl CameraHome {
    /// Read a stored camera. `target` takes the numbers of its array and
    /// needs exactly three of them.
    pub fn from_json(m: &Map<String, Value>) -> CameraHome {
        let mut c = CameraHome::default();
        for (k, v) in m {
            match k.as_str() {
                "target" => {
                    c.target = v.as_array().and_then(|t| {
                        let n: Vec<f64> = t.iter().filter_map(Value::as_f64).collect();
                        <[f64; 3]>::try_from(n).ok()
                    })
                }
                "distance" => c.distance = v.as_f64(),
                "yaw" => c.yaw = v.as_f64(),
                "pitch" => c.pitch = v.as_f64(),
                "fov_deg" => c.fov_deg = v.as_f64(),
                "perspective" => c.perspective = v.as_bool(),
                _ => {
                    c.extra.insert(k.clone(), v.clone());
                }
            }
        }
        c
    }

    /// The stored form: the fields that are set, then `extra`.
    pub fn to_json(&self) -> Map<String, Value> {
        let mut m = Map::new();
        if let Some(t) = self.target {
            m.insert("target".into(), json!(t));
        }
        let numbers = [
            ("distance", self.distance),
            ("yaw", self.yaw),
            ("pitch", self.pitch),
            ("fov_deg", self.fov_deg),
        ];
        for (k, v) in numbers {
            if let Some(x) = v {
                m.insert(k.into(), json!(x));
            }
        }
        if let Some(p) = self.perspective {
            m.insert("perspective".into(), json!(p));
        }
        for (k, v) in &self.extra {
            m.insert(k.clone(), v.clone());
        }
        m
    }
}

/// The orbit camera's own terms (what a saved view stores).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraPose {
    pub target: [f64; 3],
    pub distance: f64,
    pub yaw: f64,
    pub pitch: f64,
    pub fov_deg: f64,
    pub perspective: bool,
}

impl Default for CameraPose {
    fn default() -> Self {
        CameraPose {
            target: [0.0, 0.0, 0.0],
            distance: 20.0,
            yaw: -std::f64::consts::FRAC_PI_4,
            pitch: 0.5235987755982988,
            fov_deg: 45.0,
            perspective: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SavedView {
    pub id: ObjId,
    pub name: String,
    pub pose: CameraPose,
    /// Layer NAMES hidden in this view.
    pub hidden_layers: Vec<String>,
    /// Display-style snapshot (Style JSON) or `None`.
    pub style: Option<Value>,
    /// Section state: `{"active": uid|null, "planes_shown", "cuts_shown"}`.
    pub section: Option<Map<String, Value>>,
    /// Group uids hidden when saved; `None` = hands-off.
    pub hidden_objects: Option<Vec<String>>,
    /// `{"objects": bool, "geometry": bool}`; `None` = hands-off.
    pub hidden_shown: Option<Map<String, Value>>,
}

impl SavedView {
    pub fn new(name: &str, pose: CameraPose) -> Self {
        SavedView {
            id: next_id(),
            name: name.to_string(),
            pose,
            hidden_layers: Vec::new(),
            style: None,
            section: None,
            hidden_objects: None,
            hidden_shown: None,
        }
    }

    pub fn to_json(&self) -> Value {
        let p = &self.pose;
        let mut e = json!({
            "name": self.name,
            "target": p.target,
            "distance": p.distance,
            "yaw": p.yaw,
            "pitch": p.pitch,
        });
        if p.fov_deg != 45.0 {
            e["fov"] = json!(p.fov_deg);
        }
        if !p.perspective {
            e["parallel"] = json!(true);
        }
        if !self.hidden_layers.is_empty() {
            e["hidden_layers"] = json!(self.hidden_layers);
        }
        if let Some(s) = &self.style {
            e["style"] = s.clone();
        }
        if let Some(s) = &self.section {
            e["section"] = Value::Object(s.clone());
        }
        if let Some(h) = &self.hidden_objects {
            e["hidden_objects"] = json!(h);
        }
        if let Some(h) = &self.hidden_shown {
            e["hidden_shown"] = Value::Object(h.clone());
        }
        e
    }

    pub fn from_json(raw: &Value) -> SavedView {
        let d = CameraPose::default();
        let t = raw
            .get("target")
            .and_then(Value::as_array)
            .map(|a| {
                [
                    a.first().and_then(Value::as_f64).unwrap_or(0.0),
                    a.get(1).and_then(Value::as_f64).unwrap_or(0.0),
                    a.get(2).and_then(Value::as_f64).unwrap_or(0.0),
                ]
            })
            .unwrap_or(d.target);
        let pose = CameraPose {
            target: t,
            distance: raw
                .get("distance")
                .and_then(Value::as_f64)
                .unwrap_or(d.distance),
            yaw: raw.get("yaw").and_then(Value::as_f64).unwrap_or(d.yaw),
            pitch: raw.get("pitch").and_then(Value::as_f64).unwrap_or(d.pitch),
            fov_deg: raw.get("fov").and_then(Value::as_f64).unwrap_or(d.fov_deg),
            perspective: !raw
                .get("parallel")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        };
        let mut v = SavedView::new(
            raw.get("name").and_then(Value::as_str).unwrap_or("Scene"),
            pose,
        );
        v.hidden_layers = raw
            .get("hidden_layers")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        v.style = raw
            .get("style")
            .filter(|s| s.as_object().is_some_and(|m| !m.is_empty()))
            .cloned();
        v.section = raw
            .get("section")
            .and_then(Value::as_object)
            .filter(|m| !m.is_empty())
            .cloned();
        v.hidden_objects = raw
            .get("hidden_objects")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            });
        v.hidden_shown = raw
            .get("hidden_shown")
            .and_then(Value::as_object)
            .filter(|m| !m.is_empty())
            .cloned();
        v
    }
}

/// An eye/target/up camera, as an importer reads it.
#[derive(Clone, Copy, Debug)]
pub struct LookAt {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub up: [f64; 3],
    pub fov_deg: f64,
    pub perspective: bool,
    /// The height a parallel view frames, if the file gives it.
    pub ortho_height: Option<f64>,
}

/// A saved view from an eye/target/up camera (importer entry).
pub fn from_lookat(name: &str, camera: LookAt, hidden_layers: Vec<String>) -> SavedView {
    let LookAt {
        eye,
        target,
        up,
        fov_deg,
        perspective,
        ortho_height,
    } = camera;
    let (dx, dy, dz) = (eye[0] - target[0], eye[1] - target[1], eye[2] - target[2]);
    let mut distance = (dx * dx + dy * dy + dz * dz).sqrt();
    if distance == 0.0 {
        distance = 1.0;
    }
    let mut pitch = (dz / distance).clamp(-1.0, 1.0).asin();
    let limit = 89f64.to_radians();
    let yaw = if pitch.abs() < limit - 1e-6 {
        dy.atan2(dx)
    } else if up[0] != 0.0 || up[1] != 0.0 {
        (-up[1]).atan2(-up[0])
    } else {
        (-90f64).to_radians()
    };
    pitch = pitch.clamp(-limit, limit);
    if !perspective {
        if let Some(h) = ortho_height.filter(|h| *h > 0.0) {
            distance = (h / 2.0) / (fov_deg.to_radians() / 2.0).tan();
        }
    }
    let mut v = SavedView::new(
        name,
        CameraPose {
            target,
            distance,
            yaw,
            pitch,
            fov_deg,
            perspective,
        },
    );
    v.hidden_layers = hidden_layers;
    v
}

impl CameraPose {
    /// The pose of a live camera.
    pub fn of(camera: &xtrude_core::camera::OrbitCamera) -> CameraPose {
        CameraPose {
            target: camera.target.to_array(),
            distance: camera.distance,
            yaw: camera.yaw,
            pitch: camera.pitch,
            fov_deg: camera.fov_deg,
            perspective: camera.perspective,
        }
    }

    /// Put `camera` in this pose.
    pub fn apply(&self, camera: &mut xtrude_core::camera::OrbitCamera) {
        camera.target = xtrude_core::DVec3::from_array(self.target);
        camera.distance = self.distance;
        camera.yaw = self.yaw;
        camera.pitch = self.pitch;
        camera.fov_deg = self.fov_deg;
        camera.perspective = self.perspective;
    }
}

impl SavedView {
    /// Snapshot the live camera and the current visibility state.
    pub fn capture(
        name: &str,
        scene: &crate::scene::Scene,
        camera: &xtrude_core::camera::OrbitCamera,
    ) -> SavedView {
        let mut v = SavedView::new(name, CameraPose::of(camera));
        v.hidden_layers = scene
            .doc
            .layers
            .iter()
            .filter(|l| !l.visible)
            .map(|l| l.name.clone())
            .collect();
        v.style = Some(scene.display_style.to_json());
        let mut section = Map::new();
        section.insert(
            "active".into(),
            scene
                .active_section()
                .map_or(Value::Null, |sp| Value::from(sp.uid)),
        );
        section.insert(
            "planes_shown".into(),
            Value::Bool(scene.show_section_planes),
        );
        section.insert("cuts_shown".into(), Value::Bool(scene.show_section_cuts));
        v.section = Some(section);
        v.hidden_objects = Some(
            scene
                .groups_by_uid()
                .into_iter()
                .filter(|(_, gid)| scene.doc.find_group(*gid).is_some_and(|g| g.hidden))
                .map(|(uid, _)| uid)
                .collect(),
        );
        let mut shown = Map::new();
        shown.insert("objects".into(), Value::Bool(scene.show_hidden_objects));
        shown.insert("geometry".into(), Value::Bool(scene.show_hidden_geometry));
        v.hidden_shown = Some(shown);
        v
    }

    /// Recall the view: camera first, then the visibility state. The
    /// caller bumps `scene.version` / repaints.
    pub fn apply(
        &self,
        scene: &mut crate::scene::Scene,
        camera: &mut xtrude_core::camera::OrbitCamera,
    ) {
        self.pose.apply(camera);
        for ly in scene.doc.layers.iter_mut() {
            ly.visible = !self.hidden_layers.contains(&ly.name);
        }
        if let Some(style) = self
            .style
            .as_ref()
            .filter(|s| s.as_object().is_some_and(|o| !o.is_empty()))
        {
            scene.display_style = crate::style::Style::from_json(style);
        }
        if let Some(section) = &self.section {
            let uid = section.get("active").and_then(Value::as_i64);
            let target = scene
                .doc
                .section_planes
                .iter()
                .find(|sp| Some(sp.uid) == uid)
                .map(|sp| sp.id);
            scene.set_active_section(target);
            scene.show_section_planes = section
                .get("planes_shown")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            scene.show_section_cuts = section
                .get("cuts_shown")
                .and_then(Value::as_bool)
                .unwrap_or(true);
        }
        if let Some(hidden) = &self.hidden_objects {
            for (uid, gid) in scene.groups_by_uid() {
                if let Some(g) = scene.doc.find_group_mut(gid) {
                    g.hidden = hidden.contains(&uid);
                }
            }
        }
        if let Some(shown) = &self.hidden_shown {
            scene.show_hidden_objects = shown
                .get("objects")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            scene.show_hidden_geometry = shown
                .get("geometry")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_home_keeps_what_it_can_read_and_what_it_does_not_know() {
        let m = json!({"target": [1, "x", 2, 3], "distance": "far", "yaw": 0.5,
                       "perspective": false, "roll": 0.1});
        let c = CameraHome::from_json(m.as_object().unwrap());
        assert_eq!(c.target, Some([1.0, 2.0, 3.0]));
        assert_eq!(c.distance, None);
        assert_eq!(c.yaw, Some(0.5));
        assert_eq!((c.pitch, c.fov_deg), (None, None));
        assert_eq!(c.perspective, Some(false));
        assert_eq!(c.extra.get("roll"), Some(&json!(0.1)));
        let short = json!({"target": [1.0, 2.0]});
        assert_eq!(
            CameraHome::from_json(short.as_object().unwrap()).target,
            None
        );
        assert_eq!(CameraHome::from_json(&c.to_json()), c);
    }
}
