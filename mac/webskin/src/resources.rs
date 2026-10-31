//! "res://module/type/#id" URLs that skins use for WMP's built-in strings, images and scripts.
//!
//! There is no Windows Media Player on a Mac, so these come from a copy of `wmploc.dll` (and
//! optionally its `wmploc.dll.mui` string file) that the user places in the app's support folder.
//! Without one, resources are simply missing and skins carry on.

use crate::package::decode_text;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

const RT_BITMAP: u32 = 2;
const RT_STRING: u32 = 6;
const RT_HTML: u32 = 23;
const RT_TEXT: u32 = 256;
const RT_IMAGE: u32 = 257;

thread_local! {
    static MODULES: RefCell<HashMap<String, Vec<PeResources>>> = RefCell::default();
    static CACHE: RefCell<HashMap<String, Option<Vec<u8>>>> = RefCell::default();
}

/// Where the user puts `wmploc.dll` (and `wmploc.dll.mui`).
pub fn folder() -> PathBuf {
    crate::settings::support_dir()
}

pub fn is_resource_url(s: &str) -> bool {
    s.trim_start().get(..6).is_some_and(|p| p.eq_ignore_ascii_case("res://"))
}

/// res://module/type/name; the type may be left out ("res://wmploc/#1691").
fn parse_url(url: &str) -> Option<(String, Option<String>, String)> {
    let rest = &url.trim()[6..];
    let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() < 2 {
        return None;
    }
    let (ty, name) = if parts.len() == 2 { (None, parts[1]) } else { (Some(parts[1].to_string()), parts[parts.len() - 1]) };
    (!name.is_empty() && name != "#").then(|| (parts[0].to_string(), ty, name.to_string()))
}

fn type_id(ty: Option<&str>) -> Option<u32> {
    let Some(t) = ty else { return Some(0) };
    let t = t.to_ascii_uppercase();
    let named = match t.as_str() {
        "RT_BITMAP" => Some(RT_BITMAP),
        "RT_STRING" => Some(RT_STRING),
        "RT_HTML" => Some(RT_HTML),
        "RT_TEXT" => Some(RT_TEXT),
        "RT_IMAGE" => Some(RT_IMAGE),
        _ => None,
    };
    named.or_else(|| t.trim_start_matches('#').parse().ok())
}

fn with_modules<T>(module: &str, f: impl FnOnce(&[PeResources]) -> T) -> T {
    let mut m = module.to_ascii_lowercase();
    if m == "-" || m.is_empty() || m == "wmploc" {
        m = "wmploc.dll".into();
    }
    if !m.ends_with(".dll") {
        m.push_str(".dll");
    }
    MODULES.with(|cell| {
        let mut modules = cell.borrow_mut();
        let list = modules.entry(m.clone()).or_insert_with(|| {
            let dir = folder();
            [dir.join(&m), dir.join(format!("{m}.mui"))].iter().filter_map(|p| std::fs::read(p).ok()).filter_map(PeResources::parse).collect()
        });
        f(list)
    })
}

/// Raw resource bytes (images, scripts). RT_BITMAP data is turned into a .bmp file.
pub fn read_bytes(url: &str) -> Option<Vec<u8>> {
    if let Some(hit) = CACHE.with(|c| c.borrow().get(url).cloned()) {
        return hit;
    }
    let result = parse_url(url).and_then(|(module, ty, name)| {
        let t = type_id(ty.as_deref())?;
        let types = if t > 0 { vec![t] } else { vec![RT_IMAGE, RT_BITMAP, RT_TEXT, RT_HTML] };
        with_modules(&module, |mods| {
            mods.iter().find_map(|pe| types.iter().find_map(|&ty| pe.find(ty, &name).map(|b| if ty == RT_BITMAP { dib_to_bmp(&b) } else { b })))
        })
    });
    CACHE.with(|c| c.borrow_mut().insert(url.to_string(), result.clone()));
    result
}

pub fn read_text(url: &str) -> Option<String> {
    let (module, ty, name) = parse_url(url)?;
    if type_id(ty.as_deref()) == Some(RT_STRING) {
        return load_string(&module, &name);
    }
    read_bytes(url).map(|b| decode_text(&b))
}

fn load_string(module: &str, name: &str) -> Option<String> {
    let id: u32 = name.trim_start_matches('#').parse().ok()?;
    let (block, index) = ((id >> 4) + 1, id & 15);
    with_modules(module, |mods| {
        mods.iter().find_map(|pe| {
            let data = pe.find(RT_STRING, &format!("#{block}"))?;
            let mut pos = 0usize;
            for i in 0..16 {
                let len = usize::from(u16::from_le_bytes([*data.get(pos)?, *data.get(pos + 1)?]));
                pos += 2;
                if i == index {
                    let end = (pos + len * 2).min(data.len());
                    let units: Vec<u16> = data[pos..end].as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
                    return Some(String::from_utf16_lossy(&units));
                }
                pos += len * 2;
            }
            None
        })
    })
}

