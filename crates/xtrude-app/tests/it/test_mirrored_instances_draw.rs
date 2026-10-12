// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A mirrored component placement shows its FRONT. The frame is
//! rendered headless through wgpu and its pixels read back; skipped where the
//! machine offers no adapter. Xtrude CAD bakes placements into the frame
//! cache.

use crate::common;

use glam::{DMat4, DVec3};
use xtrude_app::viewport::Viewport;
use xtrude_core::attrs::Paint;
use xtrude_core::Mesh;
use xtrude_model::group::{add_mesh, Group};
use xtrude_render::gpu::ViewportRenderer;

const AMARILLO: [f64; 3] = [1.0, 0.85, 0.0];

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        ..Default::default()
    }))
    .ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

fn render(vp: &mut Viewport, w: u32, h: u32) -> Option<(u32, u32, Vec<u8>)> {
    let (device, queue) = device()?;
    let mut r = ViewportRenderer::new(&device, 1);
    let frame = vp.frame_data([w, h], true);
    r.render(&device, &queue, &frame);
    r.read_pixels(&device, &queue)
}

fn rgb(px: &(u32, u32, Vec<u8>), x: f64, y: f64) -> [f64; 3] {
    let i = ((y as u32 * px.0 + x as u32) * 4) as usize;
    [px.2[i], px.2[i + 1], px.2[i + 2]].map(|c| c as f64 / 255.0)
}

#[test]
fn test_a_mirrored_placement_draws_instanced_with_its_front_colour() {
    let mut vp = common::sized_viewport(400., 200.);
    let mut proto = Mesh::new();
    let f = proto.add_face(
        &[
            DVec3::new(-1., -1., 0.),
            DVec3::new(1., -1., 0.),
            DVec3::new(1., 1., 0.),
            DVec3::new(-1., 1., 0.),
        ],
        &[],
    );
    proto.face_mut(f).attrs.paint = Paint {
        color: Some(AMARILLO),
        ..Default::default()
    };
    let mesh = add_mesh(&mut vp.scene.doc.meshes, proto);
    let right = DMat4::from_translation(DVec3::new(-2., 0., 0.));
    let mirror = DMat4::from_translation(DVec3::new(2., 0., 0.))
        * DMat4::from_scale(DVec3::new(-1., 1., 1.));
    for m in [right, mirror] {
        let mut g = Group::new(mesh, Some("Part"));
        g.xform = Some(m);
        vp.scene.doc.groups.push(g);
    }
    vp.scene.version += 1;
    vp.camera.set_view("top"); // looking down −Z: the front
    vp.camera.perspective = false;
    vp.camera
        .fit_to(DVec3::new(-4., -2., 0.), DVec3::new(4., 2., 0.), 1.2);
    let Some(px) = render(&mut vp, 400, 200) else {
        eprintln!("no wgpu adapter: skipped");
        return;
    };
    for x in [-2.0, 2.0] {
        let p = vp
            .world_to_pixel(DVec3::new(x, 0.5, 0.))
            .expect("on screen");
        let [r, g, b] = rgb(&px, p.x, p.y);
        assert!(r > 0.6 && g > 0.5 && b < 0.35, "{x}: {:?}", (r, g, b));
    }
}
