// Studio's light grid skylight and local lights (LightGridCPU, "LightGrid Chunk Skylight 3D"),
// read from Studio (Studio measurements "Light grid
// (skylight)") and checked against a running Studio's chunks
// (/proc/<pid>/mem): parts voxelize bit for bit, the skylight kernel matches
// to the voxel except where Studio's own chunk-by-chunk update is still
// stale (≤ 6 levels above ground in the test scenes).
//
// - The grid is 8×4×8 chunks of 32×16×32 four-stud voxels (1024×256×1024
//   studs) that follows the camera with a one-chunk hysteresis
//   (FUN_144cd9980).
// - Each voxel has three occupancy bytes (FUN_146656750 and the shape
// voxelizers…): the part's cover of the voxel
//   seen along X, along Y and along Z (equal unless the part is thinner
//   than a voxel), × (1 − Transparency) × 255.5, summed saturating; terrain
//   adds its occupancy byte (not Air or Water) to all three (FUN_146657c10).
// - Skylight (FUN_144cd3c80) runs down from the top: a voxel receives the
//   rounded-up mean of the 8 voxels around the one above it (not that one),
//   passes on that minus max(max(occX, occZ)·180 >> 8, occY), and stores
//   252 − (255 − received)⁴ >> 24. Missing neighbours (outside the grid)
//   are 255.
// - The shaders read it at surface + 6 studs × normal (DefaultUnified*VS,
//   SmoothClusterVS) as a two-channel unorm texture: .x skylight, .y sun
//   visibility, computed by sunlight().

use glam::Vec3;

use crate::mech;
use crate::terrain::{Voxels, WATER};

/// Voxels per chunk (X, Y, Z).
pub const CHUNK: [usize; 3] = [32, 16, 32];

/// The grid's size in voxels: (X, Y, Z).
pub fn dims() -> [usize; 3] {
    let c = mech::LIGHTGRID_CHUNKS;
    [c[0] * CHUNK[0], c[1] * CHUNK[1], c[2] * CHUNK[2]]
}

/// The skylight per voxel, two bytes each (skylight, sun), X fastest, then
/// Z, then Y (the texture's width, height and depth are X, Z and Y).
pub struct LightGrid {
    /// Voxel coordinate of the grid's first voxel.
    pub origin: [i32; 3],
    pub data: Vec<u8>,
    /// Filtered local radiance, RGB plus shared scale (same X/Z/Y layout).
    pub local: Vec<u8>,
}

impl LightGrid {
    /// The skylight byte of the voxel holding `p` (studs), if inside.
    #[allow(dead_code)]
    pub fn sky_at(&self, v: [i32; 3]) -> Option<u8> {
        let [nx, ny, nz] = dims();
        let l = [0, 1, 2].map(|k| v[k] - self.origin[k]);
        if l.iter().zip([nx, ny, nz]).any(|(&c, n)| c < 0 || c >= n as i32) {
            return None;
        }
        Some(self.data[2 * ((l[1] as usize * nz + l[2] as usize) * nx + l[0] as usize)])
    }
}

/// Where the grid starts (in chunks) for a camera at `cam`, given where it
/// was: Studio moves it only when the camera's chunk leaves the middle two
/// (FUN_144cd9980), then centres it on the nearest chunk corner.
pub fn follow(prev: Option<[i32; 3]>, cam: Vec3) -> [i32; 3] {
    let c = mech::LIGHTGRID_CHUNKS.map(|n| n as i32);
    let half = [c[0] / 2, c[1] / 2, c[2] / 2];
    let v = [cam.x, cam.y, cam.z].map(|q| ifloor(q * 0.25));
    let shift = [5, 4, 5];
    let prev = prev.unwrap_or([-half[0], -half[1], -half[2]]);
    let keep = (0..3).all(|k| {
        let d = (v[k] >> shift[k]) - prev[k] - half[k];
        d == -1 || d == 0
    });
    if keep {
        return prev;
    }
    let round = [16, 8, 16];
    [0, 1, 2].map(|k| ((v[k] + round[k]) >> shift[k]) - half[k])
}

/// Studio's float → int for voxel indices: truncation, less 0.999 below 0.
fn ifloor(x: f32) -> i32 {
    if x < 0.0 {
        (x - 0.999) as i32
    } else {
        x as i32
    }
}

// --------------------------------------------------------------- primitives

/// The voxelizer's shape kinds (Studio's record type byte, FUN_146654ff0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// CylinderMesh / Head SpecialMesh: a cylinder along Y.
    CylinderY,
    /// Part.Shape Cylinder, SpecialMesh Cylinder: along X.
    CylinderX,
    /// SpecialMesh Sphere: an ellipsoid filling the part.
    Ellipsoid,
    Block,
    /// Part.Shape Ball: a sphere of radius Size.X / 2.
    Ball,
    CornerWedge,
    Wedge,
    /// MeshPart / union (record type 9, FUN_146656f60): Studio fills a
    /// blob per convex piece of its physics (part box while unavailable).
    Mesh,
}

/// A part as the light grid takes it.
#[derive(Clone, Copy, Debug)]
pub struct Prim {
    pub kind: Kind,
    pub size: Vec3,
    /// The CFrame's rotation, row-major (R00, R01, R02, R10, …).
    pub rot: [f32; 9],
    pub pos: Vec3,
    pub transparency: f32,
}

impl Prim {
    pub fn new(kind: Kind, size: Vec3, xf: &crate::math::Xform, transparency: f32) -> Prim {
        let m = xf.rot;
        let rot = [
            m.x_axis.x, m.y_axis.x, m.z_axis.x, m.x_axis.y, m.y_axis.y, m.z_axis.y, m.x_axis.z, m.y_axis.z,
            m.z_axis.z,
        ];
        Prim { kind, size, rot, pos: xf.pos, transparency }
    }

    /// Half the world-space box around it.
    fn half_extent(&self) -> [f32; 3] {
        let r = &self.rot;
        let s = self.size;
        [0, 1, 2].map(|i| {
            (r[3 * i].abs() * s.x + r[3 * i + 1].abs() * s.y + r[3 * i + 2].abs() * s.z) * 0.5
        })
    }
}

/// The light grid's kind for a part: Studio's record type (FUN_146654ff0)
/// from the SpecialMesh type (−1 none), a BlockMesh / CylinderMesh child,
/// and the physics shape. File SpecialMeshes are not voxelized here;
/// MeshParts and unions are selected separately by the scene.
pub fn kind_of(special_mesh: Option<u32>, legacy_cylinder: Option<bool>, shape: crate::render::MeshKind) -> Option<Kind> {
    use crate::render::MeshKind as M;
    if let Some(t) = special_mesh {
        return match t {
            0 => Some(Kind::CylinderY), // Head (type 1)
            1 | 6 => Some(Kind::Block), // Torso (type 8, a box here), Brick
            2 => Some(Kind::Wedge),
            3 => Some(Kind::Ellipsoid),
            4 => Some(Kind::CylinderX),
            _ => None,
        };
    }
    if let Some(cyl) = legacy_cylinder {
        return Some(if cyl { Kind::CylinderY } else { Kind::Block });
    }
    Some(match shape {
        M::Block => Kind::Block,
        M::Ball => Kind::Ball,
        M::Cylinder => Kind::CylinderX,
        M::Wedge => Kind::Wedge,
        M::CornerWedge => Kind::CornerWedge,
    })
}

// --------------------------------------------------------------- occupancy

/// Three occupancy planes (seen along X, Y, Z) over the whole grid, each
/// X fastest, then Z, then Y.
pub struct Occupancy {
    pub origin: [i32; 3],
    pub planes: [Vec<u8>; 3],
}

impl Occupancy {
    pub fn new(origin: [i32; 3]) -> Occupancy {
        let [nx, ny, nz] = dims();
        let n = nx * ny * nz;
        Occupancy { origin, planes: [vec![0; n], vec![0; n], vec![0; n]] }
    }

    fn add(&mut self, chunk: [usize; 3], v: [usize; 3], val: [i32; 3]) {
        let [nx, _, nz] = dims();
        let (x, y, z) = (chunk[0] * 32 + v[0], chunk[1] * 16 + v[1], chunk[2] * 32 + v[2]);
        let i = (y * nz + z) * nx + x;
        for k in 0..3 {
            let s = self.planes[k][i] as i32 + val[k];
            self.planes[k][i] = s.clamp(0, 255) as u8;
        }
    }

    /// Voxelizes a part into every chunk it overlaps.
    pub fn add_prim(&mut self, p: &Prim) {
        self.add_prim_boxes(p, None);
    }