fn dib_to_bmp(dib: &[u8]) -> Vec<u8> {
    let rd32 = |i: usize| dib.get(i..i + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let header_size = rd32(0);
    let bit_count = dib.get(14..16).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
    let clr_used = if header_size >= 36 { rd32(32) } else { 0 };
    let compression = rd32(16);
    let mut palette = if bit_count <= 8 { (if clr_used > 0 { clr_used } else { 1 << bit_count }) * 4 } else { 0 };
    if compression == 3 && header_size == 40 {
        palette = 12; // BI_BITFIELDS masks
    }
    let total = 14 + dib.len() as u32;
    let mut bmp = Vec::with_capacity(total as usize);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&total.to_le_bytes());
    bmp.extend_from_slice(&[0; 4]);
    bmp.extend_from_slice(&(14 + header_size + palette).to_le_bytes());
    bmp.extend_from_slice(dib);
    bmp
}

/// The resource tree of a PE file (a .dll or .mui), read without loading it.
struct PeResources {
    data: Vec<u8>,
    /// File offset of the resource directory, and the RVA it was mapped at.
    root: usize,
    rva: usize,
}

impl PeResources {
    fn parse(data: Vec<u8>) -> Option<Self> {
        let rd16 = |i: usize| data.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
        let rd32 = |i: usize| data.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
        if data.get(..2)? != b"MZ" {
            return None;
        }
        let pe = rd32(0x3C)?;
        if data.get(pe..pe + 4)? != b"PE\0\0" {
            return None;
        }
        let sections = rd16(pe + 6)?;
        let opt_size = rd16(pe + 20)?;
        let opt = pe + 24;
        let dirs = match rd16(opt)? {
            0x10B => opt + 96,
            0x20B => opt + 112,
            _ => return None,
        };
        let rva = rd32(dirs + 2 * 8)?;
        let table = opt + opt_size;
        let root = (0..sections).find_map(|s| {
            let sh = table + s * 40;
            let (vsize, vaddr, raw_size, raw_ptr) = (rd32(sh + 8)?, rd32(sh + 12)?, rd32(sh + 16)?, rd32(sh + 20)?);
            (rva >= vaddr && rva < vaddr + vsize.max(raw_size)).then_some(raw_ptr + (rva - vaddr))
        })?;
        Some(Self { data, root, rva })
    }

    fn rd16(&self, i: usize) -> Option<usize> {
        self.data.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
    }

    fn rd32(&self, i: usize) -> Option<usize> {
        self.data.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }

    /// Entries of the directory at `dir` (relative to the root): (name, is_subdir, offset).
    fn entries(&self, dir: usize) -> Vec<(EntryName, bool, usize)> {
        let base = self.root + dir;
        let (Some(named), Some(ids)) = (self.rd16(base + 12), self.rd16(base + 14)) else { return vec![] };
        (0..named + ids)
            .filter_map(|k| {
                let e = base + 16 + k * 8;
                let (name, off) = (self.rd32(e)?, self.rd32(e + 4)?);
                let name = if name & 0x8000_0000 != 0 {
                    let s = self.root + (name & 0x7FFF_FFFF);
                    let len = self.rd16(s)?;
                    let units: Vec<u16> = (0..len).filter_map(|i| self.rd16(s + 2 + i * 2).map(|u| u as u16)).collect();
                    EntryName::Str(String::from_utf16_lossy(&units))
                } else {
                    EntryName::Id(name as u32)
                };
                Some((name, off & 0x8000_0000 != 0, off & 0x7FFF_FFFF))
            })
            .collect()
    }

    /// The first language's data for (type, name); "#123" or "123" name ids, anything else by string.
    fn find(&self, ty: u32, name: &str) -> Option<Vec<u8>> {
        let want = match name.trim_start_matches('#').parse::<u32>() {
            Ok(id) => EntryName::Id(id),
            Err(_) => EntryName::Str(name.to_string()),
        };
        let (_, _, types) = self.entries(0).into_iter().find(|(n, sub, _)| *sub && *n == EntryName::Id(ty))?;
        let (_, _, names) = self.entries(types).into_iter().find(|(n, sub, _)| *sub && n.matches(&want))?;
        let (_, _, leaf) = self.entries(names).into_iter().find(|(_, sub, _)| !sub)?;
        let entry = self.root + leaf;
        let (data_rva, size) = (self.rd32(entry)?, self.rd32(entry + 4)?);
        let start = self.root + data_rva.checked_sub(self.rva)?;
        self.data.get(start..start + size).map(<[u8]>::to_vec)
    }
}

#[derive(PartialEq, Debug)]
enum EntryName {
    Id(u32),
    Str(String),
}

impl EntryName {
    fn matches(&self, other: &EntryName) -> bool {
        match (self, other) {
            (EntryName::Str(a), EntryName::Str(b)) => a.eq_ignore_ascii_case(b),
            _ => self == other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_resource_urls() {
        assert_eq!(parse_url("res://wmploc.dll/RT_TEXT/#132"), Some(("wmploc.dll".into(), Some("RT_TEXT".into()), "#132".into())));
        assert_eq!(parse_url("res://wmploc/#1691"), Some(("wmploc".into(), None, "#1691".into())));
        assert_eq!(type_id(Some("RT_STRING")), Some(RT_STRING));
        assert_eq!(type_id(Some("#257")), Some(RT_IMAGE));
        assert!(is_resource_url("  RES://x/y"));
    }

    #[test]
    fn missing_dll_means_missing_resources() {
        assert_eq!(read_bytes("res://nosuchmodule/RT_TEXT/#1"), None);
    }
}
