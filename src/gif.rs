//! Minimal GIF89a encoder: median-cut palette quantization, variable-width
//! LZW compression, per-frame delays, and an infinite loop. Platform-neutral
//! and unit-tested; the Windows export path feeds it decoded RGB frames.
//!
//! Each frame carries its own local color table, so quality scales with the
//! requested palette size without a shared global palette compromising any
//! single frame.

use std::collections::HashMap;

/// Reduce an 8-bit color to a 15-bit (5/5/5) histogram key. Clustering at
/// 5-bit precision bounds the histogram and speeds quantization; palette
/// entries themselves are averaged back at full 8-bit precision.
#[inline]
fn key555(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 >> 3) << 10) | ((g as u16 >> 3) << 5) | (b as u16 >> 3)
}

struct Bucket {
    key: u16,
    r: u8,
    g: u8,
    b: u8,
    count: u32,
}

/// Median-cut quantization. Returns the palette and a map from 5-bit color
/// key to palette index.
fn quantize(rgb: &[u8], max_colors: u16) -> (Vec<[u8; 3]>, HashMap<u16, u8>) {
    let max_colors = (max_colors.clamp(2, 256)) as usize;

    let mut hist: HashMap<u16, (u64, u64, u64, u32)> = HashMap::new();
    for px in rgb.chunks_exact(3) {
        let e = hist.entry(key555(px[0], px[1], px[2])).or_insert((0, 0, 0, 0));
        e.0 += px[0] as u64;
        e.1 += px[1] as u64;
        e.2 += px[2] as u64;
        e.3 += 1;
    }
    let mut buckets: Vec<Bucket> = hist
        .into_iter()
        .map(|(key, (sr, sg, sb, c))| Bucket {
            key,
            r: (sr / c as u64) as u8,
            g: (sg / c as u64) as u8,
            b: (sb / c as u64) as u8,
            count: c,
        })
        .collect();
    if buckets.is_empty() {
        return (vec![[0, 0, 0]], HashMap::new());
    }

    // Boxes are contiguous half-open index ranges into `buckets`.
    let mut boxes: Vec<(usize, usize)> = vec![(0, buckets.len())];

    fn spread(bs: &[Bucket]) -> (u8, u8, u8) {
        let (mut r0, mut g0, mut b0) = (255u8, 255u8, 255u8);
        let (mut r1, mut g1, mut b1) = (0u8, 0u8, 0u8);
        for x in bs {
            r0 = r0.min(x.r);
            g0 = g0.min(x.g);
            b0 = b0.min(x.b);
            r1 = r1.max(x.r);
            g1 = g1.max(x.g);
            b1 = b1.max(x.b);
        }
        (r1 - r0, g1 - g0, b1 - b0)
    }

    while boxes.len() < max_colors {
        // Pick the splittable box with the widest channel spread.
        let mut pick = None;
        let mut best = 0u16;
        for (i, &(lo, hi)) in boxes.iter().enumerate() {
            if hi - lo < 2 {
                continue;
            }
            let (dr, dg, db) = spread(&buckets[lo..hi]);
            let m = dr.max(dg).max(db) as u16;
            if m >= best {
                best = m;
                pick = Some(i);
            }
        }
        let Some(bi) = pick else { break };
        let (lo, hi) = boxes[bi];
        let (dr, dg, db) = spread(&buckets[lo..hi]);
        let axis = if dr >= dg && dr >= db {
            0
        } else if dg >= db {
            1
        } else {
            2
        };
        buckets[lo..hi].sort_by_key(|x| match axis {
            0 => x.r,
            1 => x.g,
            _ => x.b,
        });
        let total: u32 = buckets[lo..hi].iter().map(|x| x.count).sum();
        let mut acc = 0u32;
        let mut mid = lo + 1;
        for (i, x) in buckets[lo..hi].iter().enumerate() {
            acc += x.count;
            if acc * 2 >= total {
                mid = (lo + i + 1).clamp(lo + 1, hi - 1);
                break;
            }
        }
        boxes[bi] = (lo, mid);
        boxes.push((mid, hi));
    }

    let mut palette = Vec::with_capacity(boxes.len());
    let mut map: HashMap<u16, u8> = HashMap::new();
    for (idx, &(lo, hi)) in boxes.iter().enumerate() {
        let (mut sr, mut sg, mut sb, mut c) = (0u64, 0u64, 0u64, 0u64);
        for x in &buckets[lo..hi] {
            sr += x.r as u64 * x.count as u64;
            sg += x.g as u64 * x.count as u64;
            sb += x.b as u64 * x.count as u64;
            c += x.count as u64;
            map.insert(x.key, idx as u8);
        }
        let c = c.max(1);
        palette.push([(sr / c) as u8, (sg / c) as u8, (sb / c) as u8]);
    }
    (palette, map)
}

// ---------------------------------------------------------------------------
// LZW
// ---------------------------------------------------------------------------

