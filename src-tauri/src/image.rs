//! 宿主图像原语 —— `uking.image.*`。
//!
//! 为什么由宿主提供：动作模块不能用 canvas（Node 里没有），又不该各自去啃图像格式。
//! 更要紧的是，GUI 和无头两条路必须算出**同一个结果** —— 图像数学放在宿主这一份里，
//! 就不会出现「界面上好好的，AI 调出来的图不一样」这种最难查的偏差。
//!
//! 实现取舍：PNG 自己解码/编码（zlib 已有 flate2，PNG 只是 zlib + 行滤波器，约 150 行），
//! 像素运算纯 Rust；只有两件事借外力 —— 非 PNG 输入的解码、以及写字要的字体栅格化，
//! 都交给 ffmpeg（它本来就是 U-King「AI 厨具」里的一员）。没有 ffmpeg 时给明确提示，
//! 而不是悄悄出一张错图。
//!
//! **独立可插拔**：纯 std + serde_json + flate2。删它：lib.rs 去 `mod image;`，
//! miniapp.rs 去 dispatch 里 "image." 那一支。

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Clone)]
pub struct Img {
    pub w: u32,
    pub h: u32,
    /// RGBA8，长度 w*h*4
    pub px: Vec<u8>,
}

/// 句柄表。一次动作执行期间有效，跑完由 `clear_session` 清掉 ——
/// 中间结果动辄几十 MB，绝不能让它跟着进程活到天荒地老。
static STORE: Mutex<Option<HashMap<String, Img>>> = Mutex::new(None);
static SEQ: Mutex<u64> = Mutex::new(0);

fn put(img: Img) -> String {
    let mut s = SEQ.lock().unwrap();
    *s += 1;
    let id = format!("img{}", *s);
    drop(s);
    let mut st = STORE.lock().unwrap();
    st.get_or_insert_with(HashMap::new).insert(id.clone(), img);
    id
}

fn get(id: &str) -> Result<Img, String> {
    STORE
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(id).cloned())
        .ok_or_else(|| format!("invalid_input: 图像句柄 {id} 不存在（可能已被释放）"))
}

/// 就地替换句柄背后的图。
///
/// `fillRect` / `drawText` 这类「往这张图上画」的动词**必须**就地改：
/// 它们要是返回新句柄，调用方一不留神继续用旧句柄，绘制就被静默丢弃 ——
/// 动作照样返回 ok，图却没变。这个坑踩过一次（本地改字返回成功但像素没动），
/// 所以语义在这里钉死：改变尺寸或合并图像的（crop/resize/compositeFeather）返回新句柄，
/// 单纯往上画的一律就地。
fn replace(id: &str, img: Img) -> Result<(), String> {
    let mut st = STORE.lock().unwrap();
    let m = st.get_or_insert_with(HashMap::new);
    if !m.contains_key(id) {
        return Err(format!("invalid_input: 图像句柄 {id} 不存在"));
    }
    m.insert(id.to_string(), img);
    Ok(())
}

pub fn clear_session() {
    *STORE.lock().unwrap() = None;
}

fn handle_json(id: &str, img: &Img) -> Value {
    json!({ "id": id, "w": img.w, "h": img.h })
}

// ───────────────────────────── PNG ─────────────────────────────

