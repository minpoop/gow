//! Elden Ring armour parts, written from scratch for Kratos's body: the model (FLVER2, version 0x2001A), its textures
//! (a TPF of DDS files), the two in a BND4 container, compressed as DCX with DFLT (zlib). Nothing here comes from
//! either game; the layouts follow the community's documentation of these formats (SoulsFormatsNEXT, soulstruct and
//! the FLVER editors' material banks), written as our own code, and were checked field by field against the game's
//! own files where a sample was at hand (a map piece model, a parts animation binder, `systex.tpf.dcx`).
//!
//! One field the documentation gets wrong for Elden Ring: a mesh's "bounding box" is an oriented box (half extents,
//! Euler rotation, centre), not a minimum and maximum. A min/max there reads as a box of negative size at the
//! model's origin, which the game culls: 0.1.43 and 0.1.44 drew nothing, not even a cube.
//!
//! The model uses the armour material `C[AMSN]_e` with its 40-byte vertex: position (3 floats), normal, tangent and
//! four bone indices (4 bytes each, normals as b = v*127 + 127), four bone weights and a colour (4 bytes each, /255),
//! and two UV sets as signed shorts scaled by 2048. Vertex bone indices are global into the model's node list.

use std::collections::BTreeMap;

/// Written the way the game's own models name their material (`N:\GR\data\Material\mtd\<group>\matxml\<name>.matxml`);
/// the game looks the material up by its file name.
pub const MATERIAL_MTD: &str = "N:\\GR\\data\\Material\\mtd\\Parts\\matxml\\C[AMSN]_e.matxml";
pub const ALBEDO: &str = "C_AMSN__snp_Texture2D_2_AlbedoMap_0";
pub const METALLIC: &str = "C_AMSN__snp_Texture2D_0_MetallicMap_0";
pub const NORMAL: &str = "C_AMSN__snp_Texture2D_7_NormalMap_4";

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub name: String,
    pub parent: i16,
    /// Relative to the parent: translation, rotation (Euler radians, applied X, Z, Y), scale.
    pub t: [f32; 3],
    pub r: [f32; 3],
    pub s: [f32; 3],
    /// 4 = holds a mesh, 8 = a bone vertices are weighted to.
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// xyz and handedness (+1 or -1).
    pub tangent: [f32; 4],
    pub bones: [u8; 4],
    pub weights: [f32; 4],
    pub color: [u8; 4],
    pub uv: [[f32; 2]; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    pub material: u32,
    /// The node that stands for this mesh (flag 4).
    pub node: u32,
    pub vertices: Vec<Vertex>,
    /// A triangle list; 16-bit when the mesh has fewer than 65,536 vertices.
    pub indices: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    pub mtd: String,
    /// (sampler name, texture path; the game looks the path's file stem up in the part's TPF).
    pub textures: Vec<(String, String)>,
}

struct W {
    b: Vec<u8>,
    marks: BTreeMap<String, usize>,
}

