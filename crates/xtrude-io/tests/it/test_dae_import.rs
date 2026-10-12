// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! COLLADA import.

use crate::common;
use common::*;
use std::collections::BTreeSet;
use xtrude_core::math::{DMat4, MatExt};
use xtrude_core::orient::{is_closed, signed_volume};
use xtrude_core::{DVec3, Mesh};
use xtrude_io::dae::{load_dae, load_dae_with, DaeLimits};
use xtrude_io::igz;
use xtrude_model::commands::{ExplodeGroupCommand, MoveGroupCommand};
use xtrude_model::group::{add_mesh, materialize, Group};
use xtrude_model::history::History;
use xtrude_model::scene::Scene;

const NSDECL: &str = r#"xmlns="http://www.collada.org/2005/11/COLLADASchema""#;

const SMALL: DaeLimits = DaeLimits {
    max_fuse_loops: 2,
    split_min: 1,
};

fn cube_dae(tmp: &TmpDir, up: &str, meter: &str) -> std::path::PathBuf {
    let pos = "0 0 0  1 0 0  1 1 0  0 1 0  0 0 1  1 0 1  1 1 1  0 1 1";
    let tris = "0 3 2 0 2 1  4 5 6 4 6 7  0 1 5 0 5 4  1 2 6 1 6 5  2 3 7 2 7 6  3 0 4 3 4 7";
    let ns = NSDECL;
    let text = format!(
        r##"<?xml version="1.0"?>
<COLLADA {ns} version="1.4.1">
  <asset><unit meter="{meter}"/><up_axis>{up}</up_axis></asset>
  <library_effects>
    <effect id="fx-red"><profile_COMMON><technique sid="t">
      <lambert><diffuse><color>1 0 0 1</color></diffuse></lambert>
    </technique></profile_COMMON></effect>
  </library_effects>
  <library_materials>
    <material id="mat-red"><instance_effect url="#fx-red"/></material>
  </library_materials>
  <library_geometries>
    <geometry id="cube"><mesh>
      <source id="cube-pos">
        <float_array id="cube-pos-arr" count="24">{pos}</float_array>
        <technique_common>
          <accessor source="#cube-pos-arr" count="8" stride="3"/>
        </technique_common>
      </source>
      <vertices id="cube-v">
        <input semantic="POSITION" source="#cube-pos"/>
      </vertices>
      <triangles count="12" material="RED">
        <input semantic="VERTEX" source="#cube-v" offset="0"/>
        <p>{tris}</p>
      </triangles>
    </mesh></geometry>
  </library_geometries>
  <library_visual_scenes>
    <visual_scene id="scene">
      <node id="n1">
        <instance_geometry url="#cube">
          <bind_material><technique_common>
            <instance_material symbol="RED" target="#mat-red"/>
          </technique_common></bind_material>
        </instance_geometry>
      </node>
    </visual_scene>
  </library_visual_scenes>
</COLLADA>
"##
    );
    let p = tmp.join("cube.dae");
    std::fs::write(&p, text).unwrap();
    p
}

#[test]
fn test_triangulated_cube_comes_back_as_six_quads() {
    let tmp = TmpDir::new();
    let mut s = Scene::new();
    load_dae(&mut s, &cube_dae(&tmp, "Z_UP", "1.0"), None).unwrap();
    let m = s.mesh();
    assert_eq!(m.faces.len(), 6);
    assert!(m.face_ids().iter().all(|&f| m.face(f).outer.len() == 4));
    assert!(is_closed(m));
    assert!((signed_volume(m) - 1.0).abs() < 1e-6);
    assert!(m
        .face_ids()
        .iter()
        .all(|&f| m.face(f).attrs.paint.color == Some([1.0, 0.0, 0.0])));
}

