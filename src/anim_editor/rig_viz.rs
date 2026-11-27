//! 3D Viewport Microbone Visualization, FloorGrid, and SelectionBox adornments
//! for RusTudio's native `AnimationClipEditor`.
//!
//! Derived from:
//! - `Util/RigVisualization` & `Util/Adorn`
//! - `Components/FloorGrid`
//! - `Components/InstanceSelector` & `Util/DraggerSchema/getSelectionBoxComponent`

use std::collections::{HashMap, HashSet};

use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use glam::{Mat3, Vec3};
use rbx_dom_weak::types::{Ref, Variant};

use rustudio_core::doc::{self, Document};
use rustudio_core::math::{cf_mul, Ray, Xform};
use rustudio_render::camera::Camera;
use rustudio_render::render::LineVertex;
use rustudio_render::scene::Scene;

use crate::anim_editor::constants::{
    BONE_LINK_TRANSPARENCY, BONE_NODE_TRANSPARENCY, BONE_TRANSPARENCY_DEFAULT,
    BONE_TRANSPARENCY_HOVER, LENGTH_TO_RADIUS_RATIO,
};
use crate::anim_editor::state::{EditorState, RigCache};
use crate::anim_editor::theme::{
    BONE_COLOR_DEFAULT, BONE_COLOR_HOVER, BONE_COLOR_SELECTED, BONE_LINK_COLOR, BONE_NODE_COLOR,
};

const IDENTITY_XFORM: Xform = Xform {
    pos: Vec3::ZERO,
    rot: Mat3::IDENTITY,
};

const BOX_EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

/// Visual representation of a single `Bone` in world space for viewport
/// microbone rendering and ray-picking (`RigVisualization`).
#[derive(Debug, Clone)]
pub struct BoneVisual {
    pub name: String,
    pub bone_ref: Ref,
    pub world_pos: Vec3,
    pub parent_pos: Option<Vec3>,
    pub radius: f32,
}

/// Reads an instance's CFrame property (defaulting to identity if missing).
pub(crate) fn read_cframe_prop(doc: &Document, r: Ref, prop_name: &str) -> Xform {
    match doc.prop(r, prop_name) {
        Some(Variant::CFrame(cf)) => Xform::from_cframe(&cf).orthonormalized(),
        _ => IDENTITY_XFORM,
    }
}

/// Computes a `Bone`'s `TransformedWorldCFrame` by walking its ancestor
/// `Bone` / `Attachment` chain up to the hosting `BasePart`
/// (`parentWorld * bone.CFrame * bone.Transform`).
pub fn bone_transformed_world_cframe(doc: &Document, bone_ref: Ref) -> Xform {
    let mut chain = Vec::new();
    let mut cur = bone_ref;
    let base_world = loop {
        let Some(inst) = doc.dom.get_by_ref(cur) else {
            break IDENTITY_XFORM;
        };
        if doc::is_a(&inst.class, "BasePart") {
            if inst.class == "Terrain" {
                break IDENTITY_XFORM;
            }
            break doc::xform_of(doc, cur).unwrap_or(IDENTITY_XFORM);
        }
        if inst.class == "Bone" || doc::is_a(&inst.class, "Attachment") {
            chain.push((cur, inst.class == "Bone"));
        }
        cur = inst.parent();
    };

    let mut world = base_world;
    for &(r, is_bone) in chain.iter().rev() {
        let local_cf = read_cframe_prop(doc, r, "CFrame");
        world = cf_mul(&world, &local_cf);
        if is_bone {
            let transform = read_cframe_prop(doc, r, "Transform");
            world = cf_mul(&world, &transform);
        }
    }
    world
}