impl W {
    fn new() -> W {
        W { b: Vec::new(), marks: BTreeMap::new() }
    }
    fn pos(&self) -> usize {
        self.b.len()
    }
    fn u8(&mut self, v: u8) {
        self.b.push(v);
    }
    fn i16(&mut self, v: i16) {
        self.b.extend(v.to_le_bytes());
    }
    fn u16(&mut self, v: u16) {
        self.b.extend(v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.b.extend(v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.b.extend(v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.b.extend(v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.b.extend(v.to_le_bytes());
    }
    fn v3(&mut self, v: [f32; 3]) {
        v.iter().for_each(|x| self.f32(*x));
    }
    fn bytes(&mut self, v: &[u8]) {
        self.b.extend_from_slice(v);
    }
    fn zeros(&mut self, n: usize) {
        self.b.resize(self.b.len() + n, 0);
    }
    fn pad(&mut self, align: usize) {
        while self.b.len() % align != 0 {
            self.b.push(0);
        }
    }
    /// A u32 to be filled in later.
    fn reserve(&mut self, key: impl Into<String>) {
        self.marks.insert(key.into(), self.b.len());
        self.u32(0);
    }
    fn reserve64(&mut self, key: impl Into<String>) {
        self.marks.insert(key.into(), self.b.len());
        self.u64(0);
    }
    fn fill(&mut self, key: &str, v: u32) {
        let at = self.marks.remove(key).unwrap_or_else(|| panic!("no reservation {key}"));
        self.b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn fill64(&mut self, key: &str, v: u64) {
        let at = self.marks.remove(key).unwrap_or_else(|| panic!("no reservation {key}"));
        self.b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }
    fn utf16z(&mut self, s: &str) {
        for c in s.encode_utf16() {
            self.u16(c);
        }
        self.u16(0);
    }
    fn done(self) -> Vec<u8> {
        assert!(self.marks.is_empty(), "unfilled: {:?}", self.marks.keys().collect::<Vec<_>>());
        self.b
    }
}

/// Every node's (first child, next sibling, previous sibling), from the parents.
fn links(parents: &[i16]) -> Vec<(i16, i16, i16)> {
    let mut out = vec![(-1i16, -1i16, -1i16); parents.len()];
    let mut last_child: BTreeMap<i16, i16> = BTreeMap::new();
    for (i, p) in parents.iter().enumerate() {
        let i = i as i16;
        if let Some(prev) = last_child.insert(*p, i) {
            out[prev as usize].1 = i;
            out[i as usize].2 = prev;
        } else if *p >= 0 {
            out[*p as usize].0 = i;
        }
    }
    out
}

fn bbox(points: impl Iterator<Item = [f32; 3]>) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    let mut any = false;
    for p in points {
        any = true;
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    if any {
        (lo, hi)
    } else {
        ([0.0; 3], [0.0; 3])
    }
}

/// A node's model-space transform at rest as a column-major 4x4 (rotation X, then Z, then Y, as FLVER applies it).
fn local_matrix(n: &Node) -> [[f32; 4]; 4] {
    let (sx, cx) = n.r[0].sin_cos();
    let (sy, cy) = n.r[1].sin_cos();
    let (sz, cz) = n.r[2].sin_cos();
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, sx], [0.0, -sx, cx]];
    let ry = [[cy, 0.0, -sy], [0.0, 1.0, 0.0], [sy, 0.0, cy]];
    let rz = [[cz, sz, 0.0], [-sz, cz, 0.0], [0.0, 0.0, 1.0]];
    // columns: R = Ry * Rz * Rx applied to column vectors
    let mul = |a: [[f32; 3]; 3], b: [[f32; 3]; 3]| -> [[f32; 3]; 3] { std::array::from_fn(|c| std::array::from_fn(|r| (0..3).map(|k| a[k][r] * b[c][k]).sum())) };
    let r = mul(ry, mul(rz, rx));
    let mut m = [[0.0f32; 4]; 4];
    for c in 0..3 {
        for row in 0..3 {
            m[c][row] = r[c][row] * n.s[c];
        }
    }
    m[3] = [n.t[0], n.t[1], n.t[2], 1.0];
    m
}

fn mat_mul(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    std::array::from_fn(|c| std::array::from_fn(|r| (0..4).map(|k| a[k][r] * b[c][k]).sum()))
}

/// Each node's rest transform in model space.
pub fn world_matrices(nodes: &[Node]) -> Vec<[[f32; 4]; 4]> {
    let mut out: Vec<[[f32; 4]; 4]> = Vec::with_capacity(nodes.len());
    for n in nodes {
        let l = local_matrix(n);
        let w = if n.parent >= 0 { mat_mul(&out[n.parent as usize], &l) } else { l };
        out.push(w);
    }
    out
}

fn invert_rigid(m: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    // rotation (with scale) inverse by the 3x3 inverse
    let a = |c: usize, r: usize| m[c][r] as f64;
    let det = a(0, 0) * (a(1, 1) * a(2, 2) - a(2, 1) * a(1, 2)) - a(1, 0) * (a(0, 1) * a(2, 2) - a(2, 1) * a(0, 2)) + a(2, 0) * (a(0, 1) * a(1, 2) - a(1, 1) * a(0, 2));
    let inv = |r: usize, c: usize| -> f64 {
        // cofactor of the transposed position
        let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
        let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
        (a(c1, r1) * a(c2, r2) - a(c2, r1) * a(c1, r2)) / det
    };
    let mut o = [[0.0f32; 4]; 4];
    for c in 0..3 {
        for r in 0..3 {
            o[c][r] = inv(c, r) as f32;
        }
    }
    for r in 0..3 {
        o[3][r] = -(0..3).map(|k| o[k][r] * m[3][k]).sum::<f32>();
    }
    o[3][3] = 1.0;
    o
}

fn apply(m: &[[f32; 4]; 4], p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r])
}

/// The Euler angles (radians; applied X, then Z, then Y, as [`Node::r`]) of the rotation quaternion `q` (x, y, z, w).
pub fn euler_xzy(q: [f32; 4]) -> [f32; 3] {
    let [x, y, z, w] = q.map(|v| v as f64);
    let n = (x * x + y * y + z * z + w * w).sqrt().max(1e-12);
    let (x, y, z, w) = (x / n, y / n, z / n, w / n);
    // R = Ry * Rz * Rx: R10 = sin z, R11 = cos z cos x, R12 = -cos z sin x, R00 = cos y cos z, R20 = -sin y cos z
    let r00 = 1.0 - 2.0 * (y * y + z * z);
    let r10 = 2.0 * (x * y + z * w);
    let r11 = 1.0 - 2.0 * (x * x + z * z);
    let r12 = 2.0 * (y * z - x * w);
    let r20 = 2.0 * (x * z - y * w);
    let ez = r10.clamp(-1.0, 1.0).asin();
    if ez.cos() > 1e-6 {
        [(-r12).atan2(r11) as f32, (-r20).atan2(r00) as f32, ez as f32]
    } else {
        // turned straight up or down (sin z = +-1): X and Y turn about the same axis, so all of it goes to X
        let sz = r10.signum();
        let r01 = 2.0 * (x * y - z * w);
        let r02 = 2.0 * (x * z + y * w);
        [(sz * r02).atan2(-sz * r01) as f32, 0.0, ez as f32]
    }
}

fn byte_norm(v: f32) -> u8 {
    (v.clamp(-1.0, 1.0) * 127.0 + 127.0).round() as u8
}

/// Set each vertex's tangent from the mesh's first UV set (the direction U grows along the surface, made square to the
/// normal; w is +1 or -1 by which way V grows), so normal maps read the right way. A vertex whose triangles have no
/// usable UVs keeps the tangent it had.
pub fn compute_tangents(m: &mut Mesh) {
    let n = m.vertices.len();
    let mut tan = vec![[0.0f32; 3]; n];
    let mut bit = vec![[0.0f32; 3]; n];
    for tri in m.indices.chunks_exact(3) {
        let [a, b, c] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        if a >= n || b >= n || c >= n {
            continue;
        }
        let (p0, p1, p2) = (m.vertices[a].position, m.vertices[b].position, m.vertices[c].position);
        let (t0, t1, t2) = (m.vertices[a].uv[0], m.vertices[b].uv[0], m.vertices[c].uv[0]);
        let e1: [f32; 3] = std::array::from_fn(|k| p1[k] - p0[k]);
        let e2: [f32; 3] = std::array::from_fn(|k| p2[k] - p0[k]);
        let (du1, dv1, du2, dv2) = (t1[0] - t0[0], t1[1] - t0[1], t2[0] - t0[0], t2[1] - t0[1]);
        let r = du1 * dv2 - du2 * dv1;
        if r.abs() < 1e-12 {
            continue;
        }
        let s: [f32; 3] = std::array::from_fn(|k| (e1[k] * dv2 - e2[k] * dv1) / r);
        let t: [f32; 3] = std::array::from_fn(|k| (e2[k] * du1 - e1[k] * du2) / r);
        for i in [a, b, c] {
            for k in 0..3 {
                tan[i][k] += s[k];
                bit[i][k] += t[k];
            }
        }
    }
    for (i, v) in m.vertices.iter_mut().enumerate() {
        let nn = v.normal;
        let d = nn[0] * tan[i][0] + nn[1] * tan[i][1] + nn[2] * tan[i][2];
        let t: [f32; 3] = std::array::from_fn(|k| tan[i][k] - nn[k] * d);
        let len = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt();
        if len < 1e-8 {
            continue;
        }
        let t = t.map(|x| x / len);
        let c = [nn[1] * t[2] - nn[2] * t[1], nn[2] * t[0] - nn[0] * t[2], nn[0] * t[1] - nn[1] * t[0]];
        let w = if c[0] * bit[i][0] + c[1] * bit[i][1] + c[2] * bit[i][2] < 0.0 { -1.0 } else { 1.0 };
        v.tangent = [t[0], t[1], t[2], w];
    }
}

/// Room around each mesh's box, so a limb a little out of its rest place is not culled while still on screen.
pub const BOX_MARGIN: f32 = 0.1;

/// A mesh's box as Elden Ring stores it: half extents, rotation (Euler radians) and centre. Ours are axis-aligned, with
/// [`BOX_MARGIN`] around the rest-pose vertices.
pub fn mesh_box(m: &Mesh) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let (lo, hi) = bbox(m.vertices.iter().map(|v| v.position));
    (std::array::from_fn(|k| (hi[k] - lo[k]) / 2.0 + BOX_MARGIN), [0.0; 3], std::array::from_fn(|k| (hi[k] + lo[k]) / 2.0))
}

