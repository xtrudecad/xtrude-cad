// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! The wgpu pass end to end, headless: a frame drawn offscreen and read
//! back. Skipped when the machine offers no adapter at all.

use glam::{DMat4, DVec3};
use std::sync::Arc;
use xtrude_render::frame::{Cached, Depth, Draw, FaceVertex, FrameData, Source, Stipple};
use xtrude_render::gpu::ViewportRenderer;

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        ..Default::default()
    }))
    .ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

fn tri(z: f32, front: [f32; 4], back: [f32; 4]) -> Vec<FaceVertex> {
    [[-0.5, -0.5, z], [0.5, -0.5, z], [0.0, 0.5, z]]
        .map(|pos| FaceVertex { pos, front, back })
        .to_vec()
}

fn pixel(px: &(u32, u32, Vec<u8>), x: u32, y: u32) -> [u8; 4] {
    let i = ((y * px.0 + x) * 4) as usize;
    [px.2[i], px.2[i + 1], px.2[i + 2], px.2[i + 3]]
}

#[test]
fn test_a_frame_draws_front_and_back_colours_offscreen() {
    let Some((device, queue)) = device() else {
        eprintln!("no wgpu adapter: skipped");
        return;
    };
    let mut r = ViewportRenderer::new(&device, 1);
    let red = [1.0, 0.0, 0.0, 1.0];
    let blue = [0.0, 0.0, 1.0, 1.0];
    let mut frame = FrameData {
        size: [64, 64],
        background: [0.5, 0.5, 0.5],
        origin: DVec3::ZERO,
        view_proj: DMat4::IDENTITY,
        scene_faces: Cached {
            generation: 1,
            data: Arc::new(tri(0.0, red, blue)),
        },
        ..Default::default()
    };
    frame.draws.push(Draw::new(Source::SceneFaces, 0, 3).bias());
    r.render(&device, &queue, &frame);
    let px = r.read_pixels(&device, &queue).unwrap();
    assert_eq!(pixel(&px, 32, 34), [255, 0, 0, 255]); // the front, counter-clockwise
    assert_eq!(pixel(&px, 2, 2), [128, 128, 128, 255]); // the background
                                                        // Seen from behind (mirrored view): the back colour.
    frame.view_proj = DMat4::from_scale(DVec3::new(-1.0, 1.0, 1.0));
    r.render(&device, &queue, &frame);
    let px = r.read_pixels(&device, &queue).unwrap();
    assert_eq!(pixel(&px, 32, 34), [0, 0, 255, 255]);
    // The selection stipple leaves most pixels to what is under it.
    frame.view_proj = DMat4::IDENTITY;
    frame.draws = vec![Draw::new(Source::SceneFaces, 0, 3)
        .depth(Depth::TestOnly)
        .stipple(Stipple::Selection)];
    r.render(&device, &queue, &frame);
    let px = r.read_pixels(&device, &queue).unwrap();
    let reds = (20..44)
        .flat_map(|x| (24..40).map(move |y| (x, y)))
        .filter(|&(x, y)| pixel(&px, x, y) == [255, 0, 0, 255])
        .count();
    assert!(reds > 0 && reds < 24 * 16 / 4, "{reds}");
}

#[test]
fn test_the_depth_of_a_multisampled_frame_reads_back() {
    let Some((device, queue)) = device() else {
        eprintln!("no wgpu adapter: skipped");
        return;
    };
    let mut r = ViewportRenderer::new(&device, 4);
    let grey = [0.5, 0.5, 0.5, 1.0];
    let mut frame = FrameData {
        size: [64, 64],
        view_proj: DMat4::IDENTITY,
        scene_faces: Cached {
            generation: 1,
            data: Arc::new(tri(0.0, grey, grey)),
        },
        ..Default::default()
    };
    frame.draws.push(Draw::new(Source::SceneFaces, 0, 3));
    r.render(&device, &queue, &frame);
    let (w, h, depth) = r.read_depth(&device, &queue).unwrap();
    assert_eq!((w, h, depth.len()), (64, 64, 64 * 64));
    // GL z = 0 is window depth 0.5; the background is the far plane
    assert!((depth[(34 * 64 + 32) as usize] - 0.5).abs() < 1e-5);
    assert_eq!(depth[2 * 64 + 2], 1.0);
}
