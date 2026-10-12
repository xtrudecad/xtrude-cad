// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Helpers shared by the app tests.

use xtrude_model::units;

mod probe;
pub use probe::FakeProbe;

/// Bare numbers are metres whatever scene is bound.
pub struct MetreDocument;

impl MetreDocument {
    pub fn new() -> MetreDocument {
        units::set_override(Some(units::legacy_document_units()));
        MetreDocument
    }
}

impl Drop for MetreDocument {
    fn drop(&mut self) {
        units::set_override(None);
    }
}

pub fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 + 1e-6 * b.abs()
}

use glam::DVec3;
use xtrude_app::input::{Button, Modifiers, MouseEvent};
use xtrude_app::viewport::Viewport;

pub fn v3(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

/// A headless viewport with settings isolated for this thread.
pub fn viewport() -> Viewport {
    xtrude_model::settings::use_memory_store();
    Viewport::new()
}

/// The six faces of an axis-aligned box into the active mesh.
pub fn add_box(vp: &mut Viewport, lo: DVec3, hi: DVec3) {
    let (x0, y0, z0, x1, y1, z1) = (lo.x, lo.y, lo.z, hi.x, hi.y, hi.z);
    let p = [
        v3(x0, y0, z0),
        v3(x1, y0, z0),
        v3(x1, y1, z0),
        v3(x0, y1, z0),
        v3(x0, y0, z1),
        v3(x1, y0, z1),
        v3(x1, y1, z1),
        v3(x0, y1, z1),
    ];
    for idx in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        let pts: Vec<DVec3> = idx.iter().map(|&i| p[i]).collect();
        vp.scene.mesh_mut().add_face(&pts, &[]);
    }
    vp.scene.version += 1;
}

/// Press, six moves and release of `button` from `(x0, y0)` to `(x1, y1)`.
pub fn drag(vp: &mut Viewport, x0: f64, y0: f64, x1: f64, y1: f64, button: Button) {
    vp.mouse_press(&MouseEvent::button(x0, y0, button, true));
    let steps = 6;
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        vp.mouse_move(&MouseEvent::drag(
            x0 + (x1 - x0) * t,
            y0 + (y1 - y0) * t,
            button,
        ));
    }
    vp.mouse_release(&MouseEvent::button(x1, y1, button, false));
}

/// A left click (press + release) at `(x, y)`.
pub fn click(vp: &mut Viewport, x: f64, y: f64, m: Modifiers) {
    vp.mouse_press(&MouseEvent::button(x, y, Button::Left, true).with_modifiers(m));
    vp.mouse_release(&MouseEvent::button(x, y, Button::Left, false).with_modifiers(m));
}

/// A hover (no button) at `(x, y)`.
pub fn hover(vp: &mut Viewport, x: f64, y: f64) {
    vp.mouse_move(&MouseEvent::at(x, y));
}

/// The screen position of a world point (the raw NDC map).
pub fn screen(vp: &Viewport, p: DVec3) -> (f64, f64) {
    use xtrude_core::math::MatExt;
    let cam = &vp.camera;
    let ndc = (cam.projection_matrix() * cam.view_matrix()).project_point(p);
    (
        (ndc.x + 1.0) * 0.5 * vp.width(),
        (1.0 - ndc.y) * 0.5 * vp.height(),
    )
}

/// A viewport stand-in without projection.
pub fn stub() -> Viewport {
    xtrude_model::settings::use_memory_store();
    Viewport::stub()
}

/// Push a loose face as the Push/Pull tool commits it.
pub fn push(vp: &mut Viewport, face: xtrude_core::FaceId, dist: f64, keep: bool) -> bool {
    let mid = vp.scene.active_mesh_id();
    xtrude_model::pushpull::push_face(&mut vp.scene, &mut vp.history, mid, face, dist, keep)
}

/// `dist` signed so the push goes along +Z.
pub fn up(vp: &Viewport, f: xtrude_core::FaceId, dist: f64) -> f64 {
    if vp.scene.mesh().face_normal(f).z > 0.0 {
        dist
    } else {
        -dist
    }
}