/// The FLVER2 model.
pub fn flver2(nodes: &[Node], materials: &[Material], meshes: &[Mesh]) -> Vec<u8> {
    const LOD_FLAGS: [u32; 6] = [0, 0x0100_0000, 0x0200_0000, 0x8000_0000, 0x8100_0000, 0x8200_0000];
    let mut w = W::new();
    let world = world_matrices(nodes);
    let all = bbox(meshes.iter().flat_map(|m| m.vertices.iter().map(|v| v.position)));
    let wide = |m: &Mesh| m.vertices.len() >= 65536;
    let tris: usize = meshes.iter().map(|m| m.indices.len() / 3).sum();

    w.bytes(b"FLVER\0L\0");
    w.i32(0x2001A);
    w.reserve("data_offset");
    w.reserve("data_size");
    w.i32(0); // dummies
    w.i32(materials.len() as i32);
    w.i32(nodes.len() as i32);
    w.i32(meshes.len() as i32);
    w.i32(meshes.len() as i32); // one vertex buffer each
    w.v3(all.0);
    w.v3(all.1);
    // true and total faces: for triangle lists every face set counts, motion-blur ones too
    w.i32((tris * LOD_FLAGS.len()) as i32);
    w.i32((tris * LOD_FLAGS.len()) as i32);
    w.u8(0); // index size: per face set
    w.u8(1); // unicode
    w.u8(0);
    w.u8(0);
    w.i32(0);
    w.i32((meshes.len() * LOD_FLAGS.len()) as i32);
    w.i32(1); // buffer layouts
    w.i32(materials.iter().map(|m| m.textures.len() as i32).sum());
    // as the game's own models have them
    w.u8(0);
    w.u8(2); // unk5D
    w.zeros(2);
    w.zeros(8);
    w.i32(4); // unk68
    w.zeros(8);
    w.i32(0); // unk74
    w.zeros(8);
    debug_assert_eq!(w.pos(), 0x80);

    for (i, m) in materials.iter().enumerate() {
        w.reserve(format!("mat_name{i}"));
        w.reserve(format!("mat_mtd{i}"));
        w.i32(m.textures.len() as i32);
        w.reserve(format!("mat_tex{i}"));
        let chars = m.name.encode_utf16().count() + 1 + m.mtd.encode_utf16().count() + 1 + m.textures.iter().map(|(a, b)| a.encode_utf16().count() + 1 + b.encode_utf16().count() + 1).sum::<usize>();
        w.i32((chars * 2) as i32);
        w.reserve(format!("mat_gx{i}"));
        w.i32(i as i32); // the material's own index
        w.i32(0);
    }
    let parents: Vec<i16> = nodes.iter().map(|n| n.parent).collect();
    let link = links(&parents);
    // node boxes in node space: of the vertices weighted to the bone, or of the mesh it stands for
    let mut node_points: Vec<Vec<[f32; 3]>> = vec![Vec::new(); nodes.len()];
    for m in meshes {
        for v in &m.vertices {
            for k in 0..4 {
                if v.weights[k] > 0.0 && (v.bones[k] as usize) < nodes.len() {
                    node_points[v.bones[k] as usize].push(v.position);
                }
            }
            node_points[m.node as usize].push(v.position);
        }
    }
    for (i, n) in nodes.iter().enumerate() {
        let inv = invert_rigid(&world[i]);
        let (lo, hi) = bbox(node_points[i].iter().map(|p| apply(&inv, *p)));
        w.v3(n.t);
        w.reserve(format!("node_name{i}"));
        w.v3(n.r);
        w.i16(n.parent);
        w.i16(link[i].0);
        w.v3(n.s);
        w.i16(link[i].1);
        w.i16(link[i].2);
        w.v3(lo);
        w.u32(n.flags);
        w.v3(hi);
        w.zeros(0x34);
    }
    for (i, m) in meshes.iter().enumerate() {
        w.u8(1); // dynamic (skinned)
        w.zeros(3);
        w.i32(m.material as i32);
        w.i32(0);
        w.i32(0);
        w.i32(m.node as i32);
        w.i32(0); // no per-mesh bone list in Elden Ring
        w.reserve(format!("mesh_box{i}"));
        w.reserve(format!("mesh_bones{i}"));
        w.i32(LOD_FLAGS.len() as i32);
        w.reserve(format!("mesh_sets{i}"));
        w.i32(1);
        w.reserve(format!("mesh_vbs{i}"));
    }
    for (i, m) in meshes.iter().enumerate() {
        let bits = if wide(m) { 32 } else { 16 };
        for (j, f) in LOD_FLAGS.iter().enumerate() {
            w.u32(*f);
            w.u8(0); // triangle list
            w.u8(0); // no back-face culling
            w.i16(0);
            w.i32(m.indices.len() as i32);
            w.reserve(format!("set{i}_{j}"));
            w.i32((m.indices.len() * bits / 8) as i32);
            w.i32(0);
            w.i32(bits as i32);
            w.i32(0);
        }
    }
    for (i, m) in meshes.iter().enumerate() {
        w.i32(0); // the buffer's index within its mesh
        w.i32(0); // layout
        w.i32(40);
        w.i32(m.vertices.len() as i32);
        w.i32(0);
        w.i32(0);
        w.i32((40 * m.vertices.len()) as i32);
        w.reserve(format!("vb{i}"));
    }
    // the one layout
    w.i32(7);
    w.i32(0);
    w.i32(0);
    w.reserve("layout_members");
    let mut tex_index = 0;
    for (i, m) in materials.iter().enumerate() {
        w.fill(&format!("mat_tex{i}"), tex_index);
        for (k, _) in m.textures.iter().enumerate() {
            w.reserve(format!("tex_path{i}_{k}"));
            w.reserve(format!("tex_type{i}_{k}"));
            w.f32(1.0);
            w.f32(1.0);
            w.u8(1);
            w.u8(1);
            w.u8(0);
            w.u8(0);
            w.zeros(12);
        }
        tex_index += m.textures.len() as u32;
    }
    // skeleton set: every node in both lists
    w.i16(nodes.len() as i16);
    w.i16(nodes.len() as i16);
    w.reserve("skel_base");
    w.reserve("skel_all");
    w.zeros(20);
    for list in ["skel_base", "skel_all"] {
        let at = w.pos() as u32;
        w.fill(list, at);
        for (i, n) in nodes.iter().enumerate() {
            w.i16(n.parent);
            w.i16(link[i].0);
            w.i16(link[i].1);
            w.i16(link[i].2);
            w.i32(i as i32);
            w.i32(0);
        }
    }
    w.pad(16);
    let at = w.pos() as u32;
    w.fill("layout_members", at);
    // (offset in the vertex, type, semantic, index)
    for (off, ty, sem, idx) in [(0, 2, 0, 0), (12, 0x11, 3, 0), (16, 0x11, 6, 0), (20, 0x11, 2, 0), (24, 0x13, 1, 0), (28, 0x13, 10, 1), (32, 0x16, 5, 0)] {
        w.i32(0);
        w.i32(off);
        w.u32(ty);
        w.u32(sem);
        w.i32(idx);
    }
    w.pad(16);
    for (i, m) in meshes.iter().enumerate() {
        let at = w.pos() as u32;
        w.fill(&format!("mesh_box{i}"), at);
        let (half, rotation, centre) = mesh_box(m);
        w.v3(half);
        w.v3(rotation);
        w.v3(centre);
    }
    w.pad(16);
    let bones_start = w.pos() as u32;
    for i in 0..meshes.len() {
        w.fill(&format!("mesh_bones{i}"), bones_start);
    }
    w.pad(16);
    for i in 0..meshes.len() {
        let at = w.pos() as u32;
        w.fill(&format!("mesh_sets{i}"), at);
        for j in 0..LOD_FLAGS.len() {
            w.i32((i * LOD_FLAGS.len() + j) as i32);
        }
    }
    w.pad(16);
    for i in 0..meshes.len() {
        let at = w.pos() as u32;
        w.fill(&format!("mesh_vbs{i}"), at);
        w.i32(i as i32);
    }
    w.pad(16);
    // the material GX list: one GX00 item and the terminator
    let gx = w.pos() as u32;
    w.bytes(b"GX00");
    w.i32(102);
    w.i32(40 + 12);
    w.zeros(12);
    w.bytes(&[0xFF; 4]);
    w.zeros(24);
    w.i32(0x7FFF_FFFF);
    w.i32(100);
    w.i32(12);
    for i in 0..materials.len() {
        w.fill(&format!("mat_gx{i}"), gx);
    }
    w.pad(16);
    for (i, m) in materials.iter().enumerate() {
        let at = w.pos() as u32;
        w.fill(&format!("mat_name{i}"), at);
        w.utf16z(&m.name);
        let at = w.pos() as u32;
        w.fill(&format!("mat_mtd{i}"), at);
        w.utf16z(&m.mtd);
        for (k, (ty, path)) in m.textures.iter().enumerate() {
            let at = w.pos() as u32;
            w.fill(&format!("tex_path{i}_{k}"), at);
            w.utf16z(path);
            let at = w.pos() as u32;
            w.fill(&format!("tex_type{i}_{k}"), at);
            w.utf16z(ty);
        }
    }
    w.pad(16);
    for (i, n) in nodes.iter().enumerate() {
        let at = w.pos() as u32;
        w.fill(&format!("node_name{i}"), at);
        w.utf16z(&n.name);
    }
    w.pad(16);
    let data = w.pos();
    w.fill("data_offset", data as u32);
    for (i, m) in meshes.iter().enumerate() {
        for j in 0..LOD_FLAGS.len() {
            w.pad(16);
            let at = (w.pos() - data) as u32;
            w.fill(&format!("set{i}_{j}"), at);
            for x in &m.indices {
                if wide(m) {
                    w.u32(*x);
                } else {
                    w.u16(*x as u16);
                }
            }
        }
        w.pad(16);
        let at = (w.pos() - data) as u32;
        w.fill(&format!("vb{i}"), at);
        for v in &m.vertices {
            w.v3(v.position);
            for k in 0..3 {
                w.u8(byte_norm(v.normal[k]));
            }
            w.u8(0); // the bone a static mesh follows; skinned meshes use their weights
            for k in 0..4 {
                w.u8(byte_norm(v.tangent[k]));
            }
            w.bytes(&v.bones);
            for k in 0..4 {
                w.u8((v.weights[k].clamp(0.0, 1.0) * 255.0).round() as u8);
            }
            w.bytes(&v.color);
            for uv in v.uv {
                for c in uv {
                    w.i16((c * 2048.0).round().clamp(-32768.0, 32767.0) as i16);
                }
            }
        }
    }
    w.pad(16);
    let size = (w.pos() - data) as u32;
    w.fill("data_size", size);
    w.done()
}