#[test]
fn test_y_up_and_inches_convert_to_zup_metres() {
    let tmp = TmpDir::new();
    let mut s = Scene::new();
    load_dae(&mut s, &cube_dae(&tmp, "Y_UP", "0.0254"), None).unwrap();
    let m = s.mesh();
    assert!(is_closed(m));
    assert!((signed_volume(m) - 0.0254f64.powi(3)).abs() < 1e-9);
    let zs: BTreeSet<i64> = m
        .verts
        .iter()
        .map(|(_, v)| (v.position.z * 1e6).round() as i64)
        .collect();
    assert_eq!(zs.into_iter().collect::<Vec<_>>(), [0, 25400]);
}

#[test]
fn test_instanced_component_with_transform() {
    let tmp = TmpDir::new();
    let ns = NSDECL;
    let body = format!(
        r##"<?xml version="1.0"?>
<COLLADA {ns} version="1.4.1">
  <asset><up_axis>Z_UP</up_axis></asset>
  <library_geometries>
    <geometry id="tri"><mesh>
      <source id="p"><float_array id="pa" count="9">0 0 0  1 0 0  0 1 0</float_array>
        <technique_common><accessor source="#pa" count="3" stride="3"/></technique_common>
      </source>
      <vertices id="v"><input semantic="POSITION" source="#p"/></vertices>
      <polylist count="1">
        <input semantic="VERTEX" source="#v" offset="0"/>
        <vcount>3</vcount><p>0 1 2</p>
      </polylist>
    </mesh></geometry>
  </library_geometries>
  <library_nodes>
    <node id="comp"><instance_geometry url="#tri"/></node>
  </library_nodes>
  <library_visual_scenes>
    <visual_scene id="scene">
      <node id="a"><instance_node url="#comp"/></node>
      <node id="b"><translate>10 0 0</translate><instance_node url="#comp"/></node>
    </visual_scene>
  </library_visual_scenes>
</COLLADA>
"##
    );
    let p = tmp.join("comp.dae");
    std::fs::write(&p, body).unwrap();
    let mut s = Scene::new();
    load_dae(&mut s, &p, None).unwrap();
    let xs: Vec<i64> = s
        .mesh()
        .verts
        .iter()
        .map(|(_, v)| (v.position.x * 1000.0).round() as i64)
        .collect();
    assert_eq!(s.mesh().faces.len(), 2);
    assert!(xs.contains(&10000) && xs.contains(&11000));
}

#[test]
fn test_big_import_mirrors_the_source_groups() {
    let tmp = TmpDir::new();
    let ns = NSDECL;
    let body = format!(
        r##"<?xml version="1.0"?>
<COLLADA {ns} version="1.4.1">
  <asset><up_axis>Z_UP</up_axis></asset>
  <library_geometries>
    <geometry id="tri"><mesh>
      <source id="p"><float_array id="pa" count="9">0 0 0  1 0 0  0 1 0</float_array>
        <technique_common><accessor source="#pa" count="3" stride="3"/></technique_common>
      </source>
      <vertices id="v"><input semantic="POSITION" source="#p"/></vertices>
      <polylist count="1">
        <input semantic="VERTEX" source="#v" offset="0"/>
        <vcount>3</vcount><p>0 1 2</p>
      </polylist>
    </mesh></geometry>
    <geometry id="quad"><mesh>
      <source id="qp"><float_array id="qpa" count="12">0 0 0  1 0 0  1 1 0  0 1 0</float_array>
        <technique_common><accessor source="#qpa" count="4" stride="3"/></technique_common>
      </source>
      <vertices id="qv"><input semantic="POSITION" source="#qp"/></vertices>
      <polylist count="2">
        <input semantic="VERTEX" source="#qv" offset="0"/>
        <vcount>3 3</vcount><p>0 1 2 0 2 3</p>
      </polylist>
    </mesh></geometry>
  </library_geometries>
  <library_nodes>
    <node id="comp"><instance_geometry url="#tri"/></node>
  </library_nodes>
  <library_visual_scenes>
    <visual_scene id="scene">
      <node id="root" name="Model">
        <instance_geometry url="#tri"/>
        <node id="f1" name="frame"><translate>10 0 0</translate>
          <instance_node url="#comp"/></node>
        <node id="g1" name="plate"><instance_geometry url="#quad"/></node>
      </node>
    </visual_scene>
  </library_visual_scenes>
</COLLADA>
"##
    );
    let p = tmp.join("project.dae");
    std::fs::write(&p, body).unwrap();
    let mut s = Scene::new();
    load_dae_with(&mut s, &p, None, SMALL).unwrap();
    assert!(s.mesh().faces.is_empty());
    let mut names: Vec<&str> = s.doc.groups.iter().map(|g| g.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["Model", "frame", "plate"]);
    let by = |n: &str| s.doc.groups.iter().find(|g| g.name == n).unwrap();
    assert_eq!(s.doc.mesh(by("plate").mesh).faces.len(), 1);
    let min_x = s
        .doc
        .mesh(by("frame").mesh)
        .verts
        .iter()
        .map(|(_, v)| v.position.x)
        .fold(f64::MAX, f64::min);
    assert!(min_x >= 10.0);
}