fn crc32(buf: &[u8]) -> u32 {
    static TABLE: Mutex<Option<[u32; 256]>> = Mutex::new(None);
    let mut g = TABLE.lock().unwrap();
    let t = g.get_or_insert_with(|| {
        let mut t = [0u32; 256];
        for (n, e) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb88320 ^ (c >> 1) } else { c >> 1 };
            }
            *e = c;
        }
        t
    });
    let mut c = 0xffff_ffffu32;
    for b in buf {
        c = t[((c ^ *b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

pub fn encode_png(img: &Img) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    let stride = (img.w * 4) as usize;
    let mut raw = Vec::with_capacity(img.h as usize * (stride + 1));
    for y in 0..img.h as usize {
        raw.push(0); // filter: none
        raw.extend_from_slice(&img.px[y * stride..(y + 1) * stride]);
    }
    let mut z = ZlibEncoder::new(Vec::new(), Compression::new(6));
    let _ = z.write_all(&raw);
    let idat = z.finish().unwrap_or_default();

    let chunk = |ty: &[u8], data: &[u8]| {
        let mut out = Vec::with_capacity(data.len() + 12);
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut td = ty.to_vec();
        td.extend_from_slice(data);
        out.extend_from_slice(&td);
        out.extend_from_slice(&crc32(&td).to_be_bytes());
        out
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&img.w.to_be_bytes());
    ihdr.extend_from_slice(&img.h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8bit RGBA

    let mut out = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    out.extend_from_slice(&chunk(b"IHDR", &ihdr));
    out.extend_from_slice(&chunk(b"IDAT", &idat));
    out.extend_from_slice(&chunk(b"IEND", &[]));
    out
}

pub fn decode_png(b: &[u8]) -> Result<Img, String> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    if b.len() < 8 || b[..8] != [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a] {
        return Err("不是 PNG".into());
    }
    let (mut off, mut ihdr, mut idat) = (8usize, None, Vec::new());
    while off + 8 <= b.len() {
        let len = u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]) as usize;
        let ty = &b[off + 4..off + 8];
        let data = b.get(off + 8..off + 8 + len).ok_or("PNG 数据截断")?;
        match ty {
            b"IHDR" => ihdr = Some((
                u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
                u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
                data[8], data[9], data[12],
            )),
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        off += 12 + len;
    }
    let (w, h, depth, color, interlace) = ihdr.ok_or("PNG 缺 IHDR")?;
    if depth != 8 {
        return Err(format!("暂不支持 {depth} 位深的 PNG"));
    }
    if interlace != 0 {
        return Err("暂不支持隔行 PNG".into());
    }
    let ch: usize = match color {
        0 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return Err(format!("暂不支持 color type {color}")),
    };

    let mut raw = Vec::new();
    ZlibDecoder::new(&idat[..]).read_to_end(&mut raw).map_err(|e| format!("PNG 解压失败: {e}"))?;
    let stride = w as usize * ch;
    if raw.len() < h as usize * (stride + 1) {
        return Err("PNG 扫描行不完整".into());
    }

    let mut out = vec![0u8; h as usize * stride];
    let mut prev = vec![0u8; stride];
    for y in 0..h as usize {
        let ft = raw[y * (stride + 1)];
        let line = &raw[y * (stride + 1) + 1..y * (stride + 1) + 1 + stride];
        let mut cur = vec![0u8; stride];
        for i in 0..stride {
            let a = if i >= ch { cur[i - ch] as i32 } else { 0 };
            let bb = prev[i] as i32;
            let c = if i >= ch { prev[i - ch] as i32 } else { 0 };
            let v = line[i] as i32
                + match ft {
                    1 => a,
                    2 => bb,
                    3 => (a + bb) / 2,
                    4 => {
                        let p = a + bb - c;
                        let (pa, pb, pc) = ((p - a).abs(), (p - bb).abs(), (p - c).abs());
                        if pa <= pb && pa <= pc { a } else if pb <= pc { bb } else { c }
                    }
                    _ => 0,
                };
            cur[i] = (v & 0xff) as u8;
        }
        out[y * stride..(y + 1) * stride].copy_from_slice(&cur);
        prev = cur;
    }

    // 统一成 RGBA
    let n = (w * h) as usize;
    let mut px = vec![255u8; n * 4];
    for i in 0..n {
        let (s, d) = (i * ch, i * 4);
        match ch {
            4 => px[d..d + 4].copy_from_slice(&out[s..s + 4]),
            3 => {
                px[d..d + 3].copy_from_slice(&out[s..s + 3]);
            }
            2 => {
                px[d] = out[s]; px[d + 1] = out[s]; px[d + 2] = out[s]; px[d + 3] = out[s + 1];
            }
            _ => {
                px[d] = out[s]; px[d + 1] = out[s]; px[d + 2] = out[s];
            }
        }
    }
    Ok(Img { w, h, px })
}

// ───────────────────────────── ffmpeg 外援 ─────────────────────────────

fn ffmpeg() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) { &["ffmpeg.exe"] } else { &["ffmpeg"] };
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
    let mut cands: Vec<PathBuf> = vec![];
    for n in names {
        cands.push(PathBuf::from(&home).join(".uking").join("tools").join("ffmpeg").join(n));
        cands.push(PathBuf::from(&home).join(".uking").join("tools").join(n));
    }
    if let Ok(p) = std::env::var("PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for d in p.split(sep).filter(|d| !d.is_empty()) {
            for n in names {
                cands.push(Path::new(d).join(n));
            }
        }
    }
    cands.into_iter().find(|p| p.exists())
}