/// A DDS file of one mip: BC1 (DXT1) blocks, `w` x `h` texels.
pub fn dds_bc1(w: u32, h: u32, blocks: &[u8]) -> Vec<u8> {
    let mut d = b"DDS ".to_vec();
    let mut put = |v: u32| d.extend(v.to_le_bytes());
    put(124); // header size
    put(0x1 | 0x2 | 0x4 | 0x1000 | 0x80000); // caps, height, width, pixel format, linear size
    put(h);
    put(w);
    put(blocks.len() as u32);
    put(0); // depth
    put(1); // mips
    for _ in 0..11 {
        put(0);
    }
    put(32); // pixel format size
    put(0x4); // fourcc
    d.extend(b"DXT1");
    for _ in 0..5 {
        d.extend(0u32.to_le_bytes());
    }
    d.extend(0x1000u32.to_le_bytes()); // caps: texture
    for _ in 0..4 {
        d.extend(0u32.to_le_bytes());
    }
    d.extend_from_slice(blocks);
    d
}

/// RGB 5:6:5 and back.
fn to565(c: [f32; 3]) -> u16 {
    let q = |v: f32, bits: u32| (v.clamp(0.0, 255.0) / 255.0 * ((1 << bits) - 1) as f32).round() as u16;
    (q(c[0], 5) << 11) | (q(c[1], 6) << 5) | q(c[2], 5)
}

fn from565(c: u16) -> [f32; 3] {
    let (r, g, b) = ((c >> 11) & 31, (c >> 5) & 63, c & 31);
    [(r as f32) * 255.0 / 31.0, (g as f32) * 255.0 / 63.0, (b as f32) * 255.0 / 31.0]
}

/// One opaque BC1 block for 16 pixels (RGBA, row by row): the two end colours span the block's colours along their
/// widest channel spread (the box diagonal, inset by a sixteenth), and each pixel takes the nearest of the four.
pub fn bc1_block(px: &[[u8; 4]; 16]) -> [u8; 8] {
    let mut lo = [255.0f32; 3];
    let mut hi = [0.0f32; 3];
    for p in px {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k] as f32);
            hi[k] = hi[k].max(p[k] as f32);
        }
    }
    // pick the box diagonal that follows the colours' own correlation
    let mean: [f32; 3] = std::array::from_fn(|k| px.iter().map(|p| p[k] as f32).sum::<f32>() / 16.0);
    let cov = |a: usize, b: usize| px.iter().map(|p| (p[a] as f32 - mean[a]) * (p[b] as f32 - mean[b])).sum::<f32>();
    let (rg, rb, gb) = (cov(0, 1), cov(0, 2), cov(1, 2));
    let (mut a, mut b) = (hi, lo);
    // channels that fall as red rises swap their ends
    if rg < 0.0 {
        std::mem::swap(&mut a[1], &mut b[1]);
    }
    if rb < 0.0 || (rg >= 0.0 && gb < 0.0 && rb == 0.0) {
        std::mem::swap(&mut a[2], &mut b[2]);
    }
    let inset: [f32; 3] = std::array::from_fn(|k| (a[k] - b[k]) / 16.0);
    let (a, b): ([f32; 3], [f32; 3]) = (std::array::from_fn(|k| a[k] - inset[k]), std::array::from_fn(|k| b[k] + inset[k]));
    let (mut c0, mut c1) = (to565(a), to565(b));
    if c0 < c1 {
        std::mem::swap(&mut c0, &mut c1);
    }
    let mut o = [0u8; 8];
    o[0..2].copy_from_slice(&c0.to_le_bytes());
    o[2..4].copy_from_slice(&c1.to_le_bytes());
    if c0 == c1 {
        return o; // one colour: every index 0
    }
    let (e0, e1) = (from565(c0), from565(c1));
    let pal: [[f32; 3]; 4] = [e0, e1, std::array::from_fn(|k| (2.0 * e0[k] + e1[k]) / 3.0), std::array::from_fn(|k| (e0[k] + 2.0 * e1[k]) / 3.0)];
    let mut bits = 0u32;
    for (i, p) in px.iter().enumerate() {
        let d = |c: &[f32; 3]| (0..3).map(|k| (c[k] - p[k] as f32).powi(2)).sum::<f32>();
        let best = (0..4).min_by(|x, y| d(&pal[*x]).total_cmp(&d(&pal[*y]))).unwrap() as u32;
        bits |= best << (2 * i);
    }
    o[4..8].copy_from_slice(&bits.to_le_bytes());
    o
}

/// An RGBA image (`w` x `h`, row by row) as BC1 blocks; edges repeat to fill the last blocks.
pub fn bc1_encode(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let (bw, bh) = (w.div_ceil(4).max(1), h.div_ceil(4).max(1));
    let mut out = Vec::with_capacity((bw * bh * 8) as usize);
    for by in 0..bh {
        for bx in 0..bw {
            let px: [[u8; 4]; 16] = std::array::from_fn(|i| {
                let (x, y) = ((bx * 4 + (i as u32 % 4)).min(w - 1), (by * 4 + (i as u32 / 4)).min(h - 1));
                let at = ((y * w + x) * 4) as usize;
                [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
            });
            out.extend_from_slice(&bc1_block(&px));
        }
    }
    out
}

/// Half an RGBA image in each direction (2x2 averages; a side of 1 stays 1).
pub fn half(w: u32, h: u32, rgba: &[u8]) -> (u32, u32, Vec<u8>) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            for c in 0..4 {
                let mut sum = 0u32;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = ((x * 2 + dx).min(w - 1), (y * 2 + dy).min(h - 1));
                    sum += rgba[((sy * w + sx) * 4 + c) as usize] as u32;
                }
                out[((y * nw + x) * 4 + c) as usize] = ((sum + 2) / 4) as u8;
            }
        }
    }
    (nw, nh, out)
}