/// Computes the parent world CFrame for a `Bone`, matching
/// `TransformHandlesImplementation.getParentWorldCFrame`:
/// - Nearest ancestor `Bone` / `Attachment` -> its `TransformedWorldCFrame`
/// - Nearest ancestor `BasePart` -> its `CFrame`
/// - Otherwise identity.
pub fn bone_parent_world_cframe(doc: &Document, bone_ref: Ref) -> Xform {
    let Some(inst) = doc.dom.get_by_ref(bone_ref) else {
        return IDENTITY_XFORM;
    };
    let mut up = inst.parent();
    while let Some(p) = doc.dom.get_by_ref(up) {
        if p.class == "Bone" || doc::is_a(&p.class, "Attachment") {
            return bone_transformed_world_cframe(doc, up);
        }
        if doc::is_a(&p.class, "BasePart") {
            if p.class == "Terrain" {
                return IDENTITY_XFORM;
            }
            return doc::xform_of(doc, up).unwrap_or(IDENTITY_XFORM);
        }
        up = p.parent();
    }
    IDENTITY_XFORM
}

/// Collects world-space microbone visuals for every `Bone` in `rig.bone_map`
/// (`RigVisualization`).
///
/// Each bone's node sphere radius is `(parent_dist / 15.0).clamp(0.05, 0.5)`
/// (`LENGTH_TO_RADIUS_RATIO = 15.0`).
pub fn collect_bone_visuals(doc: &Document, rig: &RigCache) -> Vec<BoneVisual> {
    if rig.bone_map.is_empty() {
        return Vec::new();
    }

    let mut entries: Vec<(&String, Ref)> = rig.bone_map.iter().map(|(k, &v)| (k, v)).collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    let mut world_positions: HashMap<Ref, Vec3> = HashMap::with_capacity(entries.len());
    for &(_, bone_ref) in &entries {
        let xf = bone_transformed_world_cframe(doc, bone_ref);
        world_positions.insert(bone_ref, xf.pos);
    }

    let mut out = Vec::with_capacity(entries.len());
    for (name, bone_ref) in entries {
        let world_pos = world_positions
            .get(&bone_ref)
            .copied()
            .unwrap_or(Vec3::ZERO);

        let parent_pos = doc.dom.get_by_ref(bone_ref).and_then(|inst| {
            let parent_ref = inst.parent();
            let parent_inst = doc.dom.get_by_ref(parent_ref)?;
            if parent_inst.class == "Bone" {
                world_positions
                    .get(&parent_ref)
                    .copied()
                    .or_else(|| Some(bone_transformed_world_cframe(doc, parent_ref).pos))
            } else {
                None
            }
        });

        let parent_dist = parent_pos.map(|p| (world_pos - p).length()).unwrap_or(0.0);
        let radius = (parent_dist / LENGTH_TO_RADIUS_RATIO).clamp(0.05, 0.5);

        out.push(BoneVisual {
            name: name.clone(),
            bone_ref,
            world_pos,
            parent_pos,
            radius,
        });
    }

    out
}

/// Ray-sphere intersection matching `DraggerFramework.Utility.Math.intersectRaySphere`.
fn intersect_ray_sphere(origin: Vec3, dir: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    if radius <= 0.0 {
        return None;
    }
    let oc = origin - center;
    let b = oc.dot(dir);
    let c = oc.dot(oc) - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let sqrt_d = disc.sqrt();
    let t0 = -b - sqrt_d;
    if t0 >= 0.0 {
        return Some(t0);
    }
    let t1 = -b + sqrt_d;
    if t1 >= 0.0 {
        return Some(t1);
    }
    None
}

/// Closest parameters `(t1, t2)` between two rays `(p1, d1)` and `(p2, d2)`
/// matching `DraggerFramework.Utility.Math.intersectRayRay`.
fn intersect_ray_ray(p1: Vec3, d1: Vec3, p2: Vec3, d2: Vec3) -> Option<(f32, f32)> {
    let w0 = p1 - p2;
    let a = d1.dot(d1);
    let b = d1.dot(d2);
    let c = d2.dot(d2);
    let d = d1.dot(w0);
    let e = d2.dot(w0);
    let denom = a * c - b * b;
    if denom.abs() < 1e-6 {
        return None;
    }
    let t1 = (b * e - c * d) / denom;
    let t2 = (a * e - b * d) / denom;
    Some((t1, t2))
}