/// A rectangle as the Rectangle tool commits it.
pub fn draw_rect(vp: &mut Viewport, corners: &[DVec3]) {
    let segs: Vec<(DVec3, DVec3)> = (0..corners.len())
        .map(|i| (corners[i], corners[(i + 1) % corners.len()]))
        .collect();
    let cmd = xtrude_model::edits::build_add_edges(
        &vp.scene,
        &segs,
        false,
        vec![Box::new(xtrude_model::commands::AddFaceCommand::new(
            corners.to_vec(),
        ))],
        None,
    );
    vp.execute_boxed(cmd);
}

/// Faces of the active mesh with `n` corners.
pub fn faces_with(vp: &Viewport, n: usize) -> Vec<xtrude_core::FaceId> {
    let m = vp.scene.mesh();
    m.face_ids()
        .into_iter()
        .filter(|&f| m.face(f).outer.len() == n)
        .collect()
}

/// A scratch directory removed when dropped.
pub struct TmpDir(pub std::path::PathBuf);

impl TmpDir {
    pub fn new() -> TmpDir {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "xtrude-app-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&p).expect("temp dir");
        TmpDir(p)
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }

    pub fn join(&self, name: &str) -> std::path::PathBuf {
        self.0.join(name)
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Make a loose edge the hovered edge (`viewport._hover_edge = e`).
pub fn set_hover_edge(vp: &mut Viewport, e: xtrude_core::EdgeId) {
    let mid = vp.scene.active_mesh_id();
    let m = vp.scene.mesh();
    let (a, b) = m.edge_ab(e);
    let ed = m.edge(e);
    vp.inference.hover_edge = Some(xtrude_app::viewport::PickedEdge {
        a,
        b,
        edge: Some((mid, e)),
        in_group: false,
        context: None,
        group: None,
        soft: ed.soft,
        curve: ed.curve,
    });
}

/// The loose edge between two points.
pub fn edge_at(vp: &Viewport, a: DVec3, b: DVec3) -> xtrude_core::EdgeId {
    let m = vp.scene.mesh();
    let (va, vb) = (
        m.vertex_at(a).expect("vertex a"),
        m.vertex_at(b).expect("vertex b"),
    );
    m.find_edge(va, vb).expect("edge")
}

/// A test viewport of the given size, the camera's aspect matching.
pub fn sized_viewport(w: f64, h: f64) -> Viewport {
    let mut vp = viewport();
    vp.resize(w, h);
    vp.camera.set_aspect(w as i32, h as i32);
    vp
}

/// A main window on a memory settings store.
pub fn window() -> xtrude_app::window::MainWindow {
    xtrude_model::settings::use_memory_store();
    xtrude_app::window::MainWindow::new()
}

/// The text of every top-level context-menu entry.
pub fn context_texts(w: &xtrude_app::window::MainWindow) -> Vec<String> {
    w.context_menu()
        .iter()
        .map(|i| i.text().to_string())
        .collect()
}

/// One corner of a drawn face triangle, back in world space.
#[derive(Clone, Copy, Debug)]
pub struct Corner {
    pub pos: DVec3,
    pub front: [f32; 4],
    pub back: [f32; 4],
}

/// The triangles a range of a frame's scene faces draws, positions back
/// in world space.
pub fn frame_triangles(
    f: &xtrude_render::frame::FrameData,
    first: u32,
    count: u32,
) -> Vec<[Corner; 3]> {
    let o = f.origin;
    let corners: Vec<Corner> = f.scene_faces.data[first as usize..(first + count) as usize]
        .iter()
        .map(|fv| Corner {
            pos: o + DVec3::new(fv.pos[0] as f64, fv.pos[1] as f64, fv.pos[2] as f64),
            front: fv.front,
            back: fv.back,
        })
        .collect();
    corners
        .chunks_exact(3)
        .map(|t| [t[0], t[1], t[2]])
        .collect()
}

/// Every scene-face triangle of the viewport's next frame.
pub fn scene_triangles(vp: &mut Viewport) -> Vec<[Corner; 3]> {
    let f = vp.frame_data([200, 200], false);
    frame_triangles(&f, 0, f.scene_faces.data.len() as u32)
}

/// A colour rounded to 1/1000, for set comparisons.
pub fn rgb_key(c: [f32; 4]) -> [i32; 3] {
    [0, 1, 2].map(|i| (c[i] * 1000.0).round() as i32)
}

/// The wgpu pass, headless: `None` where the machine offers no adapter,
/// and the test skips.
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub renderer: xtrude_render::gpu::ViewportRenderer,
}

/// RGBA pixels, rows top to bottom.
pub type Pixels = (u32, u32, Vec<u8>);

impl Gpu {
    /// 4× multisampling, as the viewport draws.
    pub fn new() -> Option<Gpu> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            ..Default::default()
        }))
        .ok()?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        let renderer = xtrude_render::gpu::ViewportRenderer::new(&device, 4);
        Some(Gpu {
            device,
            queue,
            renderer,
        })
    }

    /// The frame on screen (`grabFramebuffer`): drawn at the viewport's
    /// size, its depth read back when the annotations need it, as the
    /// shell does.
    pub fn grab(&mut self, vp: &mut Viewport) -> Pixels {
        let size = [vp.width() as u32, vp.height() as u32];
        let frame = vp.frame_data(size, false);
        self.renderer.render(&self.device, &self.queue, &frame);
        if vp.annotations_need_depth() {
            self.read_depth(vp);
        }
        self.renderer
            .read_pixels(&self.device, &self.queue)
            .expect("read-back")
    }

    /// What the shell does before a wheel notch or a press: hand the depth
    /// of the frame on screen to the viewport if it wants it.
    pub fn depth_for_input(&mut self, vp: &mut Viewport) {
        if vp.wants_depth() {
            self.read_depth(vp);
        }
    }

    /// What the shell does before each input: hand the depth of the frame
    /// on screen to the viewport if that input wants it. Whether it read.
    pub fn depth_before(&mut self, vp: &mut Viewport, ev: &xtrude_app::input::Input) -> bool {
        let wants = vp.wants_depth_for(ev);
        if wants {
            self.read_depth(vp);
        }
        wants
    }

    fn read_depth(&mut self, vp: &mut Viewport) {
        if let Some((w, h, depth)) = self.renderer.read_depth(&self.device, &self.queue) {
            vp.set_depth_snap(depth, w, h);
        }
    }

    /// An image render without overlays (`render_image(w, h,
    /// overlays=False)`): the viewport takes the image's size.
    pub fn render_image(&mut self, vp: &mut Viewport, w: u32, h: u32) -> Pixels {
        vp.resize_gl(w as f64, h as f64);
        let frame = vp.frame_data([w, h], true);
        self.renderer.render(&self.device, &self.queue, &frame);
        self.renderer
            .read_pixels(&self.device, &self.queue)
            .expect("read-back")
    }
}

/// The RGB of pixel `(x, y)`, 0–255.
pub fn rgb_at(px: &Pixels, x: i64, y: i64) -> [i64; 3] {
    let (x, y) = (x.clamp(0, px.0 as i64 - 1), y.clamp(0, px.1 as i64 - 1));
    let i = ((y as u32 * px.0 + x as u32) * 4) as usize;
    [px.2[i], px.2[i + 1], px.2[i + 2]].map(|c| c as i64)
}

/// Pixels that differ between two images by more than `8` summed over
/// RGB.
pub fn changed(a: &Pixels, b: &Pixels) -> usize {
    a.2.chunks(4)
        .zip(b.2.chunks(4))
        .filter(|(p, q)| {
            (0..3)
                .map(|i| (p[i] as i64 - q[i] as i64).abs())
                .sum::<i64>()
                > 8
        })
        .count()
}

/// The summed absolute RGB difference of two images.
pub fn contrast(a: &Pixels, b: &Pixels) -> i64 {
    a.2.chunks(4)
        .zip(b.2.chunks(4))
        .map(|(p, q)| {
            (0..3)
                .map(|i| (p[i] as i64 - q[i] as i64).abs())
                .sum::<i64>()
        })
        .sum()
}