/// A DDS file holding an RGBA image as BC1 with its whole mip chain down to 1 x 1, in the DX10 form the game's own
/// textures use: `srgb` marks a colour map (DXGI 72, BC1_UNORM_SRGB, as Elden Ring's `SYSTEX_Dummy_a`) so the GPU
/// turns it linear before shading; data maps (normals, roughness) are plain BC1_UNORM (DXGI 71).
pub fn dds_bc1_mips(w: u32, h: u32, rgba: &[u8], srgb: bool) -> Vec<u8> {
    let mut levels = vec![bc1_encode(w, h, rgba)];
    let (mut lw, mut lh, mut px) = (w, h, rgba.to_vec());
    while lw > 1 || lh > 1 {
        let (nw, nh, np) = half(lw, lh, &px);
        levels.push(bc1_encode(nw, nh, &np));
        (lw, lh, px) = (nw, nh, np);
    }
    let mut d = b"DDS ".to_vec();
    let mut put = |v: u32| d.extend(v.to_le_bytes());
    put(124);
    put(0x1 | 0x2 | 0x4 | 0x1000 | 0x20000 | 0x80000); // caps, height, width, pixel format, mip count, linear size
    put(h);
    put(w);
    put(levels[0].len() as u32);
    put(0);
    put(levels.len() as u32);
    for _ in 0..11 {
        put(0);
    }
    put(32);
    put(0x4); // fourcc
    d.extend(b"DX10");
    for _ in 0..5 {
        d.extend(0u32.to_le_bytes());
    }
    d.extend((0x1000u32 | 0x8 | 0x40_0000).to_le_bytes()); // texture, complex, mipmap
    for _ in 0..4 {
        d.extend(0u32.to_le_bytes());
    }
    // the DX10 header: format, a 2D texture, no flags, one in the array
    for v in [if srgb { 72u32 } else { 71 }, 3, 0, 1, 0] {
        d.extend(v.to_le_bytes());
    }
    for l in &levels {
        d.extend_from_slice(l);
    }
    d
}

/// One BC1 block of a single colour (RGB 5:6:5).
pub fn bc1_solid(r: u8, g: u8, b: u8) -> [u8; 8] {
    let c = ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3);
    let mut o = [0u8; 8];
    o[0..2].copy_from_slice(&c.to_le_bytes());
    o[2..4].copy_from_slice(&c.to_le_bytes());
    o
}

/// A PC texture pack: (name, DDS bytes, format byte). Names are UTF-16.
pub fn tpf(textures: &[(String, Vec<u8>, u8)]) -> Vec<u8> {
    let mut w = W::new();
    w.bytes(b"TPF\0");
    w.reserve("size");
    w.i32(textures.len() as i32);
    w.u8(0); // PC
    w.u8(3);
    w.u8(1); // UTF-16 names
    w.u8(0);
    for (i, (_, dds, format)) in textures.iter().enumerate() {
        let mips = dds.get(28..32).map(|b| u32::from_le_bytes(b.try_into().unwrap())).unwrap_or(1).max(1);
        w.reserve(format!("data{i}"));
        w.i32(dds.len() as i32);
        w.u8(*format);
        w.u8(0); // a plain texture
        w.u8(mips as u8);
        w.u8(0);
        w.reserve(format!("name{i}"));
        w.i32(0);
    }
    for (i, (name, _, _)) in textures.iter().enumerate() {
        let at = w.pos() as u32;
        w.fill(&format!("name{i}"), at);
        w.utf16z(name);
    }
    let start = w.pos();
    for (i, (_, dds, _)) in textures.iter().enumerate() {
        w.pad(4);
        let at = w.pos() as u32;
        w.fill(&format!("data{i}"), at);
        w.bytes(dds);
    }
    let size = (w.pos() - start) as u32;
    w.fill("size", size);
    w.done()
}

/// The path hash a BND4 table uses: lower case, forward slashes, a leading slash, h = h * 37 + c.
pub fn path_hash(path: &str) -> u32 {
    let mut s = path.to_lowercase().replace('\\', "/");
    if !s.starts_with('/') {
        s.insert(0, '/');
    }
    s.chars().fold(0u32, |h, c| h.wrapping_mul(37).wrapping_add(c as u32))
}

fn is_prime(n: u32) -> bool {
    n >= 2 && (2..).take_while(|d| d * d <= n).all(|d| n % d != 0)
}

/// A BND4 container (version "07D7R6", format IDs | names | compression flag, UTF-16 names, path hash table) of
/// (id, name, bytes).
pub fn bnd4(files: &[(i32, String, Vec<u8>)]) -> Vec<u8> {
    let mut w = W::new();
    w.bytes(b"BND4");
    w.u8(0);
    w.u8(0);
    w.u8(0);
    w.u8(0);
    w.u8(0);
    w.u8(0); // little-endian
    w.u8(1); // bits not big-endian
    w.u8(0);
    w.i32(files.len() as i32);
    w.u64(0x40);
    w.bytes(b"07D7R6\0\0");
    // file header: flags, 3 pad, -1, size, uncompressed size, u32 offset, id, name offset
    w.u64(0x10 + 4 + 8 + 4 + 4);
    w.reserve64("headers_end");
    w.u8(1); // unicode
    w.u8(0x2Eu8.reverse_bits()); // IDs | Names1 | Names2 | Compression, stored bit-reversed
    w.u8(4); // extended: a hash table follows the names
    w.u8(0);
    w.i32(0);
    w.reserve64("hash_table");
    debug_assert_eq!(w.pos(), 0x40);
    for (i, (id, _, bytes)) in files.iter().enumerate() {
        w.u8(0x02u8.reverse_bits());
        w.zeros(3);
        w.i32(-1);
        w.u64(bytes.len() as u64);
        w.u64(bytes.len() as u64);
        w.reserve(format!("data{i}"));
        w.i32(*id);
        w.reserve(format!("name{i}"));
    }
    for (i, (_, name, _)) in files.iter().enumerate() {
        let at = w.pos() as u32;
        w.fill(&format!("name{i}"), at);
        w.utf16z(name);
    }
    w.pad(8);
    let at = w.pos() as u64;
    w.fill64("hash_table", at);
    let groups = (files.len() as u32 / 7..).find(|p| is_prime(*p)).unwrap_or(2);
    let mut buckets: Vec<Vec<(u32, i32)>> = vec![Vec::new(); groups as usize];
    for (i, (_, name, _)) in files.iter().enumerate() {
        let h = path_hash(name);
        buckets[(h % groups) as usize].push((h, i as i32));
    }
    buckets.iter_mut().for_each(|b| b.sort());
    w.reserve64("hashes");
    w.u32(groups);
    w.bytes(&[0x10, 8, 8, 0]);
    let mut start = 0;
    for b in &buckets {
        w.i32(b.len() as i32);
        w.i32(start);
        start += b.len() as i32;
    }
    let at = w.pos() as u64;
    w.fill64("hashes", at);
    for (h, i) in buckets.iter().flatten() {
        w.u32(*h);
        w.i32(*i);
    }
    let at = w.pos() as u64;
    w.fill64("headers_end", at);
    for (i, (_, _, bytes)) in files.iter().enumerate() {
        w.pad(16);
        let at = w.pos() as u32;
        w.fill(&format!("data{i}"), at);
        w.bytes(bytes);
    }
    w.done()
}