    /// Studio uses each physics convex piece's local bounds.
    pub fn add_prim_boxes(&mut self, p: &Prim, boxes: Option<&[(Vec3, Vec3)]>) {
        if p.transparency >= 1.0 {
            return;
        }
        let h = p.half_extent();
        let c = mech::LIGHTGRID_CHUNKS;
        let span = [128.0, 64.0, 128.0];
        let mut lo = [0usize; 3];
        let mut hi = [0usize; 3];
        for k in 0..3 {
            let base = self.origin[k] as f32 * 4.0;
            let a = ((p.pos[k] - h[k] - base) / span[k]).floor();
            let b = ((p.pos[k] + h[k] - base) / span[k]).floor();
            if b < 0.0 || a >= c[k] as f32 {
                return;
            }
            lo[k] = a.max(0.0) as usize;
            hi[k] = (b as usize).min(c[k] - 1);
        }
        for cy in lo[1]..=hi[1] {
            for cz in lo[2]..=hi[2] {
                for cx in lo[0]..=hi[0] {
                    let cmin = [
                        (self.origin[0] as f32 + cx as f32 * 32.0) * 4.0,
                        (self.origin[1] as f32 + cy as f32 * 16.0) * 4.0,
                        (self.origin[2] as f32 + cz as f32 * 32.0) * 4.0,
                    ];
                    let mut out = |v, val| self.add([cx, cy, cz], v, val);
                    if let Some(boxes) = boxes {
                        mesh_blob(p, boxes, cmin, (1.0 - p.transparency.clamp(0.0, 1.0)) * 255.5, &mut out);
                    } else {
                        voxelize(p, cmin, &mut out);
                    }
                }
            }
        }
    }

    /// Adds the terrain: its occupancy byte where it isn't Air or Water
    /// (FUN_146657c10), to all three planes, after the parts.
    #[allow(dead_code)]
    pub fn add_terrain(&mut self, t: &Voxels) {
        if let Some(plane) = terrain_plane(t, self.origin) {
            self.add_plane(&plane);
        }
    }

    /// Adds one occupancy byte per voxel to all three planes (saturating).
    pub fn add_plane(&mut self, plane: &[u8]) {
        for p in &mut self.planes {
            for (o, &b) in p.iter_mut().zip(plane) {
                *o = o.saturating_add(b);
            }
        }
    }
}

/// The terrain's occupancy bytes over a grid starting at voxel `origin`
/// (0 for Air and Water), or None when no terrain is in it.
pub fn terrain_plane(t: &Voxels, origin: [i32; 3]) -> Option<Vec<u8>> {
    let (ylo, yhi) = t.y_range()?;
    let [nx, ny, nz] = dims();
    if yhi < origin[1] || ylo >= origin[1] + ny as i32 {
        return None;
    }
    let mut plane = vec![0u8; nx * ny * nz];
    let mut any = false;
    let s = t.sampler();
    // Terrain chunk by terrain chunk (32³, aligned), so the sampler keeps
    // its chunk.
    let blocks = |o: i32, n: usize| {
        let first = o.div_euclid(32) * 32;
        (first..o + n as i32).step_by(32).map(move |b| (b.max(o), (b + 32).min(o + n as i32)))
    };
    for (y0, y1) in blocks(origin[1], ny) {
        if y1 <= ylo || y0 > yhi {
            continue;
        }
        for (z0, z1) in blocks(origin[2], nz) {
            for (x0, x1) in blocks(origin[0], nx) {
                // An empty terrain chunk: skip it whole.
                if s.get(x0, y0, z0).0 == 0 && !t.chunk_has_voxels([x0, y0, z0]) {
                    continue;
                }
                for wy in y0..y1 {
                    let y = (wy - origin[1]) as usize;
                    for wz in z0..z1 {
                        let row = (y * nz + (wz - origin[2]) as usize) * nx;
                        for wx in x0..x1 {
                            let (mat, occ) = s.get(wx, wy, wz);
                            if mat == 0 || mat == WATER {
                                continue;
                            }
                            // The stored byte ((byte + 1) / 256 as read).
                            plane[row + (wx - origin[0]) as usize] =
                                (occ * 256.0 - 1.0).round().clamp(0.0, 255.0) as u8;
                            any = true;
                        }
                    }
                }
            }
        }
    }
    any.then_some(plane)
}

/// The voxel range of the chunk at `cmin` a world box covers along one axis
/// (`trim` 0.01 for axis-aligned boxes, as Studio).
fn range(c: f32, h: f32, cmin: f32, max: i32, trim: f32) -> (i32, i32) {
    let lo = ifloor(((c - h) - cmin) * 0.25).max(0);
    let hi = ifloor(((h + c) - cmin) * 0.25 - trim).min(max);
    (lo, hi)
}

/// FUN_146655d20: |R| and, per local axis, 1 / cover cap and the offset of
/// the normalized cover of a thin, turned shape.
fn turned_norms(r: &[f32; 9], cap: [f32; 3]) -> ([f32; 9], [f32; 3], [f32; 3]) {
    let m = r.map(f32::abs);
    let inv = cap.map(|c| 1.0 / c);
    let rs = [m[0] + m[1] + m[2], m[3] + m[4] + m[5], m[6] + m[7] + m[8]];
    let (a, b);
    if m[0] > m[3] && m[0] > m[6] {
        a = rs[0];
        b = if m[7] < m[4] { rs[1] } else { rs[2] };
    } else if m[3] > m[6] {
        a = rs[1];
        b = if m[7] < m[1] { rs[0] } else { rs[2] };
    } else {
        a = rs[2];
        b = if m[4] < m[1] { rs[0] } else { rs[1] };
    }
    let off = [(1.0 - cap[0]) * (a - 1.0) * 0.5, (1.0 - cap[1]) * (b - 1.0) * 0.5, (1.0 - cap[2]) * (b - 1.0) * 0.5];
    (m, inv, off)
}

/// The three planes of a thin shape from its per-axis t (FUN_14664d9b0 and
/// kin): each local axis's projection, turned into world X / Y / Z by |R|.
fn project(t: [f32; 3], cap: [f32; 3], m: &[f32; 9], inv: [f32; 3], off: [f32; 3], k: f32) -> [i32; 3] {
    let cov = [0, 1, 2].map(|i| t[i].min(cap[i]).max(0.0));
    let n = [0, 1, 2].map(|i| {
        let v = (t[i] + off[i]) * inv[i];
        (if 1.0 <= v { 1.0 } else { v }).max(0.0)
    });
    let px = n[0] * cov[2] * cov[1];
    let py = n[1] * cov[2] * cov[0];
    let pz = n[2] * cov[1] * cov[0];
    [0, 1, 2].map(|row| {
        let v = pz * m[3 * row + 2] + py * m[3 * row + 1] + px * m[3 * row];
        (v * k) as i32
    })
}