/// Ray-picks the closest microbone sphere or link cone matching
/// `RigVisualization.getBoneHit(mouseRay, rootInstance)`.
pub fn pick_bone_hit(ray: &Ray, bones: &[BoneVisual]) -> Option<String> {
    let ray_dir = ray.dir.normalize_or_zero();
    if ray_dir.length_squared() < 1e-8 {
        return None;
    }

    let mut best: Option<(f32, String)> = None;

    for bone in bones {
        // 1. Node sphere test (`SphereHandleAdornment`).
        if let Some(t) = intersect_ray_sphere(ray.origin, ray_dir, bone.world_pos, bone.radius) {
            if best.as_ref().is_none_or(|(bt, _)| t < *bt) {
                best = Some((t, bone.name.clone()));
            }
        }

        // 2. Link cone test (`ConeHandleAdornment` from parent -> child).
        if let Some(p_pos) = bone.parent_pos {
            let delta = bone.world_pos - p_pos;
            let height = delta.length();
            if height > 1e-5 {
                let cone_dir = delta / height;
                let cone_radius = (height / LENGTH_TO_RADIUS_RATIO).clamp(0.05, 0.5);
                if let Some((t_cone, t_ray)) =
                    intersect_ray_ray(p_pos, cone_dir, ray.origin, ray_dir)
                {
                    if t_ray >= 0.0 && t_cone > 0.0 && t_cone < height {
                        let p_cone = p_pos + cone_dir * t_cone;
                        let p_ray = ray.origin + ray_dir * t_ray;
                        let dist = (p_cone - p_ray).length();
                        if dist < cone_radius && best.as_ref().is_none_or(|(bt, _)| t_ray < *bt) {
                            // In `RigVisualization`, `<Parent>To<Child>` has
                            // `BoneRef = parent`; fall back to child name if parent
                            // visual isn't present.
                            let owner_name = bones
                                .iter()
                                .find(|cand| (cand.world_pos - p_pos).length_squared() < 1e-6)
                                .map(|cand| cand.name.clone())
                                .unwrap_or_else(|| bone.name.clone());
                            best = Some((t_ray, owner_name));
                        }
                    }
                }
            }
        }
    }

    best.map(|(_, name)| name)
}