/// A ZSTD frame holding `data` in stored (raw) blocks of at most 128 KiB: valid for any decoder, no compressor needed.
pub fn zstd_stored(data: &[u8]) -> Vec<u8> {
    let mut o = vec![0x28, 0xB5, 0x2F, 0xFD];
    // single segment, 8-byte content size, no checksum, no dictionary
    o.push(0b1110_0000);
    o.extend((data.len() as u64).to_le_bytes());
    let chunks: Vec<&[u8]> = if data.is_empty() { vec![&[][..]] } else { data.chunks(128 << 10).collect() };
    for (k, c) in chunks.iter().enumerate() {
        let last = (k + 1 == chunks.len()) as u32;
        let header = last | (0 << 1) | ((c.len() as u32) << 3);
        o.extend(&header.to_le_bytes()[..3]);
        o.extend_from_slice(c);
    }
    o
}

/// DCX with ZSTD (big-endian header, as Elden Ring's own since its 1.12 patch), holding `data` in stored blocks.
pub fn dcx_zstd(data: &[u8]) -> Vec<u8> {
    let z = zstd_stored(data);
    let mut o = b"DCX\0".to_vec();
    for v in [0x11000u32, 0x18, 0x24, 0x44, 0x4C] {
        o.extend(v.to_be_bytes());
    }
    o.extend(b"DCS\0");
    o.extend((data.len() as u32).to_be_bytes());
    o.extend((z.len() as u32).to_be_bytes());
    o.extend(b"DCP\0ZSTD");
    o.extend(0x20u32.to_be_bytes());
    o.extend([15, 0, 0, 0]); // the level its own files record
    o.extend([0u8; 8]);
    o.extend(0u32.to_be_bytes());
    o.extend(0x10100u32.to_be_bytes());
    o.extend(b"DCA\0");
    o.extend(8u32.to_be_bytes());
    o.extend(z);
    while o.len() % 16 != 0 {
        o.push(0);
    }
    o
}

/// A zlib stream holding `data` in stored (uncompressed) deflate blocks of at most 65,535 bytes.
pub fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut o = vec![0x78, 0xDA];
    let chunks: Vec<&[u8]> = if data.is_empty() { vec![&[][..]] } else { data.chunks(65535).collect() };
    for (k, c) in chunks.iter().enumerate() {
        o.push((k + 1 == chunks.len()) as u8);
        o.extend((c.len() as u16).to_le_bytes());
        o.extend((!(c.len() as u16)).to_le_bytes());
        o.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for x in data {
        a = (a + *x as u32) % 65521;
        b = (b + a) % 65521;
    }
    o.extend(((b << 16) | a).to_be_bytes());
    o
}

/// DCX with DFLT (zlib), the way Elden Ring's own `systex.tpf.dcx` is stored, holding `data` in stored blocks.
pub fn dcx_dflt(data: &[u8]) -> Vec<u8> {
    let z = zlib_stored(data);
    let mut o = b"DCX\0".to_vec();
    for v in [0x11000u32, 0x18, 0x24, 0x44, 0x4C] {
        o.extend(v.to_be_bytes());
    }
    o.extend(b"DCS\0");
    o.extend((data.len() as u32).to_be_bytes());
    o.extend((z.len() as u32).to_be_bytes());
    o.extend(b"DCP\0DFLT");
    o.extend(0x20u32.to_be_bytes());
    o.extend(0x0900_0000u32.to_be_bytes());
    o.extend([0u8; 4]);
    o.extend(0x0F00_0000u32.to_be_bytes()); // zlib's window bits, as the game's own files record them
    o.extend([0u8; 4]);
    o.extend(0x10100u32.to_be_bytes());
    o.extend(b"DCA\0");
    o.extend(8u32.to_be_bytes());
    o.extend(z);
    o
}