/// One part into one chunk (min corner `cmin`): `out(voxel, [A, B, C])`.
fn voxelize(p: &Prim, cmin: [f32; 3], out: &mut dyn FnMut([usize; 3], [i32; 3])) {
    let k = (1.0 - p.transparency.clamp(0.0, 1.0)) * 255.5;
    let s = p.size;
    let small = s.x < 4.0 || s.y < 4.0 || s.z < 4.0;
    let r = &p.rot;
    if p.kind == Kind::Mesh {
        let h = p.size * 0.5;
        mesh_blob(p, &[(-h, h)], cmin, k, out);
        return;
    }
    if p.kind == Kind::Block {
        let turned = [r[0], r[4], r[8]].iter().any(|d| d.abs() > 1e-4 && d.abs() < 0.9999);
        if turned {
            block_turned(p, cmin, small, k, out);
        } else {
            block_aligned(p, cmin, small, k, out);
        }
        return;
    }
    let h = p.half_extent();
    let (x0, x1) = range(p.pos.x, h[0], cmin[0], 31, 0.0);
    let (y0, y1) = range(p.pos.y, h[1], cmin[1], 15, 0.0);
    let (z0, z1) = range(p.pos.z, h[2], cmin[2], 31, 0.0);
    if x0 > x1 || y0 > y1 || z0 > z1 {
        return;
    }
    let cap = [s.x, s.y, s.z].map(|v| if 1.0 <= v * 0.25 { 1.0 } else { v * 0.25 });
    let norms = small.then(|| turned_norms(r, cap));
    let (hx, hy, hz) = (s.x * 0.5, s.y * 0.5, s.z * 0.5);
    // Per-axis t (0.5 − signed distance outside / 4) by shape, studs.
    let t_of = |l: [f32; 3]| -> [f32; 3] {
        let edge = |d: f32| 0.5 - d * 0.25;
        // Signed distance to [lo, hi] (negative inside: to the nearer end).
        let slab = |v: f32, lo: f32, hi: f32| -> f32 {
            if !(lo < v) {
                lo - v
            } else if v < hi {
                -(if v - lo <= hi - v { v - lo } else { hi - v })
            } else {
                v - hi
            }
        };
        match p.kind {
            Kind::Ball => {
                let d = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
                let t = edge(d - hx);
                [t; 3]
            }
            Kind::CylinderX => {
                let rr = (l[1] * l[1] + l[2] * l[2]).sqrt();
                let t = edge(rr - hy.min(hz));
                [edge(l[0].abs() - hx), t, t]
            }
            Kind::CylinderY => {
                let rr = (l[0] * l[0] + l[2] * l[2]).sqrt();
                let t = edge(rr - hx.min(hz));
                [t, edge(l[1].abs() - hy), t]
            }
            Kind::Ellipsoid => {
                let q = (l[1] * l[1] * (1.0 / (hy * hy)) + l[0] * l[0] * (1.0 / (hx * hx))
                    + l[2] * l[2] * (1.0 / (hz * hz)))
                .sqrt()
                    - 1.0;
                [0.5 - q * hx * 0.25, 0.5 - q * hy * 0.25, 0.5 - q * hz * 0.25]
            }
            Kind::Wedge => {
                let dy = slab(l[1], -hy, l[2] * (s.y / s.z));
                let dz = slab(l[2], l[1] * (s.z / s.y), hz);
                [edge(l[0].abs() - hx), edge(dy), edge(dz)]
            }
            Kind::CornerWedge => {
                let dx = slab(l[0], l[1] * (s.x / s.y), hx);
                let top = (-l[2] * (s.y / s.z)).min(l[0] * (s.y / s.x));
                let dy = slab(l[1], -hy, top);
                let dz = slab(l[2], -hz, -l[1] * (s.z / s.y));
                [edge(dx), edge(dy), edge(dz)]
            }
            Kind::Block | Kind::Mesh => unreachable!(),
        }
    };
    let first = [
        ((x0 as f32 + 0.5) * 4.0 + cmin[0]) - p.pos.x,
        ((y0 as f32 + 0.5) * 4.0 + cmin[1]) - p.pos.y,
        ((z0 as f32 + 0.5) * 4.0 + cmin[2]) - p.pos.z,
    ];
    let local = |d: [f32; 3]| -> [f32; 3] {
        [0, 1, 2].map(|j| d[1] * r[3 + j] + d[0] * r[j] + d[2] * r[6 + j])
    };
    let step = |axis: usize| -> [f32; 3] { [0, 1, 2].map(|j| r[3 * axis + j] * 4.0) };
    let (sx, sy, sz) = (step(0), step(1), step(2));
    let mut ly = local(first);
    for y in y0..=y1 {
        let mut lz = ly;
        for z in z0..=z1 {
            let mut l = lz;
            for x in x0..=x1 {
                let t = t_of(l);
                let val = match &norms {
                    Some((m, inv, off)) => project(t, cap, m, *inv, *off, k),
                    None => {
                        let c = [0, 1, 2].map(|i| t[i].min(cap[i]).max(0.0));
                        let v = (c[1] * c[0] * c[2] * k) as i32;
                        [v; 3]
                    }
                };
                if val != [0; 3] {
                    out([x as usize, y as usize, z as usize], val);
                }
                for j in 0..3 {
                    l[j] += sx[j];
                }
            }
            for j in 0..3 {
                lz[j] += sz[j];
            }
        }
        for j in 0..3 {
            ly[j] += sy[j];
        }
    }
}

/// A mesh's convex pieces (part-space boxes) as Studio fills them
/// (FUN_146656f60): per piece, a blob around its world box's centre,
/// (1 − √q)³ with q = Σ (d·g / (2·half extent))² per world axis, taken over
/// the world box only; g grows the blob in for parts over 75 studs.
fn mesh_blob(p: &Prim, boxes: &[(Vec3, Vec3)], cmin: [f32; 3], k: f32, out: &mut dyn FnMut([usize; 3], [i32; 3])) {
    let r = &p.rot;
    let scale = if boxes.len() > 1 { 1.3 } else { 1.0 };
    let len = p.size.length();
    let over = (len * (1.0 / 300.0) - 0.25).max(0.0);
    let g = over * 1.7 * over + 1.0;
    for (lo, hi) in boxes {
        let c = (*lo + *hi) * 0.5;
        let full = (*hi - *lo) * scale;
        let wc = [0, 1, 2].map(|i| r[3 * i] * c.x + r[3 * i + 1] * c.y + r[3 * i + 2] * c.z + p.pos[i]);
        let he = [0, 1, 2].map(|i| {
            (r[3 * i + 1].abs() * full.y + r[3 * i].abs() * full.x + r[3 * i + 2].abs() * full.z) * 0.5
        });
        let kq = he.map(|h| (1.0 / (h * h)) * g * g * 0.25);
        let caps = [full.x, full.y, full.z].map(|v| if 1.0 <= v * 0.25 { 1.0 } else { v * 0.25 });
        let tmax = caps[0].max(caps[1].max(caps[2]));
        let (x0, x1) = range(wc[0], he[0], cmin[0], 31, 0.01);
        let (y0, y1) = range(wc[1], he[1], cmin[1], 15, 0.01);
        let (z0, z1) = range(wc[2], he[2], cmin[2], 31, 0.01);
        let d0 = [x0, y0, z0];
        let first = [0, 1, 2].map(|i| ((d0[i] as f32 + 0.5) * 4.0 + cmin[i]) - wc[i]);
        let mut dy = first[1];
        for y in y0..=y1 {
            let qy = kq[1] * dy * dy;
            let mut dz = first[2];
            for z in z0..=z1 {
                let qz = kq[2] * dz * dz;
                let mut dx = first[0];
                for x in x0..=x1 {
                    let q = kq[0] * dx * dx + qy + qz;
                    // Studio's bit-trick square root.
                    let root = f32::from_bits((((q.to_bits() as i32) - 0x80_0000) >> 1).wrapping_add(0x2000_0000) as u32);
                    let t = (1.0 - root).min(tmax).max(0.0);
                    let v = (t * t * t * k) as i32;
                    if v != 0 {
                        out([x as usize, y as usize, z as usize], [v; 3]);
                    }
                    dx += 4.0;
                }
                dz += 4.0;
            }
            dy += 4.0;
        }
    }
}

/// An axis-aligned block (also turned by multiples of 90°): FUN_14664ba10,
/// thin ones FUN_14664ab70.
fn block_aligned(p: &Prim, cmin: [f32; 3], small: bool, k: f32, out: &mut dyn FnMut([usize; 3], [i32; 3])) {
    let r = &p.rot;
    let s = p.size;
    let e = [0, 1, 2].map(|i| r[3 * i].abs() * s.x + r[3 * i + 1].abs() * s.y + r[3 * i + 2].abs() * s.z);
    let c = [p.pos.x, p.pos.y, p.pos.z];
    let max = [31, 15, 31];
    let mut lo = [0; 3];
    let mut hi = [0; 3];
    let mut frac = [0.0f32; 3];
    for i in 0..3 {
        lo[i] = ifloor(((c[i] - e[i] * 0.5) - cmin[i]) * 0.25).max(0);
        let up = ((e[i] * 0.5 + c[i]) - cmin[i]) * 0.25;
        let h = ifloor(up - 0.01);
        frac[i] = ((up - h as f32) - 1.0).max(0.0);
        hi[i] = h.min(max[i]);
    }
    if (0..3).any(|i| lo[i] > hi[i]) {
        return;
    }
    let floor = if small { 0.05 } else { 1.0 };
    let e2 = e.map(|v| if v <= floor { floor } else { v });
    let cap = e2.map(|v| if v * 0.25 < 1.0 { v * 0.25 } else { 1.0 });
    let half = e2.map(|v| v * 0.125 + 0.5);
    let inv = [0, 1, 2].map(|i| 1.0 / (cap[i] - frac[i]));
    let d0 = [0, 1, 2].map(|i| (((lo[i] as f32 + 0.5) * 4.0 + cmin[i]) - c[i]) * 0.25);
    let mut dy = d0[1];
    for y in lo[1]..=hi[1] {
        let raw_y = (half[1] - dy.abs()).max(0.0);
        let cy = raw_y.min(cap[1]);
        let ny = (inv[1] * raw_y).min(1.0);
        let mut dz = d0[2];
        for z in lo[2]..=hi[2] {
            let raw_z = (half[2] - dz.abs()).max(0.0);
            let cz = raw_z.min(cap[2]);
            let nz = (inv[2] * raw_z).min(1.0);
            let mut dx = d0[0];
            for x in lo[0]..=hi[0] {
                let raw_x = (half[0] - dx.abs()).max(0.0);
                let cx = raw_x.min(cap[0]);
                let val = if small {
                    let nx = (inv[0] * raw_x).min(1.0);
                    let a = (nx * (cz * cy * k)) as i32;
                    let b = (cx * cz * ny * k) as i32;
                    let cc = (cx * cy * nz * k) as i32;
                    [a, b, cc]
                } else {
                    let v = (cx * (cz * cy * k)) as i32;
                    [v; 3]
                };
                if val != [0; 3] {
                    out([x as usize, y as usize, z as usize], val);
                }
                dx += 1.0;
            }
            dz += 1.0;
        }
        dy += 1.0;
    }
}