const NO_FFMPEG: &str = "capability_unavailable: 这一步需要 ffmpeg。到「AI 厨具」里一键装上即可（它也是剪视频用的那个）。";

/// 任意格式 → PNG 字节。PNG 直接返回，其它交给 ffmpeg。
fn to_png_bytes(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > 8 && bytes[..8] == [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a] {
        return Ok(bytes.to_vec());
    }
    let ff = ffmpeg().ok_or(NO_FFMPEG)?;
    let tmp = std::env::temp_dir().join(format!("uk-img-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let inp = tmp.join("in.bin");
    let outp = tmp.join("out.png");
    std::fs::write(&inp, bytes).map_err(|e| e.to_string())?;
    let st = std::process::Command::new(&ff)
        .args(["-v", "error", "-y", "-i"])
        .arg(&inp)
        .args(["-pix_fmt", "rgba"])
        .arg(&outp)
        .status()
        .map_err(|e| format!("ffmpeg 起不来: {e}"))?;
    let r = if st.success() {
        std::fs::read(&outp).map_err(|e| e.to_string())
    } else {
        Err("这张图解不开（格式不认识或文件损坏）".to_string())
    };
    let _ = std::fs::remove_dir_all(&tmp);
    r
}

// ───────────────────────────── base64 ─────────────────────────────

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(b: &[u8]) -> String {
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = ((c[0] as u32) << 16) | ((*c.get(1).unwrap_or(&0) as u32) << 8) | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    let mut map = [255u8; 256];
    for (i, c) in B64.iter().enumerate() {
        map[*c as usize] = i as u8;
    }
    let (mut out, mut acc, mut n) = (Vec::new(), 0u32, 0u32);
    for c in s.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = map[c as usize];
        if v == 255 {
            return Err("非法 base64".into());
        }
        acc = (acc << 6) | v as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((acc >> n) as u8);
        }
    }
    Ok(out)
}

// ───────────────────────────── 像素运算 ─────────────────────────────

fn rect_of(v: &Value, w: u32, h: u32) -> (u32, u32, u32, u32) {
    let g = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0).round().max(0.0) as u32;
    let (x, y) = (g("x").min(w.saturating_sub(1)), g("y").min(h.saturating_sub(1)));
    let (rw, rh) = (
        v.get("w").and_then(|x| x.as_f64()).unwrap_or(w as f64).round().max(1.0) as u32,
        v.get("h").and_then(|x| x.as_f64()).unwrap_or(h as f64).round().max(1.0) as u32,
    );
    (x, y, rw.min(w - x), rh.min(h - y))
}

fn crop(src: &Img, x: u32, y: u32, w: u32, h: u32) -> Img {
    let mut px = vec![0u8; (w * h * 4) as usize];
    for row in 0..h {
        let s = (((y + row) * src.w + x) * 4) as usize;
        let d = (row * w * 4) as usize;
        px[d..d + (w * 4) as usize].copy_from_slice(&src.px[s..s + (w * 4) as usize]);
    }
    Img { w, h, px }
}