/// A whole armour part file (`<stem>.partsbnd.dcx`): the model and its textures under the paths the game expects.
pub fn partsbnd(stem: &str, flver: Vec<u8>, tpf: Vec<u8>) -> Vec<u8> {
    let base = format!("N:\\GR\\data\\INTERROOT_win64\\parts\\{stem}\\{stem}");
    dcx_dflt(&bnd4(&[(100, format!("{base}.tpf"), tpf), (200, format!("{base}.flver"), flver)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_zstd_frames_split_into_raw_blocks() {
        let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let z = zstd_stored(&data);
        assert_eq!(&z[..4], &[0x28, 0xB5, 0x2F, 0xFD]);
        assert_eq!(u64::from_le_bytes(z[5..13].try_into().unwrap()), 300_000);
        // three blocks: 128 KiB, 128 KiB, the rest; only the last flagged
        let h0 = u32::from_le_bytes([z[13], z[14], z[15], 0]);
        assert_eq!((h0 & 1, (h0 >> 1) & 3, h0 >> 3), (0, 0, 128 << 10));
        assert_eq!(z.len(), 13 + 3 * 3 + 300_000);
        let at = 13 + 2 * (3 + (128 << 10));
        let h2 = u32::from_le_bytes([z[at], z[at + 1], z[at + 2], 0]);
        assert_eq!((h2 & 1, h2 >> 3), (1, 300_000 - 2 * (128 << 10)));
    }

    #[test]
    fn dcx_header_is_big_endian_with_the_sizes() {
        let d = dcx_zstd(b"hello");
        assert_eq!(&d[..4], b"DCX\0");
        assert_eq!(u32::from_be_bytes(d[4..8].try_into().unwrap()), 0x11000);
        assert_eq!(&d[0x18..0x1C], b"DCS\0");
        assert_eq!(u32::from_be_bytes(d[0x1C..0x20].try_into().unwrap()), 5);
        assert_eq!(&d[0x24..0x2C], b"DCP\0ZSTD");
        assert_eq!(&d[0x44..0x48], b"DCA\0");
        assert_eq!(&d[0x4C..0x50], &[0x28, 0xB5, 0x2F, 0xFD]);
        assert_eq!(d.len() % 16, 0);
    }

    #[test]
    fn dflt_dcx_wraps_a_valid_zlib_stream() {
        let data: Vec<u8> = (0..200_000u32).map(|i| (i * 7 % 256) as u8).collect();
        let d = dcx_dflt(&data);
        assert_eq!(&d[0x24..0x2C], b"DCP\0DFLT");
        assert_eq!(u32::from_be_bytes(d[0x1C..0x20].try_into().unwrap()) as usize, data.len());
        let z = &d[0x4C..];
        assert_eq!(u32::from_be_bytes(d[0x20..0x24].try_into().unwrap()) as usize, z.len());
        // walk the stored blocks back to the data and check the Adler-32
        let (mut at, mut out) = (2, Vec::new());
        loop {
            let last = z[at] & 1;
            let len = u16::from_le_bytes([z[at + 1], z[at + 2]]) as usize;
            assert_eq!(!u16::from_le_bytes([z[at + 3], z[at + 4]]) as usize, len);
            out.extend_from_slice(&z[at + 5..at + 5 + len]);
            at += 5 + len;
            if last == 1 {
                break;
            }
        }
        assert_eq!(out, data);
        assert_eq!(z.len(), at + 4);
        // the header the game's own systex.tpf.dcx has, sizes aside
        assert_eq!(&d[..0x18], &[b'D', b'C', b'X', 0, 0, 1, 0x10, 0, 0, 0, 0, 0x18, 0, 0, 0, 0x24, 0, 0, 0, 0x44, 0, 0, 0, 0x4C][..]);
        assert_eq!(&d[0x2C..0x4C], &[0, 0, 0, 0x20, 9, 0, 0, 0, 0, 0, 0, 0, 0x0F, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, b'D', b'C', b'A', 0, 0, 0, 0, 8][..]);
    }

    #[test]
    fn euler_angles_rebuild_the_quaternions_rotation() {
        let quats = [[0.0, 0.0, 0.0, 1.0], [0.0, 0.70710677, 0.0, 0.70710677], [-0.49987617, 0.49987692, -0.5001238, 0.5001231], [0.1, -0.7, 0.3, 0.64], [0.0, 0.0, 0.70710677, 0.70710677], [0.5, 0.5, 0.5, 0.5]];
        for q in quats {
            let n = (q.iter().map(|v: &f32| v * v).sum::<f32>()).sqrt();
            let [x, y, z, w] = q.map(|v| v / n);
            let want = [[1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + z * w), 2.0 * (x * z - y * w)], [2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w)], [2.0 * (x * z + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y)]];
            let node = Node { name: "b".into(), parent: -1, t: [0.0; 3], r: euler_xzy(q), s: [1.0; 3], flags: 8 };
            let m = world_matrices(&[node])[0];
            for c in 0..3 {
                for r in 0..3 {
                    assert!((m[c][r] - want[c][r]).abs() < 1e-5, "{q:?}: column {c} row {r}: {} vs {}", m[c][r], want[c][r]);
                }
            }
        }
    }

    #[test]
    fn a_meshs_box_is_half_extents_rotation_and_centre() {
        let (_, _, meshes) = one_triangle();
        let (half, rotation, centre) = mesh_box(&meshes[0]);
        let (lo, hi) = bbox(meshes[0].vertices.iter().map(|v| v.position));
        for k in 0..3 {
            assert!(half[k] >= BOX_MARGIN);
            assert!((centre[k] - (half[k] - BOX_MARGIN) - lo[k]).abs() < 1e-6 && (centre[k] + (half[k] - BOX_MARGIN) - hi[k]).abs() < 1e-6);
        }
        assert_eq!(rotation, [0.0; 3]);
    }

    #[test]
    fn bnd4_lists_its_files_with_ids_names_and_hashes() {
        let b = bnd4(&[(100, "N:\\a\\x.tpf".into(), vec![1, 2, 3]), (200, "N:\\a\\x.flver".into(), vec![9; 40])]);
        assert_eq!(&b[..4], b"BND4");
        assert_eq!(i32::from_le_bytes(b[0xC..0x10].try_into().unwrap()), 2);
        assert_eq!(b[0x31], 0x74); // 0x2E bit-reversed
        let header = |i: usize| 0x40 + i * 0x24;
        let data_at = u32::from_le_bytes(b[header(1) + 0x18..header(1) + 0x1C].try_into().unwrap()) as usize;
        assert_eq!(&b[data_at..data_at + 40], &[9; 40]);
        assert_eq!(i32::from_le_bytes(b[header(1) + 0x1C..header(1) + 0x20].try_into().unwrap()), 200);
        let name_at = u32::from_le_bytes(b[header(0) + 0x20..header(0) + 0x24].try_into().unwrap()) as usize;
        let name: Vec<u16> = b[name_at..name_at + 20].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(String::from_utf16_lossy(&name[..10]), "N:\\a\\x.tpf");
        assert_eq!(path_hash("N:\\a\\X.tpf"), path_hash("/n:/a/x.tpf"));
    }

    fn one_triangle() -> (Vec<Node>, Vec<Material>, Vec<Mesh>) {
        let nodes = vec![
            Node { name: "Master".into(), parent: -1, t: [0.0; 3], r: [0.0; 3], s: [1.0; 3], flags: 8 },
            Node { name: "kratos".into(), parent: -1, t: [0.0; 3], r: [0.0; 3], s: [1.0; 3], flags: 4 },
        ];
        let v = |x: f32, y: f32| Vertex { position: [x, y, 0.0], normal: [0.0, 0.0, 1.0], tangent: [1.0, 0.0, 0.0, 1.0], bones: [0; 4], weights: [1.0, 0.0, 0.0, 0.0], color: [255; 4], uv: [[x, y], [0.0, 0.0]] };
        let mesh = Mesh { material: 0, node: 1, vertices: vec![v(0.0, 0.0), v(1.0, 0.0), v(0.0, 1.0)], indices: vec![0, 1, 2] };
        let mat = Material { name: "kratos".into(), mtd: MATERIAL_MTD.into(), textures: vec![(ALBEDO.into(), "N:\\kratos_a.tif".into())] };
        (nodes, vec![mat], vec![mesh])
    }

    #[test]
    fn a_flver_has_its_header_tables_and_data_in_place() {
        let (nodes, mats, meshes) = one_triangle();
        let f = flver2(&nodes, &mats, &meshes);
        assert_eq!(&f[..8], b"FLVER\0L\0");
        assert_eq!(i32::from_le_bytes(f[8..12].try_into().unwrap()), 0x2001A);
        let data = u32::from_le_bytes(f[12..16].try_into().unwrap()) as usize;
        let size = u32::from_le_bytes(f[16..20].try_into().unwrap()) as usize;
        assert_eq!(data + size, f.len());
        assert_eq!(data % 16, 0);
        // counts: materials, nodes, meshes, buffers
        assert_eq!(&f[0x18..0x28].chunks(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect::<Vec<_>>(), &[1, 2, 1, 1]);
        // the vertex buffer's offset (relative to the data) points at the first vertex's position
        let vb = 0x80 + 0x20 + 2 * 0x80 + 0x30 + 6 * 0x20;
        assert_eq!(i32::from_le_bytes(f[vb + 8..vb + 12].try_into().unwrap()), 40);
        let at = data + u32::from_le_bytes(f[vb + 28..vb + 32].try_into().unwrap()) as usize;
        let x1 = f32::from_le_bytes(f[at + 40..at + 44].try_into().unwrap());
        assert_eq!(x1, 1.0);
        // UV 1.0 is 2048
        assert_eq!(i16::from_le_bytes(f[at + 40 + 32..at + 40 + 34].try_into().unwrap()), 2048);
        // the first face set's indices
        let fs = 0x80 + 0x20 + 2 * 0x80 + 0x30;
        let idx = data + u32::from_le_bytes(f[fs + 12..fs + 16].try_into().unwrap()) as usize;
        assert_eq!(&f[idx..idx + 6], &[0, 0, 1, 0, 2, 0]);
        assert_eq!(i32::from_le_bytes(f[fs + 24..fs + 28].try_into().unwrap()), 16);
        // the header values and material index the game's own models have
        assert_eq!((f[0x5C], f[0x5D], i32::from_le_bytes(f[0x68..0x6C].try_into().unwrap())), (0, 2, 4));
        assert_eq!(i32::from_le_bytes(f[0x80 + 0x18..0x80 + 0x1C].try_into().unwrap()), 0);
        // the mesh's box: half extents, no rotation, centre
        let mesh = 0x80 + 0x20 + 2 * 0x80;
        let at = u32::from_le_bytes(f[mesh + 24..mesh + 28].try_into().unwrap()) as usize;
        let b: Vec<f32> = f[at..at + 36].chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        assert_eq!(b, [0.6, 0.6, 0.1, 0.0, 0.0, 0.0, 0.5, 0.5, 0.0]);
        // normal w is 0
        assert_eq!(f[data + u32::from_le_bytes(f[vb + 28..vb + 32].try_into().unwrap()) as usize + 15], 0);
    }

    #[test]
    fn rest_transforms_compose_down_the_hierarchy() {
        let nodes = vec![
            Node { name: "a".into(), parent: -1, t: [0.0, 1.0, 0.0], r: [0.0, std::f32::consts::FRAC_PI_2, 0.0], s: [1.0; 3], flags: 8 },
            Node { name: "b".into(), parent: 0, t: [1.0, 0.0, 0.0], r: [0.0; 3], s: [1.0; 3], flags: 8 },
        ];
        let w = world_matrices(&nodes);
        let p = apply(&w[1], [0.0; 3]);
        // a quarter turn about Y takes +X to -Z
        assert!((p[0]).abs() < 1e-5 && (p[1] - 1.0).abs() < 1e-5 && (p[2] + 1.0).abs() < 1e-5, "{p:?}");
        let back = apply(&invert_rigid(&w[1]), p);
        assert!(back.iter().all(|x| x.abs() < 1e-5));
        assert_eq!(links(&[-1, 0, 0, 1]), vec![(1, -1, -1), (3, 2, -1), (-1, -1, 1), (-1, -1, -1)]);
    }

    /// Writes a sample part for checking with an outside reader:
    /// `GOWRING_ERPART_OUT=dir cargo test -p gowring-gow sample_part -- --ignored`.
    #[test]
    #[ignore]
    fn sample_part() {
        let Ok(dir) = std::env::var("GOWRING_ERPART_OUT") else { return };
        let (nodes, mut mats, mut meshes) = one_triangle();
        mats[0].textures = vec![(ALBEDO.into(), "N:\\x\\bd_m_9701_a.tif".into()), (METALLIC.into(), "N:\\x\\bd_m_9701_m.tif".into()), (NORMAL.into(), "N:\\x\\bd_m_9701_n.tif".into())];
        // a second, bigger mesh: a 300 x 300 grid (90,000 vertices, so 32-bit indices)
        let n = 300u32;
        let vertices = (0..n * n).map(|k| Vertex { position: [(k % n) as f32 / n as f32, (k / n) as f32 / n as f32, 0.1], normal: [0.0, 0.0, 1.0], tangent: [1.0, 0.0, 0.0, 1.0], bones: [0; 4], weights: [1.0, 0.0, 0.0, 0.0], color: [255; 4], uv: [[0.5, 0.5], [0.0, 0.0]] }).collect();
        let indices = (0..n - 1).flat_map(|y| (0..n - 1).flat_map(move |x| { let a = y * n + x; [a, a + 1, a + n, a + 1, a + n + 1, a + n] })).collect();
        meshes.push(Mesh { material: 0, node: 1, vertices, indices });
        let flver = flver2(&nodes, &mats, &meshes);
        let tex = |r, g, b| dds_bc1(4, 4, &bc1_solid(r, g, b));
        let t = tpf(&[("bd_m_9701_a".into(), tex(200, 150, 120), 0), ("bd_m_9701_m".into(), tex(0, 128, 0), 0), ("bd_m_9701_n".into(), tex(128, 128, 255), 0)]);
        std::fs::write(std::path::Path::new(&dir).join("bd_m_9701.partsbnd.dcx"), partsbnd("bd_m_9701", flver, t)).unwrap();
    }

    #[test]
    fn bc1_keeps_an_images_colours_and_mips_halve_down_to_one_pixel() {
        // a smooth gradient with a red stripe
        let (w, h) = (64u32, 32u32);
        let mut img = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let at = ((y * w + x) * 4) as usize;
                let red = (8..12).contains(&y);
                img[at..at + 4].copy_from_slice(&[if red { 230 } else { (x * 4) as u8 }, if red { 20 } else { (y * 8) as u8 }, if red { 20 } else { 128 }, 255]);
            }
        }
        let blocks = bc1_encode(w, h, &img);
        assert_eq!(blocks.len(), (16 * 8 * 8) as usize);
        let back = crate::dds::decode_raw(crate::dds::Format::Bc1, w, h, &blocks).unwrap();
        let err = img.iter().zip(&back.rgba).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
        assert!(err <= 24, "worst channel error {err}");
        let dds = dds_bc1_mips(w, h, &img, true);
        // 64x32, 32x16, 16x8, 8x4, 4x2, 2x1, 1x1: blocks of 4x4 at least
        let mips = u32::from_le_bytes(dds[28..32].try_into().unwrap());
        assert_eq!(mips, 7);
        assert_eq!(dds.len(), 148 + (128 + 32 + 8 + 2 + 1 + 1 + 1) * 8);
        // a DX10 header naming BC1 sRGB, as the game's own colour maps
        assert_eq!(&dds[84..88], b"DX10");
        assert_eq!(u32::from_le_bytes(dds[128..132].try_into().unwrap()), 72);
        assert_eq!(u32::from_le_bytes(dds_bc1_mips(8, 8, &[0; 256], false)[128..132].try_into().unwrap()), 71);
        let decoded = crate::dds::decode(&dds).unwrap();
        assert_eq!((decoded.w, decoded.h), (w, h));
        assert_eq!(&decoded.rgba[..], &back.rgba[..]);
    }

    #[test]
    fn tangents_follow_u_across_the_surface_and_flip_with_v() {
        let (_, _, mut meshes) = one_triangle();
        // one_triangle: positions (0,0), (1,0), (0,1) in the xy plane with UV equal to xy, normal +z
        compute_tangents(&mut meshes[0]);
        for v in &meshes[0].vertices {
            assert!((v.tangent[0] - 1.0).abs() < 1e-6 && v.tangent[1].abs() < 1e-6 && v.tangent[3] == 1.0, "{:?}", v.tangent);
        }
        // mirror V: the tangent stays along x, the handedness flips
        for v in meshes[0].vertices.iter_mut() {
            v.uv[0][1] = -v.uv[0][1];
        }
        compute_tangents(&mut meshes[0]);
        assert!(meshes[0].vertices.iter().all(|v| v.tangent[3] == -1.0 && (v.tangent[0] - 1.0).abs() < 1e-6));
    }

    #[test]
    fn a_tpf_holds_named_dds_files() {
        let dds = dds_bc1(4, 4, &bc1_solid(200, 150, 120));
        assert_eq!(&dds[..4], b"DDS ");
        assert_eq!(&dds[84..88], b"DXT1");
        assert_eq!(dds.len(), 128 + 8);
        let t = tpf(&[("kratos_a".into(), dds.clone(), 0)]);
        assert_eq!(&t[..4], b"TPF\0");
        let at = u32::from_le_bytes(t[0x10..0x14].try_into().unwrap()) as usize;
        assert_eq!(&t[at..at + dds.len()], &dds[..]);
        assert_eq!(u32::from_le_bytes(t[4..8].try_into().unwrap()) as usize, t.len() - (0x10 + 0x14 + 18));
    }
}