/// A turned block: FUN_146649f00, thin ones FUN_146649580.
fn block_turned(p: &Prim, cmin: [f32; 3], small: bool, k: f32, out: &mut dyn FnMut([usize; 3], [i32; 3])) {
    let r = &p.rot;
    let s = [p.size.x, p.size.y, p.size.z];
    let h = p.half_extent();
    let (x0, x1) = range(p.pos.x, h[0], cmin[0], 31, 0.0);
    let (y0, y1) = range(p.pos.y, h[1], cmin[1], 15, 0.0);
    let (z0, z1) = range(p.pos.z, h[2], cmin[2], 31, 0.0);
    if x0 > x1 || y0 > y1 || z0 > z1 {
        return;
    }
    let floor = if small { 0.05 } else { 1.0 };
    let s2 = s.map(|v| if floor < v { v } else { floor });
    let cap = s2.map(|v| if v * 0.25 < 1.0 { v * 0.25 } else { 1.0 });
    let half = s2.map(|v| v * 0.125 + 0.5);
    let norms = small.then(|| turned_norms(r, cap));
    let d = [
        ((x0 as f32 + 0.5) * 4.0 + cmin[0]) - p.pos.x,
        ((y0 as f32 + 0.5) * 4.0 + cmin[1]) - p.pos.y,
        ((z0 as f32 + 0.5) * 4.0 + cmin[2]) - p.pos.z,
    ];
    // Local position in voxels; one voxel step along world axis a adds row a.
    let mut ly = [0, 1, 2].map(|j| (d[1] * r[3 + j] + d[0] * r[j] + d[2] * r[6 + j]) * 0.25);
    for y in y0..=y1 {
        let mut lz = ly;
        for z in z0..=z1 {
            let mut l = lz;
            for x in x0..=x1 {
                let t = [0, 1, 2].map(|i| half[i] - l[i].abs());
                let val = match &norms {
                    Some((m, inv, off)) => project(t, cap, m, *inv, *off, k),
                    None => {
                        let c = [0, 1, 2].map(|i| t[i].min(cap[i]).max(0.0));
                        let v = (c[0] * c[1] * c[2] * k) as i32;
                        [v; 3]
                    }
                };
                if val != [0; 3] {
                    out([x as usize, y as usize, z as usize], val);
                }
                for j in 0..3 {
                    l[j] += r[j];
                }
            }
            for j in 0..3 {
                lz[j] += r[6 + j];
            }
        }
        for j in 0..3 {
            ly[j] += r[3 + j];
        }
    }
}

// ------------------------------------------------------------------ skylight

/// What a voxel stores from the light it receives (0..255): 252 −
/// (255 − v)⁴ >> 24 (the table FUN_144cd3c80 builds).
fn store(v: i32) -> u8 {
    let d = (255 - v) as u32;
    (252 - ((d * d * d * d) >> 24)) as u8
}

/// Runs the skylight down through the occupancy (FUN_144cd3c80, the whole
/// grid at once): two bytes per voxel, skylight and sun (255).
pub fn skylight(occ: &Occupancy) -> LightGrid {
    let [nx, ny, nz] = dims();
    let table: Vec<u8> = (0..256).map(store).collect();
    let mut data = vec![255u8; nx * ny * nz * 2];
    let (w, d) = (nx + 2, nz + 2);
    // The light leaving the layer above, with a ring of 255 around it.
    let mut above = vec![255i32; w * d];
    let mut next = vec![255i32; w * d];
    let [pa, pb, pc] = &occ.planes;
    for y in (0..ny).rev() {
        for z in 0..nz {
            for x in 0..nx {
                let i = (z + 1) * w + x + 1;
                let sum = above[i - w - 1] + above[i - w] + above[i - w + 1] + above[i - 1] + above[i + 1]
                    + above[i + w - 1]
                    + above[i + w]
                    + above[i + w + 1];
                let got = (sum + 7) >> 3;
                let o = (y * nz + z) * nx + x;
                data[2 * o] = table[got as usize];
                let side = ((pa[o].max(pc[o]) as i32) * 180) >> 8;
                let block = side.max(pb[o] as i32);
                next[i] = (got - block).max(0);
            }
        }
        std::mem::swap(&mut above, &mut next);
    }
    LightGrid { origin: occ.origin, data, local: Vec::new() }
}