/// 双线性缩放。缩小时也够用（imagefix 的回采样比例通常 <2×）。
fn resize(src: &Img, tw: u32, th: u32) -> Img {
    if src.w == tw && src.h == th {
        return src.clone();
    }
    let mut px = vec![0u8; (tw * th * 4) as usize];
    for y in 0..th {
        let sy = ((y as f32 + 0.5) * src.h as f32) / th as f32 - 0.5;
        let y0 = sy.floor().max(0.0) as u32;
        let y1 = (y0 + 1).min(src.h - 1);
        let fy = (sy - y0 as f32).clamp(0.0, 1.0);
        for x in 0..tw {
            let sx = ((x as f32 + 0.5) * src.w as f32) / tw as f32 - 0.5;
            let x0 = sx.floor().max(0.0) as u32;
            let x1 = (x0 + 1).min(src.w - 1);
            let fx = (sx - x0 as f32).clamp(0.0, 1.0);
            for c in 0..4 {
                let p = |yy: u32, xx: u32| src.px[((yy * src.w + xx) * 4 + c) as usize] as f32;
                let top = p(y0, x0) * (1.0 - fx) + p(y0, x1) * fx;
                let bot = p(y1, x0) * (1.0 - fx) + p(y1, x1) * fx;
                px[((y * tw + x) * 4 + c) as usize] = (top * (1.0 - fy) + bot * fy).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Img { w: tw, h: th, px }
}

/// 环带（rect 之内、inner 之外）的逐通道中位数与最大标准差。
/// 放在宿主算，而不是把几百万像素塞进 JSON 送去 JS —— 那样慢且撑爆内存。
fn ring_stats(img: &Img, rect: (u32, u32, u32, u32), inner: (i64, i64, i64, i64)) -> ([u8; 3], f64) {
    let (rx, ry, rw, rh) = rect;
    let (ix, iy, iw, ih) = inner;
    let mut ch: [Vec<u8>; 3] = [vec![], vec![], vec![]];
    for y in 0..rh as i64 {
        for x in 0..rw as i64 {
            if x >= ix && x < ix + iw && y >= iy && y < iy + ih {
                continue;
            }
            let i = (((ry as i64 + y) * img.w as i64 + rx as i64 + x) * 4) as usize;
            if i + 2 >= img.px.len() {
                continue;
            }
            for c in 0..3 {
                ch[c].push(img.px[i + c]);
            }
        }
    }
    if ch[0].is_empty() {
        return ([0, 0, 0], 0.0);
    }
    let mut med = [0u8; 3];
    let mut sdmax = 0.0f64;
    for c in 0..3 {
        ch[c].sort_unstable();
        med[c] = ch[c][ch[c].len() / 2];
        let m = ch[c].iter().map(|v| *v as f64).sum::<f64>() / ch[c].len() as f64;
        let sd = (ch[c].iter().map(|v| (*v as f64 - m).powi(2)).sum::<f64>() / ch[c].len() as f64).sqrt();
        sdmax = sdmax.max(sd);
    }
    (med, sdmax)
}

/// 羽化回贴：选区内全替换，向外 feather 像素线性渐隐回原图。
/// **框外每个像素逐字节不变** —— 这是算法保证，靠的就是 alpha 到边界处恰好为 0。
#[allow(clippy::too_many_arguments)]
fn composite_feather(
    base: &Img, patch: &Img, at: (i64, i64), sel: (i64, i64, i64, i64), feather: f64, offset: Option<[f64; 3]>,
) -> Img {
    let mut out = base.clone();
    let (ax, ay) = at;
    let (sx, sy, sw, sh) = sel;
    // sel 是源图坐标，换算到 patch 内坐标
    let (px0, py0) = (sx - ax, sy - ay);
    for y in 0..patch.h as i64 {
        for x in 0..patch.w as i64 {
            let dxo = ((px0 - x).max(x - (px0 + sw)).max(0)) as f64;
            let dyo = ((py0 - y).max(y - (py0 + sh)).max(0)) as f64;
            let d = (dxo * dxo + dyo * dyo).sqrt();
            let a = (1.0 - d / feather.max(1.0)).clamp(0.0, 1.0);
            if a <= 0.0 {
                continue;
            }
            let (bx, by) = (ax + x, ay + y);
            if bx < 0 || by < 0 || bx >= base.w as i64 || by >= base.h as i64 {
                continue;
            }
            let si = ((y * patch.w as i64 + x) * 4) as usize;
            let di = ((by * base.w as i64 + bx) * 4) as usize;
            for c in 0..3 {
                let mut v = patch.px[si + c] as f64;
                if let Some(o) = offset {
                    v += o[c];
                }
                let v = v.clamp(0.0, 255.0);
                out.px[di + c] = (out.px[di + c] as f64 * (1.0 - a) + v * a).round() as u8;
            }
        }
    }
    out
}

/// 透视校正：把源图里一个任意四边形，拉正成 tw×th 的矩形。
///
/// 用的是标准单位方形→四边形的射影变换（Heckbert 闭式解），不是双线性拉伸 ——
/// 双线性对付不了透视：斜着拍的证件，四条边在像平面上不是等分的，
/// 用双线性拉出来中间会鼓。
///
/// 角点顺序固定：左上、右上、右下、左下。
fn warp_perspective(src: &Img, quad: &[(f64, f64); 4], tw: u32, th: u32) -> Img {
    let (x0, y0) = quad[0];
    let (x1, y1) = quad[1];
    let (x2, y2) = quad[2];
    let (x3, y3) = quad[3];

    let (dx1, dx2, dx3) = (x1 - x2, x3 - x2, x0 - x1 + x2 - x3);
    let (dy1, dy2, dy3) = (y1 - y2, y3 - y2, y0 - y1 + y2 - y3);

    let (a, b, c, d, e, f, g, h);
    if dx3.abs() < 1e-9 && dy3.abs() < 1e-9 {
        // 退化成仿射：拍得很正的时候会走到这里
        a = x1 - x0; b = x2 - x1; c = x0;
        d = y1 - y0; e = y2 - y1; f = y0;
        g = 0.0; h = 0.0;
    } else {
        let den = dx1 * dy2 - dy1 * dx2;
        if den.abs() < 1e-12 {
            // 四点共线，没有有效的透视 —— 原样返回好过输出一张乱码
            return src.clone();
        }
        g = (dx3 * dy2 - dy3 * dx2) / den;
        h = (dx1 * dy3 - dy1 * dx3) / den;
        a = x1 - x0 + g * x1; b = x3 - x0 + h * x3; c = x0;
        d = y1 - y0 + g * y1; e = y3 - y0 + h * y3; f = y0;
    }

    let mut px = vec![0u8; (tw * th * 4) as usize];
    let (sw, sh) = (src.w as f64, src.h as f64);
    for ty in 0..th {
        let v = (ty as f64 + 0.5) / th as f64;
        for tx in 0..tw {
            let u = (tx as f64 + 0.5) / tw as f64;
            let w = g * u + h * v + 1.0;
            if w.abs() < 1e-12 { continue; }
            let sx = (a * u + b * v + c) / w;
            let sy = (d * u + e * v + f) / w;
            // 双线性采样。落在图外的按边缘钳制，不留黑边。
            let fx = sx.clamp(0.0, sw - 1.0);
            let fy = sy.clamp(0.0, sh - 1.0);
            let (ix, iy) = (fx.floor() as u32, fy.floor() as u32);
            let (jx, jy) = ((ix + 1).min(src.w - 1), (iy + 1).min(src.h - 1));
            let (rx, ry) = (fx - ix as f64, fy - iy as f64);
            let di = ((ty * tw + tx) * 4) as usize;
            for ch in 0u32..4 {
                let p = |yy: u32, xx: u32| src.px[((yy * src.w + xx) * 4 + ch) as usize] as f64;
                let top = p(iy, ix) * (1.0 - rx) + p(iy, jx) * rx;
                let bot = p(jy, ix) * (1.0 - rx) + p(jy, jx) * rx;
                px[di + ch as usize] = (top * (1.0 - ry) + bot * ry).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Img { w: tw, h: th, px }
}

fn parse_color(v: &Value) -> [u8; 3] {
    if let Some(a) = v.as_array() {
        let g = |i: usize| a.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0).clamp(0.0, 255.0) as u8;
        return [g(0), g(1), g(2)];
    }
    if let Some(s) = v.as_str() {
        let s = s.trim_start_matches('#');
        if s.len() >= 6 {
            let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0);
            return [p(0), p(2), p(4)];
        }
    }
    [0, 0, 0]
}

// ───────────────────────────── 分发 ─────────────────────────────

/// `uking.image.<verb>(...)`。args 是位置参数数组（桥那边用 Proxy 透传的）。
pub fn dispatch(verb: &str, args: &Value) -> Result<Value, String> {
    let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
    let sid = |i: usize| -> Result<String, String> {
        a(i).as_str().map(String::from).ok_or_else(|| "invalid_input: 缺少图像句柄".into())
    };

    match verb {
        "decode" => {
            let src = a(0);
            let s = src.as_str().ok_or("invalid_input: decode 需要 data URL 或路径")?;
            let bytes = if let Some(i) = s.find(";base64,") {
                b64_decode(&s[i + 8..])?
            } else if s.starts_with("data:") {
                return Err("invalid_input: 只支持 base64 形式的 data URL".into());
            } else {
                std::fs::read(s).map_err(|e| format!("读不到图片 {s}: {e}"))?
            };
            let img = decode_png(&to_png_bytes(&bytes)?)?;
            let id = put(img.clone());
            Ok(handle_json(&id, &img))
        }
        "clone" => {
            let img = get(&sid(0)?)?;
            let id = put(img.clone());
            Ok(handle_json(&id, &img))
        }
        "crop" => {
            let img = get(&sid(0)?)?;
            let (x, y, w, h) = rect_of(&a(1), img.w, img.h);
            let out = crop(&img, x, y, w, h);
            let id = put(out.clone());
            Ok(handle_json(&id, &out))
        }
        "resize" => {
            let img = get(&sid(0)?)?;
            let tw = a(1).as_u64().unwrap_or(img.w as u64).clamp(1, 8192) as u32;
            let th = a(2).as_u64().unwrap_or(img.h as u64).clamp(1, 8192) as u32;
            let out = resize(&img, tw, th);
            let id = put(out.clone());
            Ok(handle_json(&id, &out))
        }
        "encode" => {
            let img = get(&sid(0)?)?;
            Ok(json!(format!("data:image/png;base64,{}", b64_encode(&encode_png(&img)))))
        }
        // 小区域取像素给 JS 自己算（字形检测那种）。大区域走 ringStats，别把几百万像素塞进 JSON。
        "pixels" => {
            let img = get(&sid(0)?)?;
            let (x, y, w, h) = rect_of(&a(1), img.w, img.h);
            if (w as u64) * (h as u64) > 1_048_576 {
                return Err("invalid_input: 取像素区域过大（>1M 像素）；大区域统计请用 image.ringStats".into());
            }
            let sub = crop(&img, x, y, w, h);
            Ok(json!({ "w": w, "h": h, "rgba_b64": b64_encode(&sub.px) }))
        }
        "ringStats" => {
            let img = get(&sid(0)?)?;
            let (x, y, w, h) = rect_of(&a(1), img.w, img.h);
            let inner = a(2);
            let gi = |k: &str| inner.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0) as i64;
            let (med, sd) = ring_stats(&img, (x, y, w, h), (gi("x"), gi("y"), gi("w"), gi("h")));
            Ok(json!({ "median": med, "stddev": (sd * 100.0).round() / 100.0 }))
        }
        "fillRect" => {
            let id0 = sid(0)?;
            let img = get(&id0)?;
            let (x, y, w, h) = rect_of(&a(1), img.w, img.h);
            let c = parse_color(&a(2));
            let noise = a(3).get("noise").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let mut out = img.clone();
            // 平背景往往还有一点拍摄颗粒；补同方差噪声，免得那块看着像贴纸。
            // 用位置确定的伪随机，保证同样输入出同样结果（本地重绘声明了 idempotent）。
            for yy in y..y + h {
                for xx in x..x + w {
                    let i = ((yy * img.w + xx) * 4) as usize;
                    for k in 0..3 {
                        let n = if noise > 0.5 {
                            let s = (xx.wrapping_mul(73_856_093) ^ yy.wrapping_mul(19_349_663) ^ (k as u32).wrapping_mul(83_492_791)) as f64;
                            ((s % 1000.0) / 1000.0 - 0.5) * 2.0 * noise
                        } else {
                            0.0
                        };
                        out.px[i + k] = (c[k] as f64 + n).clamp(0.0, 255.0) as u8;
                    }
                    out.px[i + 3] = 255;
                }
            }
            replace(&id0, out.clone())?; // 就地：调用方继续用原句柄
            Ok(handle_json(&id0, &out))
        }
        "compositeFeather" => {
            let base = get(&sid(0)?)?;
            let patch = get(&sid(1)?)?;
            let at = a(2);
            let sel = a(3);
            let feather = a(4).as_f64().unwrap_or(8.0);
            let off = a(5).as_array().map(|o| {
                let g = |i: usize| o.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0);
                [g(0), g(1), g(2)]
            });
            let gi = |v: &Value, k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as i64;
            let out = composite_feather(
                &base, &patch,
                (gi(&at, "x"), gi(&at, "y")),
                (gi(&sel, "x"), gi(&sel, "y"), gi(&sel, "w"), gi(&sel, "h")),
                feather, off,
            );
            let id = put(out.clone());
            Ok(handle_json(&id, &out))
        }
        // warpPerspective(id, [[x,y]×4], tw, th) —— 角点顺序：左上/右上/右下/左下
        "warpPerspective" => {
            let img = get(&sid(0)?)?;
            let pts = a(1);
            let arr = pts.as_array().ok_or("invalid_input: 需要 4 个角点")?;
            if arr.len() != 4 {
                return Err(format!("invalid_input: 需要恰好 4 个角点，收到 {}", arr.len()));
            }
            let mut quad = [(0.0f64, 0.0f64); 4];
            for (i, p) in arr.iter().enumerate() {
                let (x, y) = match p {
                    Value::Array(xy) => (
                        xy.first().and_then(|v| v.as_f64()).unwrap_or(0.0),
                        xy.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0),
                    ),
                    Value::Object(_) => (
                        p.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        p.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    ),
                    _ => return Err("invalid_input: 角点要么是 [x,y] 要么是 {x,y}".into()),
                };
                quad[i] = (x, y);
            }
            let tw = a(2).as_u64().unwrap_or(0).clamp(1, 8192) as u32;
            let th = a(3).as_u64().unwrap_or(0).clamp(1, 8192) as u32;
            let out = warp_perspective(&img, &quad, tw, th);
            let id = put(out.clone());
            Ok(handle_json(&id, &out))
        }
        "drawText" => draw_text(&sid(0)?, &a(1)),
        other => Err(format!("unknown_capability: image.{other}")),
    }
}

/// 写字要字体栅格化，纯 Rust 做不了（没有字体解析器）。交给 ffmpeg 的 drawtext。
/// 先把底图存成 PNG，让 ffmpeg 把字画上去，再读回来 —— 慢一点，但字形是系统字体，
/// 中文完全可控，这正是「改字默认走本地」的立足点。
fn draw_text(id: &str, o: &Value) -> Result<Value, String> {
    let img = get(id)?;
    let ff = ffmpeg().ok_or(NO_FFMPEG)?;
    let text = o.get("text").and_then(|v| v.as_str()).unwrap_or("");
    if text.is_empty() {
        return Err("invalid_input: drawText 缺少 text".into());
    }
    let rect = o.get("rect").cloned().unwrap_or(json!({}));
    let (rx, ry, rw, rh) = rect_of(&rect, img.w, img.h);
    let color = o.get("color").map(|v| parse_color(v)).unwrap_or([0, 0, 0]);
    let fit_h = o.get("fit_height").and_then(|v| v.as_f64()).unwrap_or(rh as f64 * 0.78);
    let size = (fit_h * 0.92).round().max(8.0) as u32;
    let align = o.get("align").and_then(|v| v.as_str()).unwrap_or("center");

    let font = font_file().ok_or(
        "capability_unavailable: 找不到中文字体（微软雅黑/黑体）。系统字体缺失时无法本地重绘文字。",
    )?;

    let tmp = std::env::temp_dir().join(format!("uk-txt-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let inp = tmp.join("in.png");
    let outp = tmp.join("out.png");
    std::fs::write(&inp, encode_png(&img)).map_err(|e| e.to_string())?;

    // ffmpeg filter 的转义规则很毒：反斜杠、冒号、单引号、百分号都要处理
    let esc = |s: &str| s.replace('\\', "\\\\").replace(':', "\\:").replace('\'', "\\'").replace('%', "\\%");
    let x_expr = match align {
        "left" => format!("{}", rx + 2),
        "right" => format!("{}-tw-2", rx + rw),
        _ => format!("{}+({}-tw)/2", rx, rw),
    };
    let filter = format!(
        "drawtext=fontfile='{}':text='{}':fontcolor=0x{:02x}{:02x}{:02x}:fontsize={}:x={}:y={}+({}-th)/2",
        esc(&font.to_string_lossy()), esc(text),
        color[0], color[1], color[2], size, x_expr, ry, rh
    );

    let st = std::process::Command::new(&ff)
        .args(["-v", "error", "-y", "-i"])
        .arg(&inp)
        .args(["-vf", &filter, "-frames:v", "1"])
        .arg(&outp)
        .status()
        .map_err(|e| format!("ffmpeg 起不来: {e}"))?;
    let r = if st.success() {
        std::fs::read(&outp).map_err(|e| e.to_string()).and_then(|b| decode_png(&to_png_bytes(&b)?))
    } else {
        Err("写字失败（ffmpeg 可能没编进 freetype，装完整版即可）".to_string())
    };
    let _ = std::fs::remove_dir_all(&tmp);
    let out = r?;
    replace(id, out.clone())?; // 就地，同 fillRect
    Ok(handle_json(id, &out))
}

fn font_file() -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = vec![];
    #[cfg(windows)]
    {
        let win = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
        for n in ["msyh.ttc", "msyhbd.ttc", "simhei.ttf", "simsun.ttc", "arial.ttf"] {
            c.push(Path::new(&win).join("Fonts").join(n));
        }
    }
    #[cfg(target_os = "macos")]
    for n in ["/System/Library/Fonts/PingFang.ttc", "/System/Library/Fonts/STHeiti Medium.ttc"] {
        c.push(PathBuf::from(n));
    }
    #[cfg(target_os = "linux")]
    for n in [
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ] {
        c.push(PathBuf::from(n));
    }
    c.into_iter().find(|p| p.exists())
}