#[test]
fn test_empty_document_raises() {
    let tmp = TmpDir::new();
    let p = tmp.join("empty.dae");
    std::fs::write(
        &p,
        format!(r#"<?xml version="1.0"?><COLLADA {NSDECL} version="1.4.1"/>"#),
    )
    .unwrap();
    assert!(load_dae(&mut Scene::new(), &p, None).is_err());
}

fn textured_quad_dae(tmp: &TmpDir, image_name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let img = tmp.join(image_name);
    let mut brick = image::RgbImage::from_pixel(64, 64, image::Rgb([160, 70, 50]));
    for y in (0..64).step_by(16) {
        for x in 0..64 {
            brick.put_pixel(x, y, image::Rgb([200, 200, 190]));
        }
    }
    brick.save(&img).unwrap();
    let ns = NSDECL;
    let body = format!(
        r##"<?xml version="1.0"?>
<COLLADA {ns} version="1.4.1">
  <asset><up_axis>Z_UP</up_axis></asset>
  <library_images>
    <image id="img1"><init_from>{image_name}</init_from></image>
  </library_images>
  <library_effects>
    <effect id="fx1"><profile_COMMON>
      <newparam sid="surf"><surface type="2D"><init_from>img1</init_from></surface></newparam>
      <newparam sid="samp"><sampler2D><source>surf</source></sampler2D></newparam>
      <technique sid="t"><lambert><diffuse>
        <texture texture="samp" texcoord="UV0"/>
      </diffuse></lambert></technique>
    </profile_COMMON></effect>
  </library_effects>
  <library_materials>
    <material id="mat1"><instance_effect url="#fx1"/></material>
  </library_materials>
  <library_geometries>
    <geometry id="quad"><mesh>
      <source id="qp"><float_array id="qpa" count="12">0 0 0  2 0 0  2 3 0  0 3 0</float_array>
        <technique_common><accessor source="#qpa" count="4" stride="3"/></technique_common>
      </source>
      <source id="quv"><float_array id="quva" count="8">0 0  1 0  1 1  0 1</float_array>
        <technique_common><accessor source="#quva" count="4" stride="2"/></technique_common>
      </source>
      <vertices id="qv"><input semantic="POSITION" source="#qp"/></vertices>
      <triangles count="2" material="M">
        <input semantic="VERTEX" source="#qv" offset="0"/>
        <input semantic="TEXCOORD" source="#quv" offset="1" set="0"/>
        <p>0 0 1 1 2 2  0 0 2 2 3 3</p>
      </triangles>
    </mesh></geometry>
  </library_geometries>
  <library_visual_scenes>
    <visual_scene id="scene">
      <node id="n1">
        <instance_geometry url="#quad">
          <bind_material><technique_common>
            <instance_material symbol="M" target="#mat1"/>
          </technique_common></bind_material>
        </instance_geometry>
      </node>
    </visual_scene>
  </library_visual_scenes>
</COLLADA>
"##
    );
    let p = tmp.join("textured.dae");
    std::fs::write(&p, body).unwrap();
    (p, img)
}

#[test]
fn test_a_textured_material_imports_as_its_image_colour() {
    let tmp = TmpDir::new();
    let (p, _img) = textured_quad_dae(&tmp, "brick.png");
    let mut s = Scene::new();
    load_dae(&mut s, &p, None).unwrap();
    let m = s.mesh();
    assert_eq!(m.faces.len(), 1);
    let [r, g, b] = m.face(m.face_ids()[0]).attrs.paint.color.unwrap();
    assert!(
        (r - 0.64).abs() < 0.05 && (g - 0.31).abs() < 0.05 && (b - 0.23).abs() < 0.05,
        "{r} {g} {b}"
    );
}

#[test]
fn test_a_missing_image_falls_back_to_a_tint() {
    let tmp = TmpDir::new();
    let (p, img) = textured_quad_dae(&tmp, "brick.png");
    std::fs::remove_file(img).unwrap();
    let mut s = Scene::new();
    load_dae(&mut s, &p, None).unwrap();
    let m = s.mesh();
    assert_eq!(m.faces.len(), 1);
    assert!(m.face(m.face_ids()[0]).attrs.paint.color.is_some());
}

#[test]
fn test_a_vertical_photo_panel_imports_as_plain_geometry() {
    let tmp = TmpDir::new();
    let (p, _img) = textured_quad_dae(&tmp, "mural.png");
    let text = std::fs::read_to_string(&p)
        .unwrap()
        .replace("0 0 0  2 0 0  2 3 0  0 3 0", "0 0 0  2 0 0  2 0 3  0 0 3");
    std::fs::write(&p, text).unwrap();
    let mut s = Scene::new();
    load_dae(&mut s, &p, None).unwrap();
    assert!(s.doc.groups.is_empty());
    assert_eq!(s.mesh().faces.len(), 1);
}

#[test]
fn test_repeated_components_share_one_prototype() {
    let tmp = TmpDir::new();
    let quad = r##"
    <geometry id="quad"><mesh>
      <source id="qp"><float_array id="qpa" count="12">0 0 0  1 0 0  1 1 0  0 1 0</float_array>
        <technique_common><accessor source="#qpa" count="4" stride="3"/></technique_common>
      </source>
      <vertices id="qv"><input semantic="POSITION" source="#qp"/></vertices>
      <polylist count="2">
        <input semantic="VERTEX" source="#qv" offset="0"/>
        <vcount>3 3</vcount><p>0 1 2 0 2 3</p>
      </polylist>
    </mesh></geometry>"##;
    let ns = NSDECL;
    let body = format!(
        r##"<?xml version="1.0"?>
<COLLADA {ns} version="1.4.1">
  <asset><up_axis>Z_UP</up_axis></asset>
  <library_geometries>{quad}</library_geometries>
  <library_nodes>
    <node id="comp" name="spacer"><instance_geometry url="#quad"/></node>
  </library_nodes>
  <library_visual_scenes>
    <visual_scene id="scene">
      <node id="root" name="Model">
        <node id="i1"><instance_node url="#comp"/></node>
        <node id="i2"><translate>10 0 0</translate><instance_node url="#comp"/></node>
        <node id="i3"><translate>20 0 0</translate><instance_node url="#comp"/></node>
      </node>
    </visual_scene>
  </library_visual_scenes>
</COLLADA>
"##
    );
    let p = tmp.join("spacers.dae");
    std::fs::write(&p, body).unwrap();
    let mut s = Scene::new();
    load_dae_with(&mut s, &p, None, SMALL).unwrap();
    let inst: Vec<Group> = s
        .doc
        .groups
        .iter()
        .filter(|g| g.xform.is_some())
        .cloned()
        .collect();
    assert_eq!(inst.len(), 3);
    assert_eq!(
        inst.iter().map(|g| g.mesh).collect::<BTreeSet<_>>().len(),
        1
    );
    assert_eq!(s.doc.mesh(inst[0].mesh).faces.len(), 1);
    let origin_x =
        |g: &Group| (g.xform.unwrap().project_point(DVec3::ZERO).x * 1000.0).round() as i64;
    let origin_y =
        |g: &Group| (g.xform.unwrap().project_point(DVec3::ZERO).y * 1000.0).round() as i64;
    let mut origins: Vec<i64> = inst.iter().map(origin_x).collect();
    origins.sort();
    assert_eq!(origins, [0, 10000, 20000]);

    let mut h = History::new();
    h.execute(
        &mut s,
        &mut MoveGroupCommand {
            group: inst[0].id,
            delta: v3(0., 5., 0.),
        },
    );
    let g0 = s.doc.find_group(inst[0].id).unwrap().clone();
    let g1 = s.doc.find_group(inst[1].id).unwrap().clone();
    assert_eq!(origin_y(&g0), 5000);
    assert_eq!(origin_y(&g1), 0);
    assert_eq!(g0.mesh, g1.mesh);
    h.undo(&mut s);
    assert_eq!(origin_y(s.doc.find_group(inst[0].id).unwrap()), 0);

    let out = tmp.join("spacers.igz");
    igz::save_scene(&s, &out).unwrap();
    let mut s2 = Scene::new();
    igz::load_into(&mut s2, &out, None).unwrap();
    let inst2: Vec<Group> = s2
        .doc
        .groups
        .iter()
        .filter(|g| g.xform.is_some())
        .cloned()
        .collect();
    assert_eq!(inst2.len(), 3);
    assert_eq!(
        inst2.iter().map(|g| g.mesh).collect::<BTreeSet<_>>().len(),
        1
    );
    let mut origins2: Vec<i64> = inst2.iter().map(origin_x).collect();
    origins2.sort();
    assert_eq!(origins2, [0, 10000, 20000]);

    let mut g = inst2[1].clone();
    materialize(&mut s2.doc.meshes, &mut g);
    assert!(g.xform.is_none());
    assert_ne!(g.mesh, inst2[0].mesh);
    let xs: BTreeSet<i64> = s2
        .doc
        .mesh(g.mesh)
        .verts
        .iter()
        .map(|(_, v)| (v.position.x * 1000.0).round() as i64)
        .collect();
    let xs: Vec<i64> = xs.into_iter().collect();
    assert!([0, 10000, 20000].contains(&xs[0]) && xs[xs.len() - 1] - xs[0] == 1000);
}

#[test]
fn test_instance_edit_flows_make_independent() {
    let mut s = Scene::new();
    let mut proto = Mesh::new();
    quad(&mut proto, 0., 1., 0., 1., 0.);
    let pid = add_mesh(&mut s.doc.meshes, proto);
    let mut g1 = Group::new(pid, Some("a"));
    let mut g2 = Group::new(pid, Some("b"));
    g1.xform = Some(DMat4::IDENTITY);
    g2.xform = Some(DMat4::from_translation(v3(10., 0., 0.)));
    let (id1, id2) = (g1.id, g2.id);
    s.doc.groups.extend([g1, g2]);
    s.begin_group_edit(id2);
    let g2 = s.doc.find_group(id2).unwrap().clone();
    assert!(g2.xform.is_none() && g2.mesh != pid);
    assert_eq!(s.active_mesh_id(), g2.mesh);
    let xs: BTreeSet<i64> = s
        .mesh()
        .verts
        .iter()
        .map(|(_, v)| (v.position.x * 1000.0).round() as i64)
        .collect();
    assert_eq!(xs.into_iter().collect::<Vec<_>>(), [10000, 11000]);
    s.end_group_edit();
    let g1 = s.doc.find_group(id1).unwrap();
    assert!(g1.mesh == pid && g1.xform.is_some());
    let mut h = History::new();
    h.execute(&mut s, &mut ExplodeGroupCommand { group: id1 });
    assert!(s.doc.find_group(id1).is_none());
    let xs: BTreeSet<i64> = s
        .mesh()
        .verts
        .iter()
        .map(|(_, v)| (v.position.x * 1000.0).round() as i64)
        .collect();
    assert_eq!(xs.into_iter().collect::<Vec<_>>(), [0, 1000]);
    h.undo(&mut s);
    assert!(s.doc.find_group(id1).is_some() && s.mesh().faces.is_empty());
}