#[inline]
fn color_with_transparency(color: Color32, transparency: f32) -> Color32 {
    let alpha = ((1.0 - transparency.clamp(0.0, 1.0)) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

/// Draws AlwaysOnTop microbone spheres, tapered link cones, and link center
/// lines (`RigVisualization`).
pub fn draw_bone_visualization(
    painter: &egui::Painter,
    rect: Rect,
    cam: &Camera,
    bones: &[BoneVisual],
    selected: &HashSet<String>,
    hovered: Option<&str>,
) {
    if bones.is_empty() {
        return;
    }

    // Pass 1: Bone link cones + center lines (drawn behind node spheres).
    for bone in bones {
        let Some(p_pos) = bone.parent_pos else {
            continue;
        };
        let (Some(p_px), Some(c_px)) =
            (cam.project(rect, p_pos), cam.project(rect, bone.world_pos))
        else {
            continue;
        };

        // Determine link owner (parent bone if present, else child).
        let owner_name = bones
            .iter()
            .find(|cand| (cand.world_pos - p_pos).length_squared() < 1e-6)
            .map(|cand| cand.name.as_str())
            .unwrap_or(bone.name.as_str());

        let is_sel = selected.contains(owner_name);
        let is_hov = !is_sel && hovered == Some(owner_name);

        let (cone_base_color, cone_transp, line_color) = if is_sel {
            (
                BONE_COLOR_SELECTED,
                BONE_TRANSPARENCY_DEFAULT,
                BONE_COLOR_SELECTED,
            )
        } else if is_hov {
            (BONE_COLOR_HOVER, BONE_TRANSPARENCY_HOVER, BONE_COLOR_HOVER)
        } else {
            (BONE_COLOR_DEFAULT, BONE_LINK_TRANSPARENCY, BONE_LINK_COLOR)
        };

        let seg = c_px - p_px;
        let seg_len = seg.length();
        if seg_len > 1.0 {
            let perp = Vec2::new(-seg.y, seg.x) / seg_len;
            let wpp = cam.world_per_px(rect, p_pos).max(1e-5);
            let base_r_px = (bone.radius / wpp).clamp(2.5, 18.0);
            let left = p_px + perp * base_r_px;
            let right = p_px - perp * base_r_px;

            let fill = color_with_transparency(cone_base_color, cone_transp);
            let outline = color_with_transparency(cone_base_color, (cone_transp * 0.5).max(0.0));
            painter.add(egui::Shape::convex_polygon(
                vec![left, right, c_px],
                fill,
                Stroke::new(1.0, outline),
            ));
        }

        // Center link line (`LineHandleAdornment`, `Thickness = 1`).
        painter.line_segment(
            [p_px, c_px],
            Stroke::new(
                1.0,
                color_with_transparency(line_color, BONE_TRANSPARENCY_DEFAULT),
            ),
        );
    }

    // Pass 2: Joint node spheres (`SphereHandleAdornment`).
    for bone in bones {
        let Some(center_px) = cam.project(rect, bone.world_pos) else {
            continue;
        };
        let is_sel = selected.contains(&bone.name);
        let is_hov = !is_sel && hovered == Some(bone.name.as_str());

        let (base_color, transp, outline_color) = if is_sel {
            (BONE_COLOR_SELECTED, 0.1, Color32::WHITE)
        } else if is_hov {
            (BONE_COLOR_HOVER, BONE_TRANSPARENCY_HOVER, Color32::WHITE)
        } else {
            (BONE_COLOR_DEFAULT, BONE_NODE_TRANSPARENCY, BONE_NODE_COLOR)
        };

        let wpp = cam.world_per_px(rect, bone.world_pos).max(1e-5);
        let r_px = (bone.radius / wpp).clamp(3.5, 20.0);

        painter.circle(
            center_px,
            r_px,
            color_with_transparency(base_color, transp),
            Stroke::new(if is_sel || is_hov { 1.5 } else { 1.0 }, outline_color),
        );
    }
}

/// Computes world-space `(min, max)` bounds of a rig's `BasePart`s.
fn rig_world_bounds(doc: &Document, rig: &RigCache) -> Option<(Vec3, Vec3)> {
    let mut min_pt = Vec3::splat(f32::INFINITY);
    let mut max_pt = Vec3::splat(f32::NEG_INFINITY);
    let mut found = false;

    let mut visit_part = |r: Ref| {
        let Some(xf) = doc::xform_of(doc, r) else {
            return;
        };
        let size = doc::vec3_of(doc.prop(r, "Size")).unwrap_or(Vec3::ONE);
        let h = size * 0.5;
        let m = xf.rot;
        let ext = Vec3::new(
            m.x_axis.x.abs() * h.x + m.y_axis.x.abs() * h.y + m.z_axis.x.abs() * h.z,
            m.x_axis.y.abs() * h.x + m.y_axis.y.abs() * h.y + m.z_axis.y.abs() * h.z,
            m.x_axis.z.abs() * h.x + m.y_axis.z.abs() * h.y + m.z_axis.z.abs() * h.z,
        );
        min_pt = min_pt.min(xf.pos - ext);
        max_pt = max_pt.max(xf.pos + ext);
        found = true;
    };

    if let Some(root_part) = rig.root_part_ref {
        visit_part(root_part);
    }
    for &r in rig.part_refs.values() {
        visit_part(r);
    }

    found.then_some((min_pt, max_pt))
}

/// Appends the 22 scrolling FloorGrid `LineVertex` lines (44 vertices) at the
/// rig's feet height, matching `Components/FloorGrid`:
/// - `gridSize = max(extents.x, extents.z) + 3.0`
/// - `spacing = gridSize / 10.0`
/// - `yOffset = extents.y * 0.5 + (rootPart.Position.y - modelCenter.y)`
/// - 11 Z-lines + 11 X-lines (`i = -5..=5`), scrolled along the rig's forward
///   axis by `grid_offset`.
pub fn append_floor_grid_lines(
    lines: &mut Vec<LineVertex>,
    doc: &Document,
    rig: &RigCache,
    grid_offset: f32,
) {
    let root_xf = rig
        .root_part_ref
        .and_then(|r| doc::xform_of(doc, r))
        .or(rig.root_cframe)
        .unwrap_or(IDENTITY_XFORM);

    let (extents, model_center) = match rig_world_bounds(doc, rig) {
        Some((mn, mx)) => (mx - mn, (mn + mx) * 0.5),
        None => (Vec3::splat(4.0), root_xf.pos),
    };

    let grid_size = extents.x.max(extents.z) + 3.0;
    let spacing = grid_size / 10.0;
    let half = grid_size * 0.5;
    let y_offset = extents.y * 0.5 + (root_xf.pos.y - model_center.y);
    let base_offset = Vec3::new(0.0, -y_offset, 0.0);
    let scroll = if spacing > 1e-5 {
        grid_offset.rem_euclid(spacing)
    } else {
        0.0
    };

    // Studio grid theme line color (subtle neutral gray).
    let color = [0.62, 0.62, 0.62, 0.65];

    // 11 Z-lines (`i = -5..=5`): parallel to rig Z axis, shifted by `scroll`.
    for i in -5..=5 {
        let x = i as f32 * spacing;
        let a = root_xf.apply(base_offset + Vec3::new(x, 0.0, -half + scroll));
        let b = root_xf.apply(base_offset + Vec3::new(x, 0.0, half + scroll));
        lines.push(LineVertex {
            pos: a.to_array(),
            color,
        });
        lines.push(LineVertex {
            pos: b.to_array(),
            color,
        });
    }

    // 11 X-lines (`i = -5..=5`): perpendicular to rig Z axis, shifted by `scroll`.
    for i in -5..=5 {
        let z = i as f32 * spacing + scroll;
        let a = root_xf.apply(base_offset + Vec3::new(-half, 0.0, z));
        let b = root_xf.apply(base_offset + Vec3::new(half, 0.0, z));
        lines.push(LineVertex {
            pos: a.to_array(),
            color,
        });
        lines.push(LineVertex {
            pos: b.to_array(),
            color,
        });
    }
}

fn box_corners(xf: &Xform, size: Vec3) -> [Vec3; 8] {
    let h = size * 0.5;
    let mut out = [Vec3::ZERO; 8];
    for (i, o) in out.iter_mut().enumerate() {
        let local = Vec3::new(
            if i & 1 == 0 { -h.x } else { h.x },
            if i & 2 == 0 { -h.y } else { h.y },
            if i & 4 == 0 { -h.z } else { h.z },
        );
        *o = xf.apply(local);
    }
    out
}

/// Draws a 3D OBB wireframe onto `painter` using `cam`.
fn draw_obb_wireframe(
    painter: &egui::Painter,
    rect: Rect,
    cam: &Camera,
    xf: &Xform,
    size: Vec3,
    stroke: Stroke,
) {
    let corners = box_corners(xf, size);
    let projected: [Option<Pos2>; 8] = corners.map(|c| cam.project(rect, c));
    for (a, b) in BOX_EDGES {
        if let (Some(pa), Some(pb)) = (projected[a], projected[b]) {
            painter.line_segment([pa, pb], stroke);
        }
    }
}

fn part_xf_and_size(scene: &Scene, doc: &Document, r: Ref) -> Option<(Xform, Vec3)> {
    if let Some(sp) = scene.get(r) {
        return Some((sp.xf, sp.size));
    }
    let xf = doc::xform_of(doc, r)?;
    let size = doc::vec3_of(doc.prop(r, "Size")).unwrap_or(Vec3::ONE);
    Some((xf, size))
}

/// Draws a `LineThickness = 2` hover SelectionBox around a candidate rig `Model`
/// matching `InstanceSelector`'s `HoverBox`.
pub fn draw_rig_hover_box(
    painter: &egui::Painter,
    rect: Rect,
    cam: &Camera,
    scene: &Scene,
    doc: &Document,
    model_ref: Ref,
) {
    let mut min_pt = Vec3::splat(f32::INFINITY);
    let mut max_pt = Vec3::splat(f32::NEG_INFINITY);
    let mut found = false;

    for desc in doc.dom.descendants_of(model_ref) {
        if !doc::is_a(&desc.class, "BasePart") {
            continue;
        }
        let Some((xf, size)) = part_xf_and_size(scene, doc, desc.referent()) else {
            continue;
        };
        let h = size * 0.5;
        let m = xf.rot;
        let ext = Vec3::new(
            m.x_axis.x.abs() * h.x + m.y_axis.x.abs() * h.y + m.z_axis.x.abs() * h.z,
            m.x_axis.y.abs() * h.x + m.y_axis.y.abs() * h.y + m.z_axis.y.abs() * h.z,
            m.x_axis.z.abs() * h.x + m.y_axis.z.abs() * h.y + m.z_axis.z.abs() * h.z,
        );
        min_pt = min_pt.min(xf.pos - ext);
        max_pt = max_pt.max(xf.pos + ext);
        found = true;
    }

    if found {
        let center = (min_pt + max_pt) * 0.5;
        let size = max_pt - min_pt;
        let xf = Xform {
            pos: center,
            rot: Mat3::IDENTITY,
        };
        let hover_color = Color32::from_rgb(0, 162, 255);
        draw_obb_wireframe(painter, rect, cam, &xf, size, Stroke::new(2.0, hover_color));
    }
}

/// Draws 3D OBB wireframe SelectionBoxes matching `InstanceSelector` &
/// `getSelectionBoxComponent`:
/// - Selected rig parts: `LineThickness = 1`, `alpha = 0.5`
/// - Hovered rig candidate when no clip is active: `LineThickness = 2`
/// - Suppresses SelectionBox on `Bone` instances
pub fn draw_selection_boxes(
    painter: &egui::Painter,
    rect: Rect,
    cam: &Camera,
    scene: &Scene,
    doc: &Document,
    rig: Option<&RigCache>,
    state: &EditorState,
) {
    // Selected rig BaseParts: LineThickness = 1, alpha = 0.5.
    let sel_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 162, 255, 128));
    for &inst_ref in &state.selected_track_instances {
        // Suppress SelectionBox on Bone instances (`getSelectionBoxComponent`).
        if doc.class_of(inst_ref).is_some_and(|c| c == "Bone") {
            continue;
        }
        if let Some((xf, size)) = part_xf_and_size(scene, doc, inst_ref) {
            draw_obb_wireframe(painter, rect, cam, &xf, size, sel_stroke);
        }
    }

    // Hovered rig part (when a rig & clip are active) or hovered rig candidate
    // (when no clip is active).
    if let Some(ref hovered_name) = state.viewport_dragger.hovered_part {
        if let Some(rig) = rig.filter(|_| state.animation_data.is_some()) {
            // Suppress SelectionBox on bones.
            if !rig.bone_map.contains_key(hovered_name) {
                let part_ref = rig
                    .joints_by_part
                    .get(hovered_name)
                    .and_then(|j| j.part1_ref)
                    .or_else(|| rig.part_refs.get(hovered_name).copied());
                if let Some(r) = part_ref {
                    if let Some((xf, size)) = part_xf_and_size(scene, doc, r) {
                        let hover_stroke =
                            Stroke::new(1.5, Color32::from_rgba_unmultiplied(102, 204, 255, 204));
                        draw_obb_wireframe(painter, rect, cam, &xf, size, hover_stroke);
                    }
                }
            }
        } else if let Some(ws) = doc.workspace() {
            // No active clip: look for hovered rig Model candidate under Workspace.
            for desc in doc.dom.descendants_of(ws) {
                if desc.class == "Model" && desc.name == *hovered_name {
                    draw_rig_hover_box(painter, rect, cam, scene, doc, desc.referent());
                    break;
                }
            }
        }
    }
}