struct BitWriter {
    out: Vec<u8>,
    acc: u32,
    nbits: u32,
}

impl BitWriter {
    fn new() -> BitWriter {
        BitWriter { out: Vec::new(), acc: 0, nbits: 0 }
    }
    fn put(&mut self, code: u16, size: u32) {
        self.acc |= (code as u32) << self.nbits;
        self.nbits += size;
        while self.nbits >= 8 {
            self.out.push((self.acc & 0xFF) as u8);
            self.acc >>= 8;
            self.nbits -= 8;
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.nbits > 0 {
            self.out.push((self.acc & 0xFF) as u8);
        }
        self.out
    }
}

/// GIF variable-width LZW. `min_code_size` is 2..=8. Returns the raw code
/// stream (not yet split into sub-blocks).
fn lzw(min_code_size: u8, indices: &[u8]) -> Vec<u8> {
    let clear = 1u16 << min_code_size;
    let end = clear + 1;
    let base = min_code_size as u32 + 1;

    let mut bw = BitWriter::new();
    let mut code_size = base;
    let mut dict: HashMap<(u16, u8), u16> = HashMap::new();
    let mut next = end + 1;

    bw.put(clear, code_size);
    if indices.is_empty() {
        bw.put(end, code_size);
        return bw.finish();
    }

    let mut cur = indices[0] as u16;
    for &b in &indices[1..] {
        if let Some(&c) = dict.get(&(cur, b)) {
            cur = c;
        } else {
            bw.put(cur, code_size);
            dict.insert((cur, b), next);
            next += 1;
            // Grow the code width one code before the table fills, matching
            // the reference GIF decoder (validated by the round-trip test).
            if next == (1 << code_size) + 1 && code_size < 12 {
                code_size += 1;
            }
            if next == 4096 {
                bw.put(clear, code_size);
                dict.clear();
                next = end + 1;
                code_size = base;
            }
            cur = b as u16;
        }
    }
    bw.put(cur, code_size);
    bw.put(end, code_size);
    bw.finish()
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

pub struct GifWriter {
    buf: Vec<u8>,
    w: u16,
    h: u16,
}

impl GifWriter {
    /// Start a GIF of the given pixel size (looping forever).
    pub fn new(w: u16, h: u16) -> GifWriter {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"GIF89a");
        buf.extend_from_slice(&w.to_le_bytes());
        buf.extend_from_slice(&h.to_le_bytes());
        buf.push(0x70); // no global color table; color resolution bits set
        buf.push(0); // background color index
        buf.push(0); // pixel aspect ratio
                     // NETSCAPE2.0 application extension: loop forever
        buf.extend_from_slice(&[0x21, 0xFF, 0x0B]);
        buf.extend_from_slice(b"NETSCAPE2.0");
        buf.extend_from_slice(&[0x03, 0x01, 0x00, 0x00, 0x00]);
        GifWriter { buf, w, h }
    }

    /// Append one frame. `rgb` is tightly packed RGB (w*h*3 bytes), `delay_cs`
    /// is the on-screen time in centiseconds, `max_colors` caps the palette.
    pub fn add_frame(&mut self, rgb: &[u8], delay_cs: u16, max_colors: u16) {
        let (palette, map) = quantize(rgb, max_colors);

        let mut bits = 1u8;
        while (1usize << bits) < palette.len() {
            bits += 1;
        }
        let table_len = 1usize << bits;
        let min_code_size = bits.max(2);

        // Graphic control extension (delay + disposal = leave in place).
        self.buf.extend_from_slice(&[0x21, 0xF9, 0x04, 0x04]);
        self.buf.extend_from_slice(&delay_cs.to_le_bytes());
        self.buf.push(0); // transparent color index (unused)
        self.buf.push(0); // block terminator

        // Image descriptor with a local color table.
        self.buf.push(0x2C);
        self.buf.extend_from_slice(&0u16.to_le_bytes()); // left
        self.buf.extend_from_slice(&0u16.to_le_bytes()); // top
        self.buf.extend_from_slice(&self.w.to_le_bytes());
        self.buf.extend_from_slice(&self.h.to_le_bytes());
        self.buf.push(0x80 | (bits - 1)); // local color table, size = bits-1

        for i in 0..table_len {
            let c = palette.get(i).copied().unwrap_or([0, 0, 0]);
            self.buf.extend_from_slice(&c);
        }

        let mut indices = Vec::with_capacity(rgb.len() / 3);
        for px in rgb.chunks_exact(3) {
            let k = key555(px[0], px[1], px[2]);
            indices.push(*map.get(&k).unwrap_or(&0));
        }

        self.buf.push(min_code_size);
        let data = lzw(min_code_size, &indices);
        for chunk in data.chunks(255) {
            self.buf.push(chunk.len() as u8);
            self.buf.extend_from_slice(chunk);
        }
        self.buf.push(0); // block terminator
    }

    /// Finish and return the complete GIF bytes.
    pub fn finish(mut self) -> Vec<u8> {
        self.buf.push(0x3B); // trailer
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference GIF-spec LZW decoder used to validate the encoder end to end.
    fn lzw_decode(min_code_size: u8, data: &[u8]) -> Vec<u8> {
        let clear = 1u16 << min_code_size;
        let end = clear + 1;
        let base = min_code_size as u32 + 1;

        // bit reader, LSB first
        let mut bitpos = 0usize;
        let read = |bitpos: &mut usize, size: u32| -> u16 {
            let mut v = 0u32;
            for i in 0..size {
                let byte = data[(*bitpos) >> 3];
                let bit = (byte >> ((*bitpos) & 7)) & 1;
                v |= (bit as u32) << i;
                *bitpos += 1;
            }
            v as u16
        };

        let mut table: Vec<Vec<u8>> = Vec::new();
        let reset = |t: &mut Vec<Vec<u8>>| {
            t.clear();
            for i in 0..clear {
                t.push(vec![i as u8]);
            }
            t.push(vec![]); // clear
            t.push(vec![]); // end
        };
        reset(&mut table);
        let mut code_size = base;
        let mut out = Vec::new();
        let mut prev: Option<u16> = None;

        loop {
            let code = read(&mut bitpos, code_size);
            if code == clear {
                reset(&mut table);
                code_size = base;
                prev = None;
                continue;
            }
            if code == end {
                break;
            }
            let entry: Vec<u8> = if (code as usize) < table.len() {
                table[code as usize].clone()
            } else {
                let Some(pc) = prev.filter(|&p| (p as usize) < table.len()) else {
                    out.push(0xFE); // sentinel: desync
                    break;
                };
                let p = table[pc as usize].clone();
                let mut e = p.clone();
                e.push(p[0]);
                e
            };
            out.extend_from_slice(&entry);
            if let Some(pc) = prev {
                let mut ne = table[pc as usize].clone();
                ne.push(entry[0]);
                table.push(ne);
                if table.len() == (1usize << code_size) && code_size < 12 {
                    code_size += 1;
                }
            }
            prev = Some(code);
        }
        out
    }

    #[test]
    fn lzw_roundtrip_small() {
        let cases: Vec<Vec<u8>> = vec![
            vec![0],
            vec![1, 1, 1, 1, 1, 1],
            vec![0, 1, 2, 3, 3, 2, 1, 0, 0, 1, 2, 3],
            (0..500).map(|i| (i % 7) as u8).collect(),
        ];
        for (ci, idx) in cases.iter().enumerate() {
            for mcs in [2u8, 4, 8] {
                if idx.iter().all(|&x| (x as u16) < (1 << mcs)) {
                    let enc = lzw(mcs, idx);
                    let dec = lzw_decode(mcs, &enc);
                    assert_eq!(&dec, idx, "case={ci} mcs={mcs} enc={enc:?}");
                }
            }
        }
    }

    #[test]
    fn lzw_roundtrip_forces_codesize_growth() {
        // A long, varied stream forces multiple code-size increases and a
        // dictionary reset; the reference decoder must still recover it.
        let idx: Vec<u8> = (0..20000u64).map(|i| ((i.wrapping_mul(2654435761)) >> 24) as u8).collect();
        let enc = lzw(8, &idx);
        assert_eq!(lzw_decode(8, &enc), idx);
    }

    #[test]
    fn quantize_preserves_few_colors() {
        // Four distinct, well-separated colors → each survives quantization.
        let colors = [[10u8, 20, 30], [200, 10, 10], [10, 200, 10], [10, 10, 200]];
        let mut rgb = Vec::new();
        for _ in 0..64 {
            for c in &colors {
                rgb.extend_from_slice(c);
            }
        }
        let (pal, map) = quantize(&rgb, 256);
        assert!(pal.len() >= 4);
        for c in &colors {
            let idx = map[&key555(c[0], c[1], c[2])] as usize;
            let p = pal[idx];
            for k in 0..3 {
                assert!((p[k] as i32 - c[k] as i32).abs() <= 8, "{p:?} vs {c:?}");
            }
        }
    }

    #[test]
    fn encode_structure() {
        let mut g = GifWriter::new(2, 2);
        let f0 = vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0];
        let f1 = vec![0, 0, 0, 255, 255, 255, 128, 128, 128, 64, 64, 64];
        g.add_frame(&f0, 50, 64);
        g.add_frame(&f1, 50, 64);
        let bytes = g.finish();
        assert_eq!(&bytes[0..6], b"GIF89a");
        assert_eq!(bytes[bytes.len() - 1], 0x3B);
        assert!(bytes.windows(11).any(|w| w == b"NETSCAPE2.0"));
        assert_eq!(bytes.iter().filter(|&&b| b == 0x2C).count(), 2); // two image descriptors
    }
}