/// Solve Studio's directional sun channel (144cd34e0 and quadrant sweeps).
/// `sun` points toward the active celestial light. Below the horizon Studio
/// uses the moon; callers passing the astronomical sun get that flip here.
/// GlobalShadows=false callers should retain the initial 255 bytes instead.
pub fn sunlight(occ: &Occupancy, grid: &mut LightGrid, sun: Vec3) {
    let sun = if sun.y < 0.0 { Vec3::new(-sun.x, -sun.y, sun.z) } else { sun };
    let a = sun.abs().to_array();
    let major = if a[2] >= a[1] && a[2] >= a[0] { 2 }
        else if a[1] >= a[2] && a[1] >= a[0] { 1 } else { 0 };
    if !sun.is_finite() || a[major] == 0.0 { return; }
    let other: Vec<usize> = (0..3).filter(|&k| k != major).collect();
    let u = a[other[0]] / a[major];
    let v = a[other[1]] / a[major];
    let cross = u * v / 6.0;
    let mut w = [0.0; 3];
    w[other[0]] = u * 0.5 - cross;
    w[other[1]] = v * 0.5 - cross;
    w[major] = (1.0 - w[other[0]]) - w[other[1]];
    let mut q = w.map(|v| (v.clamp(0.0, 1.0) * 256.0 + 0.5) as i32);
    q[2] = (q[2] + (256 - q.iter().sum::<i32>()) / 3).max(0);
    q[0] = (q[0] + (256 - q[2] - q[1] - q[0]) / 2).max(0);
    q[1] = (256 - q[2] - q[0]).max(0);
    let attenuation = a.map(|v| (v * 255.0) as i32);
    let [nx, ny, nz] = dims();
    assert_eq!(grid.origin, occ.origin);
    assert_eq!(grid.data.len(), nx * ny * nz * 2);
    let mut pass = vec![255i32; nx * ny * nz];
    let sx = if sun.x < 0.0 { -1 } else { 1 };
    let sz = if sun.z < 0.0 { -1 } else { 1 };
    for y in (0..ny).rev() {
        for zi in 0..nz {
            let z = if sz > 0 { nz - 1 - zi } else { zi };
            for xi in 0..nx {
                let x = if sx > 0 { nx - 1 - xi } else { xi };
                let index = |x: usize, y: usize, z: usize| (y * nz + z) * nx + x;
                let read = |x: i32, y: usize, z: i32| {
                    if x < 0 || x >= nx as i32 || y >= ny || z < 0 || z >= nz as i32 { 255 }
                    else { pass[index(x as usize, y, z as usize)] }
                };
                let got = (read(x as i32 + sx, y, z as i32) * q[0]
                    + read(x as i32, y + 1, z as i32) * q[1]
                    + read(x as i32, y, z as i32 + sz) * q[2] + 255) >> 8;
                let i = index(x, y, z);
                grid.data[2 * i + 1] = got as u8;
                let block = (occ.planes[0][i] as i32 * attenuation[0]
                    + occ.planes[1][i] as i32 * attenuation[1]
                    + occ.planes[2][i] as i32 * attenuation[2] + 127) >> 8;
                pass[i] = (got - block).max(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Xform;
    use glam::{Mat3, Vec3};

    #[test]
    fn sunlight_air_slab_thin_roof_and_low_sun() {
        let mut o = Occupancy::new([0; 3]);
        let [nx, _, nz] = dims();
        let i = |x, y, z| (y * nz + z) * nx + x;
        let mut g = skylight(&o);
        sunlight(&o, &mut g, Vec3::Y);
        assert!(g.data.chunks_exact(2).all(|p| p[1] == 255));
        for plane in &mut o.planes { plane[i(20, 20, 20)] = 255; }
        sunlight(&o, &mut g, Vec3::Y);
        assert_eq!(g.data[2 * i(20, 19, 20) + 1], 1);
        for plane in &mut o.planes { plane[i(20, 20, 20)] = 64; }
        sunlight(&o, &mut g, Vec3::Y);
        assert_eq!(g.data[2 * i(20, 19, 20) + 1], 191);
        for plane in &mut o.planes { plane[i(20, 20, 20)] = 255; }
        sunlight(&o, &mut g, Vec3::new(1.0, 0.05, 0.0).normalize());
        assert!(g.data[2 * i(19, 20, 20) + 1] < 30);
        assert_eq!(g.data[2 * i(21, 20, 20) + 1], 255);
    }

    /// RS_LG_DUMP=prefix RS_LG_SUN=x,y,z cargo test ... -- --ignored --nocapture
    #[test]
    #[ignore]
    fn sunlight_vs_studio() {
        let pre = std::env::var("RS_LG_DUMP").unwrap();
        let hdr: Vec<i32> = std::fs::read_to_string(format!("{pre}.hdr")).unwrap()
            .split_whitespace().map(|v| v.parse().unwrap()).collect();
        let mut o = Occupancy::new([hdr[0], hdr[1], hdr[2]]);
        let bytes = std::fs::read(format!("{pre}.occ.bin")).unwrap();
        let n = o.planes[0].len();
        for k in 0..3 { o.planes[k].copy_from_slice(&bytes[k*n..(k+1)*n]); }
        let v: Vec<f32> = std::env::var("RS_LG_SUN").unwrap().split(',')
            .map(|s| s.parse().unwrap()).collect();
        let mut g = skylight(&o);
        sunlight(&o, &mut g, Vec3::new(v[0], v[1], v[2]));
        let expected = std::fs::read(format!("{pre}.sun.bin")).unwrap();
        let mut exact = 0; let mut near = 0; let mut sum = 0u64;
        for i in 0..n {
            let d = g.data[2*i+1].abs_diff(expected[i]);
            exact += usize::from(d == 0); near += usize::from(d <= 6); sum += d as u64;
        }
        println!("sun: {exact}/{n} exact ({:.6}%), <=6 {:.6}%, mean error {:.6}",
            exact as f64*100.0/n as f64, near as f64*100.0/n as f64, sum as f64/n as f64);
    }

    fn grid_with(prims: &[Prim]) -> (Occupancy, LightGrid) {
        let mut o = Occupancy::new([-128, -32, -128]);
        for p in prims {
            o.add_prim(p);
        }
        let g = skylight(&o);
        (o, g)
    }

    fn block(size: Vec3, pos: Vec3) -> Prim {
        Prim::new(Kind::Block, size, &Xform { pos, rot: Mat3::IDENTITY }, 0.0)
    }

    #[test]
    fn convex_blobs_leave_mesh_interior_open() {
        let p = Prim::new(Kind::Mesh, Vec3::splat(40.0),
            &Xform { pos: Vec3::splat(32.0), rot: Mat3::IDENTITY }, 0.0);
        let pieces = [
            (Vec3::new(-20.0, -20.0, -20.0), Vec3::new(-16.0, 20.0, 20.0)),
            (Vec3::new(16.0, -20.0, -20.0), Vec3::new(20.0, 20.0, 20.0)),
        ];
        let mut interior = 0;
        let mut wall = 0;
        mesh_blob(&p, &pieces, [0.0; 3], 255.5, &mut |v, c| {
            if v == [8, 8, 8] { interior += c[0]; }
            if v == [12, 8, 8] { wall += c[0]; }
        });
        assert_eq!(interior, 0);
        assert!(wall > 0);
    }

    /// Values read from Studio 0.746's chunks for the same parts
    /// (Studio measurements "Light grid (skylight)", scene1).
    #[test]
    fn matches_studio_occupancy() {
        let (o, _) = grid_with(&[
            block(Vec3::new(2.0, 40.0, 2.0), Vec3::new(10.0, 20.0, 10.0)),
            block(Vec3::new(32.0, 2.0, 32.0), Vec3::new(-48.0, 31.0, 16.0)),
            block(Vec3::new(4.0, 64.0, 64.0), Vec3::new(-16.0, 32.0, -64.0)),
            Prim::new(
                Kind::Ball,
                Vec3::splat(12.0),
                &Xform { pos: Vec3::new(100.0, 30.0, 100.0), rot: Mat3::IDENTITY },
                0.0,
            ),
        ]);
        let at = |p: [i32; 3]| -> [u8; 3] {
            let [nx, _, nz] = dims();
            let l = [0, 1, 2].map(|k| (p[k].div_euclid(4) - o.origin[k]) as usize);
            let i = (l[1] * nz + l[2]) * nx + l[0];
            [o.planes[0][i], o.planes[1][i], o.planes[2][i]]
        };
        assert_eq!(at([10, 20, 10]), [127, 63, 127]);
        assert_eq!(at([-48, 31, 16]), [127, 255, 127]);
        assert_eq!(at([-16, 32, -64]), [127, 127, 127]);
        assert_eq!(at([100, 30, 100]), [255, 255, 255]);
        assert_eq!(at([104, 30, 100]), [18, 18, 18]);
        assert_eq!(at([100, 35, 100]), [119, 119, 119]);
    }

    #[test]
    fn open_sky_and_shade() {
        let (_, g) = grid_with(&[block(Vec3::new(64.0, 4.0, 64.0), Vec3::new(0.0, 40.0, 0.0))]);
        assert_eq!(g.sky_at([60, 2, 60]), Some(252));
        // Right under the middle of a wide roof: dark.
        assert!(g.sky_at([0, 2, 0]).unwrap() < 40);
    }

    #[test]
    fn grid_follows_with_hysteresis() {
        let o = follow(None, Vec3::new(2.0, 60.0, 2.0));
        assert_eq!(o, [-4, -2, -4]);
        assert_eq!(follow(Some(o), Vec3::new(100.0, 60.0, 2.0)), o);
        assert_eq!(follow(Some([-4, -2, -4]), Vec3::new(100.0, 232.0, 335.0)), [-3, 2, -1]);
    }

    /// Times a full rebuild over terrain filling the grid's lower half.
    /// cargo test --release --bin rustudio lightgrid_timing -- --ignored --nocapture
    #[test]
    #[ignore]
    fn lightgrid_timing() {
        let mut v = Voxels::empty();
        let mut cells = Vec::new();
        for x in -128i32..128 {
            for z in -128..128 {
                for y in -32..((x * z as i32).rem_euclid(7)) {
                    cells.push(([x, y, z], 5u8, 1.0));
                }
            }
        }
        v.set_many(cells);
        let origin = [-128, -32, -128];
        let t = std::time::Instant::now();
        let plane = terrain_plane(&v, origin).unwrap();
        let read = t.elapsed();
        let mut o = Occupancy::new(origin);
        o.add_plane(&plane);
        let g = skylight(&o);
        println!("terrain read {read:?}, merge + skylight {:?}, {}", t.elapsed() - read, g.data.len());
    }

    /// Times a rebuild over a real place's parts and terrain at a point.
    /// RS_LG_PLACE=…rbxl RS_LG_AT=x,y,z cargo test --release --bin rustudio
    /// lightgrid_place_timing -- --ignored --nocapture
    #[test]
    #[ignore]
    fn lightgrid_place_timing() {
        let place = std::env::var("RS_LG_PLACE").expect("RS_LG_PLACE");
        let at: Vec<f32> = std::env::var("RS_LG_AT")
            .unwrap_or("0,50,0".into())
            .split(',')
            .map(|s| s.parse().unwrap())
            .collect();
        let mut doc = crate::doc::Document::open(std::path::Path::new(&place)).unwrap();
        let mut scene = crate::scene::Scene::default();
        scene.sync(&mut doc);
        let v = Voxels::from_dom(&doc.dom).and_then(|r| r.ok());
        let place = follow(None, Vec3::new(at[0], at[1], at[2]));
        let origin = [place[0] * 32, place[1] * 16, place[2] * 32];
        let t = std::time::Instant::now();
        let prims: Vec<Prim> = scene.parts.iter().filter_map(|p| p.light).collect();
        let t_prims = t.elapsed();
        let plane = v.as_ref().and_then(|v| terrain_plane(v, origin));
        let t_terrain = t.elapsed() - t_prims;
        let t = std::time::Instant::now();
        let mut o = Occupancy::new(origin);
        for p in &prims {
            o.add_prim(p);
        }
        let t_vox = t.elapsed();
        if let Some(p) = &plane {
            o.add_plane(p);
        }
        let g = skylight(&o);
        println!(
            "{} parts ({} in the light grid's kinds): gather {t_prims:?}, terrain {t_terrain:?}; worker: voxelize {t_vox:?}, terrain + skylight {:?} ({} voxels below 252)",
            scene.parts.len(),
            prims.len(),
            t.elapsed() - t_vox,
            g.data.chunks(2).filter(|c| c[0] < 252).count()
        );
    }

    /// Compares against a Studio dump: set
    /// RS_LG_DUMP=prefix (prefix.hdr / .occ.bin / .sky.bin) and
    /// RS_LG_PARTS=parts.txt, or RS_LG_PLACE=place.rbxl for parts + terrain.
    /// cargo test --release --bin rustudio lightgrid_vs_studio -- --ignored --nocapture
    #[test]
    #[ignore]
    fn lightgrid_vs_studio() {
        let pre = std::env::var("RS_LG_DUMP").expect("RS_LG_DUMP");
        let hdr: Vec<i32> = std::fs::read_to_string(format!("{pre}.hdr"))
            .unwrap()
            .split_whitespace()
            .map(|s| s.parse().unwrap())
            .collect();
        let origin = [hdr[0], hdr[1], hdr[2]];
        let socc = std::fs::read(format!("{pre}.occ.bin")).unwrap();
        let ssky = std::fs::read(format!("{pre}.sky.bin")).unwrap();
        let mut o = Occupancy::new(origin);
        if let Ok(parts) = std::env::var("RS_LG_PARTS") {
            for line in std::fs::read_to_string(parts).unwrap().lines() {
                let t: Vec<&str> = line.split_whitespace().collect();
                if t.len() < 19 || t[2] != "1" {
                    continue;
                }
                let f: Vec<f32> = t[3..19].iter().map(|s| s.parse().unwrap()).collect();
                let kind = match (t[1], t.get(19).copied().unwrap_or("-")) {
                    (_, "SMSphere") => Some(Kind::Ellipsoid),
                    (_, "SMCylinder") => Some(Kind::CylinderX),
                    (_, sm) if sm.starts_with("SM") => None,
                    ("Ball", _) => Some(Kind::Ball),
                    ("Cylinder", _) => Some(Kind::CylinderX),
                    ("Wedge", _) => Some(Kind::Wedge),
                    ("CornerWedge", _) => Some(Kind::CornerWedge),
                    ("MeshPart" | "UnionOperation", fid) => match std::env::var("RS_LG_MESH").as_deref() {
                        Ok("none") => None,
                        Ok("box") => Some(Kind::Block),
                        Ok("blobbox") => Some(if fid == "Box" { Kind::Block } else { Kind::Mesh }),
                        _ => Some(Kind::Mesh),
                    },
                    _ => Some(Kind::Block),
                };
                let Some(kind) = kind else { continue };
                let rot = [f[6], f[7], f[8], f[9], f[10], f[11], f[12], f[13], f[14]];
                o.add_prim(&Prim {
                    kind,
                    size: Vec3::new(f[0], f[1], f[2]),
                    rot,
                    pos: Vec3::new(f[3], f[4], f[5]),
                    transparency: f[15],
                });
            }
        }
        if let Ok(place) = std::env::var("RS_LG_PLACE") {
            let mut doc = crate::doc::Document::open(std::path::Path::new(&place)).unwrap();
            if std::env::var("RS_LG_PARTS").is_err() {
                let mut scene = crate::scene::Scene::default();
                scene.sync(&mut doc);
                for p in scene.parts.iter().filter_map(|p| p.light) {
                    o.add_prim(&p);
                }
            }
            if let Some(Ok(v)) = Voxels::from_dom(&doc.dom) {
                o.add_terrain(&v);
            }
        }
        let n = o.planes[0].len();
        if let Ok(out) = std::env::var("RS_LG_OUT") {
            std::fs::write(out, o.planes.concat()).unwrap();
        }
        let mut occ_bad = 0;
        let mut worst = (0, 0usize);
        for k in 0..3 {
            for i in 0..n {
                let d = (o.planes[k][i] as i32 - socc[k * n + i] as i32).abs();
                if d != 0 {
                    occ_bad += 1;
                    if d > worst.0 {
                        worst = (d, i);
                    }
                }
            }
        }
        let [nx, _, nz] = dims();
        let wi = worst.1;
        let wv = [wi % nx, wi / (nx * nz), (wi / nx) % nz];
        println!(
            "occupancy: {occ_bad} of {} bytes differ, worst {} at voxel {:?} (studs {:?})",
            3 * n,
            worst.0,
            wv,
            [0, 1, 2].map(|k| (wv[k] as i32 + origin[k]) * 4)
        );
        // The kernel on Studio's own occupancy, then on ours.
        let mut so = Occupancy::new(origin);
        for k in 0..3 {
            so.planes[k].copy_from_slice(&socc[k * n..(k + 1) * n]);
        }
        for (name, occ) in [("studio occupancy", &so), ("rustudio occupancy", &o)] {
            let t = std::time::Instant::now();
            let mut g = skylight(occ);
            let el = t.elapsed();
            let mut hist = [0usize; 5];
            let mut sum = 0u64;
            for i in 0..n {
                let d = (g.data[2 * i] as i32 - ssky[i] as i32).unsigned_abs() as usize;
                sum += d as u64;
                hist[match d {
                    0 => 0,
                    1..=2 => 1,
                    3..=6 => 2,
                    7..=20 => 3,
                    _ => 4,
                }] += 1;
            }
            if let (Ok(out), true) = (std::env::var("RS_LG_SKY_OUT"), name == "rustudio occupancy") {
                if let Ok(sun) = std::env::var("RS_LG_SUN") {
                    let v: Vec<f32> = sun.split(',').map(|s| s.parse().unwrap()).collect();
                    sunlight(occ, &mut g, Vec3::new(v[0], v[1], v[2]));
                }
                std::fs::write(out, &g.data).unwrap();
            }
            println!(
                "skylight from {name} ({el:?}): exact {:.4}, ≤2 {}, ≤6 {}, ≤20 {}, >20 {}, mean |d| {:.3}",
                hist[0] as f64 / n as f64,
                hist[1],
                hist[2],
                hist[3],
                hist[4],
                sum as f64 / n as f64
            );
        }
    }
}

/// Studio 0.746 CPU local lighting:,
/// shadow sweep and filter/RGB-scale packing.
/// Distances here are in four-stud voxels, independently of surface normals.
/// (The viewport keeps it as LocalLights; this is the whole channel at once.)
#[allow(dead_code)]
pub fn local_lights(occ: &Occupancy, lights: &[crate::render::LightRaw]) -> Vec<u8> {
    local_lights_dims(occ, lights, dims())
}

#[allow(dead_code)]
fn local_lights_dims(
    occ: &Occupancy,
    lights: &[crate::render::LightRaw],
    dims: [usize; 3],
) -> Vec<u8> {
    LocalLights::build_dims(occ, lights, dims).1
}

/// The voxels light `l` reaches in a grid of `dims` whose first voxel is
/// `origin`: (first, last), inclusive, or None when it reaches none.
fn light_box(origin: [i32; 3], l: &crate::render::LightRaw, dims: [usize; 3]) -> Option<([usize; 3], [usize; 3])> {
    let [nx, ny, nz] = dims;
    let pos = Vec3::from_slice(&l.pos_range) * 0.25 - Vec3::from_array(origin.map(|v| v as f32));
    let range = l.pos_range[3] * 0.25;
    if range <= 0.0 || !pos.is_finite() || !range.is_finite() {
        return None;
    }
    let u = Vec3::from_slice(&l.surface_u);
    let v = Vec3::from_slice(&l.surface_v);
    let hu = l.surface_u[3] * 0.25;
    let hv = l.surface_v[3] * 0.25;
    let extent = Vec3::splat(range + 2.0) + if l.color[3] > 1.5 { u.abs() * hu + v.abs() * hv } else { Vec3::ZERO };
    let lo = (pos - extent).floor().max(Vec3::ZERO);
    let hi = (pos + extent).ceil().min(Vec3::new(nx as f32 - 1.0, ny as f32 - 1.0, nz as f32 - 1.0));
    if lo.cmpgt(hi).any() {
        return None;
    }
    Some(([lo.x as usize, lo.y as usize, lo.z as usize], [hi.x as usize, hi.y as usize, hi.z as usize]))
}

fn box_volume(b: ([usize; 3], [usize; 3])) -> usize {
    (0..3).map(|k| b.1[k] - b.0[k] + 1).product()
}

/// Adds (or, `remove`, takes away exactly what adding it gave) light `l`'s
/// unfiltered radiance to `raw` over its box `(a, b)`.
fn light_into(
    occ: &Occupancy,
    l: &crate::render::LightRaw,
    dims: [usize; 3],
    (a, b): ([usize; 3], [usize; 3]),
    raw: &mut [Vec3],
    remove: bool,
) {
    let [nx, _, nz] = dims;
    let index = |p: [usize; 3]| (p[1] * nz + p[2]) * nx + p[0];
    let pos = Vec3::from_slice(&l.pos_range) * 0.25 - Vec3::from_array(occ.origin.map(|v| v as f32));
    let range = l.pos_range[3] * 0.25;
    let u = Vec3::from_slice(&l.surface_u);
    let v = Vec3::from_slice(&l.surface_v);
    let hu = l.surface_u[3] * 0.25;
    let hv = l.surface_v[3] * 0.25;
    let size = [0, 1, 2].map(|k| b[k] - a[k] + 1);
    let si = |p: [usize; 3]| ((p[1] - a[1]) * size[2] + p[2] - a[2]) * size[0] + p[0] - a[0];
    let source = [0, 1, 2].map(|k| pos[k].floor() as i32);
    let shadowed = l.extra[2] > 0.5 && l.color[3] < 1.5;
    let mut shadow = if shadowed { vec![255u8; size.iter().product()] } else { Vec::new() };
    let order: [Vec<usize>; 3] = [0, 1, 2].map(|k| {
        let mut axis: Vec<_> = (a[k]..=b[k]).collect();
        axis.sort_by_key(|&i| (i as i32 - source[k]).abs());
        axis
    });
    let color = Vec3::from_slice(&l.color);
    let dir = Vec3::from_slice(&l.dir);
    for &y in &order[1] {
        for &z in &order[2] {
            for &x in &order[0] {
                let p = [x, y, z];
                let i = index(p);
                let delta = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5) - pos;
                let mut visibility = 1.0;
                if shadowed {
                    let w = [0, 1, 2].map(|k| {
                        if p[k] as i32 == source[k] {
                            0.0
                        } else {
                            // The shadow sweep uses the integer voxel coordinate;
                            // the radiance writer uses its centre instead.
                            (p[k] as f32 - pos[k]).powi(2)
                        }
                    });
                    let total: f32 = w.iter().sum();
                    if total > 0.0 {
                        let mut recv = 0.0;
                        let mut block = 0.0;
                        for k in 0..3 {
                            if w[k] == 0.0 {
                                continue;
                            }
                            let mut q = p;
                            let next = p[k] as i32 + if (p[k] as i32) < source[k] { 1 } else { -1 };
                            let got = if next < a[k] as i32 || next > b[k] as i32 {
                                255
                            } else {
                                q[k] = next as usize;
                                shadow[si(q)]
                            };
                            recv += got as f32 * w[k];
                            block += occ.planes[k][i] as f32 * w[k];
                        }
                        shadow[si(p)] =
                            ((recv / total + 0.5) as i32 - (block / total + 0.5) as i32).max(0) as u8;
                    }
                    visibility = shadow[si(p)] as f32 / 255.0;
                }
                let mut d = delta;
                if l.color[3] > 1.5 {
                    d -= u * delta.dot(u).clamp(-hu, hu) + v * delta.dot(v).clamp(-hv, hv);
                }
                let distance = d.length();
                let shifted = (distance - 0.71 * range).max(0.0);
                let t = (shifted / (0.29 * range)).min(1.0);
                let mut energy = (1.0 - (3.0 - 2.0 * t) * t * t) * 1.35 / (shifted * shifted + 1.35);
                if l.color[3] > 0.5 {
                    let cosine = d.dot(dir) / distance.max(1e-6);
                    energy *= ((cosine - l.dir[3]) / (1.0 - l.dir[3]).max(1e-6)).clamp(0.0, 1.0);
                }
                let c = color * (energy * visibility);
                if remove {
                    raw[i] -= c;
                } else {
                    raw[i] += c;
                }
            }
        }
    }
}

/// Packs one filtered radiance: RGB plus the shared scale (Studio).
fn encode_local(c: Vec3) -> [u8; 4] {
    let c = c.max(Vec3::ZERO).min(Vec3::splat(120.0));
    let a = (c.max_element() * 2.125).max(12.75).ceil();
    let rgb = (c * (541.875 / a)).as_uvec3();
    [rgb.x as u8, rgb.y as u8, rgb.z as u8, a as u8]
}

/// The filtered, packed local channel over `lo..=hi` (X fastest, then Z,
/// then Y, as the texture), from the unfiltered `raw`: the separable
/// uniform 3x3x3 filter, in the full build's order, so the voxels come out
/// as a full build gives them.
fn filter_region(raw: &[Vec3], dims: [usize; 3], lo: [usize; 3], hi: [usize; 3]) -> Vec<u8> {
    let [nx, _, nz] = dims;
    // What the filter reads: one voxel more on every side (in the grid).
    let slo = [0, 1, 2].map(|k| lo[k].saturating_sub(1));
    let shi = [0, 1, 2].map(|k| (hi[k] + 1).min(dims[k] - 1));
    let size = [0, 1, 2].map(|k| shi[k] - slo[k] + 1);
    let si = |p: [usize; 3]| ((p[1] - slo[1]) * size[2] + p[2] - slo[2]) * size[0] + p[0] - slo[0];
    let mut sub = vec![Vec3::ZERO; size.iter().product()];
    for y in slo[1]..=shi[1] {
        for z in slo[2]..=shi[2] {
            let row = (y * nz + z) * nx;
            let at = si([slo[0], y, z]);
            sub[at..at + size[0]].copy_from_slice(&raw[row + slo[0]..=row + shi[0]]);
        }
    }
    let mut tmp = vec![Vec3::ZERO; sub.len()];
    for axis in [0, 2, 1] {
        for y in slo[1]..=shi[1] {
            for z in slo[2]..=shi[2] {
                for x in slo[0]..=shi[0] {
                    let p = [x, y, z];
                    let mut sum = Vec3::ZERO;
                    for offset in -1..=1 {
                        let c = p[axis] as i32 + offset;
                        // (past the sub-volume's own edge only its border is
                        // wrong, and the border isn't kept)
                        if c >= slo[axis] as i32 && c <= shi[axis] as i32 {
                            let mut q = p;
                            q[axis] = c as usize;
                            sum += sub[si(q)];
                        }
                    }
                    tmp[si(p)] = sum / 3.0;
                }
            }
        }
        std::mem::swap(&mut sub, &mut tmp);
    }
    let mut out = Vec::with_capacity((0..3).map(|k| hi[k] - lo[k] + 1).product::<usize>() * 4);
    for y in lo[1]..=hi[1] {
        for z in lo[2]..=hi[2] {
            for x in lo[0]..=hi[0] {
                out.extend_from_slice(&encode_local(sub[si([x, y, z])]));
            }
        }
    }
    out
}

/// The local channel as it is kept between frames: the unfiltered sum of
/// every light, so a change of a few lights (a muzzle flash's PointLight
/// flicked on and off, moving with the camera) re-lights just their boxes
/// on the spot, as Studio's grid shows a flash within a frame or two,
/// instead of the whole grid (~30 ms, which showed a flash late and kept
/// it lit after it was off).
pub struct LocalLights {
    pub origin: [i32; 3],
    dims: [usize; 3],
    /// Unfiltered radiance per voxel (empty while there are no lights).
    raw: Vec<Vec3>,
    /// The lights in `raw`.
    pub lights: Vec<crate::render::LightRaw>,
}

/// A re-light of part of the local channel: its first and last voxel and
/// the packed voxels between (as `filter_region`).
pub struct LocalPatch {
    pub lo: [usize; 3],
    pub hi: [usize; 3],
    pub data: Vec<u8>,
}

impl LocalLights {
    /// Every light from scratch: the kept state and the packed channel.
    pub fn build(occ: &Occupancy, lights: &[crate::render::LightRaw]) -> (LocalLights, Vec<u8>) {
        Self::build_dims(occ, lights, dims())
    }

    fn build_dims(occ: &Occupancy, lights: &[crate::render::LightRaw], dims: [usize; 3]) -> (LocalLights, Vec<u8>) {
        let len = dims.iter().product::<usize>();
        let mut me = LocalLights { origin: occ.origin, dims, raw: Vec::new(), lights: lights.to_vec() };
        let mut active: Option<([usize; 3], [usize; 3])> = None;
        for l in lights {
            let Some(bx) = light_box(occ.origin, l, dims) else { continue };
            if me.raw.is_empty() {
                me.raw = vec![Vec3::ZERO; len];
            }
            light_into(occ, l, dims, bx, &mut me.raw, false);
            active = Some(union(active, bx));
        }
        let mut out = Vec::with_capacity(len * 4);
        for _ in 0..len {
            out.extend_from_slice(&encode_local(Vec3::ZERO));
        }
        if let Some((lo, hi)) = active {
            let (lo, hi) = grow(lo, hi, dims);
            let patch = filter_region(&me.raw, dims, lo, hi);
            me.write(&mut out, lo, hi, &patch);
        }
        (me, out)
    }

    fn write(&self, out: &mut [u8], lo: [usize; 3], hi: [usize; 3], patch: &[u8]) {
        let [nx, _, nz] = self.dims;
        let w = (hi[0] - lo[0] + 1) * 4;
        let mut at = 0;
        for y in lo[1]..=hi[1] {
            for z in lo[2]..=hi[2] {
                let i = ((y * nz + z) * nx + lo[0]) * 4;
                out[i..i + w].copy_from_slice(&patch[at..at + w]);
                at += w;
            }
        }
    }

    /// Brings the kept lights to `lights` by taking away the ones gone and
    /// adding the new ones (the same ones, byte for byte, stay). Returns the
    /// re-filtered part of the channel to upload, None when nothing changed;
    /// Err when the lights that changed reach more than `max_voxels`
    /// (re-light everything on a worker instead; nothing was touched).
    pub fn update(
        &mut self,
        occ: &Occupancy,
        lights: &[crate::render::LightRaw],
        max_voxels: usize,
    ) -> Result<Option<LocalPatch>, ()> {
        if occ.origin != self.origin {
            return Err(());
        }
        let key = |l: &crate::render::LightRaw| bytemuck::bytes_of(l).to_vec();
        let mut kept: std::collections::HashMap<Vec<u8>, usize> = std::collections::HashMap::new();
        for l in &self.lights {
            *kept.entry(key(l)).or_default() += 1;
        }
        let mut added = Vec::new();
        for l in lights {
            match kept.get_mut(&key(l)) {
                Some(n) if *n > 0 => *n -= 1,
                _ => added.push(*l),
            }
        }
        let removed: Vec<_> = {
            let mut left = kept;
            self.lights
                .iter()
                .filter(|l| match left.get_mut(&key(l)) {
                    Some(n) if *n > 0 => {
                        *n -= 1;
                        true
                    }
                    _ => false,
                })
                .copied()
                .collect()
        };
        if added.is_empty() && removed.is_empty() {
            self.lights = lights.to_vec();
            return Ok(None);
        }
        let boxes: Vec<_> = removed
            .iter()
            .map(|l| (*l, true))
            .chain(added.iter().map(|l| (*l, false)))
            .filter_map(|(l, rm)| light_box(self.origin, &l, self.dims).map(|b| (l, rm, b)))
            .collect();
        if boxes.iter().map(|(_, _, b)| box_volume(*b)).sum::<usize>() > max_voxels {
            return Err(());
        }
        self.lights = lights.to_vec();
        let mut dirty = None;
        for (l, rm, b) in &boxes {
            if self.raw.is_empty() {
                self.raw = vec![Vec3::ZERO; self.dims.iter().product()];
            }
            light_into(occ, l, self.dims, *b, &mut self.raw, *rm);
            dirty = Some(union(dirty, *b));
        }
        let Some((lo, hi)) = dirty else { return Ok(None) };
        if self.lights.iter().all(|l| light_box(self.origin, l, self.dims).is_none()) {
            // (no light left: no float residue either, in the boxes the
            // ones taken away reached)
            let [nx, _, nz] = self.dims;
            for y in lo[1]..=hi[1] {
                for z in lo[2]..=hi[2] {
                    let row = (y * nz + z) * nx;
                    self.raw[row + lo[0]..=row + hi[0]].iter_mut().for_each(|c| *c = Vec3::ZERO);
                }
            }
        }
        let (lo, hi) = grow(lo, hi, self.dims);
        let data = filter_region(&self.raw, self.dims, lo, hi);
        Ok(Some(LocalPatch { lo, hi, data }))
    }
}

fn union(a: Option<([usize; 3], [usize; 3])>, b: ([usize; 3], [usize; 3])) -> ([usize; 3], [usize; 3]) {
    match a {
        None => b,
        Some((lo, hi)) => ([0, 1, 2].map(|k| lo[k].min(b.0[k])), [0, 1, 2].map(|k| hi[k].max(b.1[k]))),
    }
}

/// A box with the filter's reach (one voxel) added, in the grid.
fn grow(lo: [usize; 3], hi: [usize; 3], dims: [usize; 3]) -> ([usize; 3], [usize; 3]) {
    ([0, 1, 2].map(|k| lo[k].saturating_sub(1)), [0, 1, 2].map(|k| (hi[k] + 1).min(dims[k] - 1)))
}

#[cfg(test)]
mod local_tests {
    use super::*;
    use crate::render::LightRaw;

    fn light(range: f32) -> LightRaw {
        LightRaw {
            pos_range: [18.0, 18.0, 18.0, range],
            color: [1.0, 1.0, 1.0, 0.0],
            dir: [1.0, 0.0, 0.0, 0.0],
            extra: [0.0, 0.0, 0.0, -1.0],
            surface_u: [0.0, 1.0, 0.0, 8.0],
            surface_v: [0.0, 0.0, 1.0, 8.0],
        }
    }
    fn decode(data: &[u8], p: [usize; 3]) -> f32 {
        let i = ((p[1] * 16 + p[2]) * 16 + p[0]) * 4;
        data[i] as f32 * data[i + 3] as f32 * 120.0 / (255.0 * 255.0)
    }
    #[test]
    fn voxel_short_light_filter_and_shadow_wall() {
        let mut occ = Occupancy {
            origin: [0; 3],
            planes: std::array::from_fn(|_| vec![0; 16 * 16 * 16]),
        };
        let mut l = light(1.0);
        let short = local_lights_dims(&occ, &[l], [16; 3]);
        // One lit voxel spreads over exactly 27 cells, including quantization.
        // RGB-scale minimum alpha is ceil(12.75)=13; floor(541.875/(27*13))=1.
        assert!((decode(&short, [4, 4, 4]) - 13.0*120.0/(255.0*255.0)).abs() < 1e-6);
        assert_eq!(decode(&short, [6, 4, 4]), 0.0);
        l.pos_range[3] = 32.0;
        for plane in &mut occ.planes {
            for y in 0..16 {
                for z in 0..16 {
                    plane[(y * 16 + z) * 16 + 6] = 255;
                }
            }
        }
        let unshadowed = local_lights_dims(&occ, &[l], [16; 3]);
        l.extra[2] = 1.0;
        let shadowed = local_lights_dims(&occ, &[l], [16; 3]);
        assert!(decode(&unshadowed, [8, 4, 4]) > 0.5);
        assert_eq!(decode(&shadowed, [8, 4, 4]), 0.0);
        assert!(
            decode(&shadowed, [4, 4, 4]) > 0.9,
            "the source remains lit in front of the wall"
        );
    }
    /// The packed channel a patch leaves, written into `full`.
    fn apply(ll: &LocalLights, full: &mut [u8], patch: &LocalPatch) {
        ll.write(full, patch.lo, patch.hi, &patch.data);
    }
    fn wall_occ() -> Occupancy {
        let mut occ = Occupancy { origin: [0; 3], planes: std::array::from_fn(|_| vec![0; 16 * 16 * 16]) };
        for plane in &mut occ.planes {
            for y in 0..16 {
                for z in 0..16 {
                    plane[(y * 16 + z) * 16 + 9] = 255;
                }
            }
        }
        occ
    }
    fn max_diff(a: &[u8], b: &[u8]) -> f32 {
        let n = a.len() / 4;
        (0..n)
            .map(|i| {
                let d = |x: &[u8]| x[i * 4] as f32 * x[i * 4 + 3] as f32 * 120.0 / (255.0 * 255.0);
                (d(a) - d(b)).abs()
            })
            .fold(0.0, f32::max)
    }
    #[test]
    fn local_update_matches_a_full_build() {
        let occ = wall_occ();
        let mut lamp = light(24.0);
        lamp.extra[2] = 1.0; // shadowed, behind the wall's other side
        lamp.pos_range = [44.0, 30.0, 30.0, 24.0];
        let mut flash = light(12.0);
        flash.pos_range = [14.0, 20.0, 22.0, 12.0];
        flash.color = [3.5, 2.6, 1.6, 0.0];
        let dims = [16; 3];
        let (mut ll, mut shown) = LocalLights::build_dims(&occ, &[lamp], dims);
        // The flash on: the same as building both from scratch.
        let patch = ll.update(&occ, &[lamp, flash], usize::MAX).unwrap().expect("a patch");
        apply(&ll, &mut shown, &patch);
        let both = local_lights_dims(&occ, &[lamp, flash], dims);
        assert!(max_diff(&shown, &both) < 0.02, "on: {}", max_diff(&shown, &both));
        assert!(decode(&shown, [3, 5, 5]) > 0.5, "the flash lights its voxels");
        // Moved a voxel and a bit (the camera turned): still the same.
        flash.pos_range[0] += 5.0;
        let patch = ll.update(&occ, &[lamp, flash], usize::MAX).unwrap().expect("a patch");
        apply(&ll, &mut shown, &patch);
        let moved = local_lights_dims(&occ, &[lamp, flash], dims);
        assert!(max_diff(&shown, &moved) < 0.02, "moved: {}", max_diff(&shown, &moved));
        // Off again: back to the lamp alone, nothing left behind.
        let patch = ll.update(&occ, &[lamp], usize::MAX).unwrap().expect("a patch");
        apply(&ll, &mut shown, &patch);
        let lamp_only = local_lights_dims(&occ, &[lamp], dims);
        assert!(max_diff(&shown, &lamp_only) < 0.02, "off: {}", max_diff(&shown, &lamp_only));
        assert_eq!(decode(&shown, [3, 5, 5]), 0.0, "the flash is gone where only it reached");
        // The same lights: nothing to do.
        assert!(ll.update(&occ, &[lamp], usize::MAX).unwrap().is_none());
        // All off: the whole channel is dark again, exactly.
        let patch = ll.update(&occ, &[], usize::MAX).unwrap().expect("a patch");
        apply(&ll, &mut shown, &patch);
        assert_eq!(shown, local_lights_dims(&occ, &[], dims));
    }
    #[test]
    fn local_update_past_its_budget_leaves_everything() {
        let occ = wall_occ();
        let a = light(12.0);
        let (mut ll, _) = LocalLights::build_dims(&occ, &[a], [16; 3]);
        let mut b = a;
        b.pos_range[0] += 4.0;
        assert!(ll.update(&occ, &[b], 10).is_err());
        assert_eq!(bytemuck::bytes_of(&ll.lights[0]), bytemuck::bytes_of(&a), "kept as it was");
    }
    #[test]
    fn spot_cone_and_surface_rectangle() {
        let occ = Occupancy {
            origin: [0; 3],
            planes: std::array::from_fn(|_| vec![0; 16 * 16 * 16]),
        };
        let mut l = light(16.0);
        l.color[3] = 1.0;
        l.dir[3] = 0.70710677;
        let spot = local_lights_dims(&occ, &[l], [16; 3]);
        assert!(decode(&spot, [6, 4, 4]) > 0.3);
        assert_eq!(decode(&spot, [2, 4, 4]), 0.0);
        l.color[3] = 2.0;
        let surface = local_lights_dims(&occ, &[l], [16; 3]);
        assert!(
            decode(&surface, [6, 6, 4]) > decode(&spot, [6, 6, 4]),
            "distance is from the emitting rectangle"
        );
    }
}
