//! The files of one skin: a `.wmz` (zip) archive or a loose folder holding a `.wms`.
//! File names match case-insensitively and with or without sub-folders, as WMP did.

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;

const MAX_FILE: u64 = 64 * 1024 * 1024;

pub struct SkinPackage {
    /// The `.wms` skin definition, as a key into the package.
    pub definition: String,
    files: HashMap<String, Vec<u8>>,
    by_base_name: HashMap<String, String>,
}

#[derive(Debug)]
pub struct PackageError(pub String);

impl std::fmt::Display for PackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PackageError {}

impl SkinPackage {
    pub fn open(path: &Path) -> Result<Self, PackageError> {
        let source = std::fs::canonicalize(path).map_err(|e| PackageError(format!("{}: {e}", path.display())))?;
        let mut pkg = SkinPackage { definition: String::new(), files: HashMap::new(), by_base_name: HashMap::new() };
        let is_wms = source.extension().is_some_and(|e| e.eq_ignore_ascii_case("wms"));
        if is_wms {
            let dir = source.parent().unwrap_or(Path::new("/"));
            pkg.add_folder(dir, dir);
            pkg.definition = normalize(&source.file_name().unwrap_or_default().to_string_lossy());
            return Ok(pkg);
        }

        let mut bytes = std::fs::read(&source).map_err(|e| PackageError(format!("{}: {e}", source.display())))?;
        let order = match pkg.add_zip(&bytes) {
            Ok(order) => order,
            // Some skins have damaged local file headers that WMP never checked; repair and retry.
            Err(first) if repair_local_headers(&mut bytes) => {
                pkg.files.clear();
                pkg.by_base_name.clear();
                pkg.add_zip(&bytes).map_err(|_| first)?
            }
            Err(e) => return Err(e),
        };
        // Prefer the .wms named like the .wmz, then the first one at the shallowest level.
        let stem = source.file_stem().unwrap_or_default().to_string_lossy().to_lowercase();
        let defs: Vec<&String> = order.iter().filter(|k| k.to_lowercase().ends_with(".wms")).collect();
        let named = defs.iter().find(|k| file_stem(k).eq_ignore_ascii_case(&stem));
        let shallowest = defs.iter().min_by_key(|k| k.matches('/').count());
        pkg.definition =
            named.or(shallowest).map(|k| (*k).clone()).ok_or_else(|| PackageError("The skin archive has no .wms skin definition.".into()))?;
        Ok(pkg)
    }

    fn add_folder(&mut self, root: &Path, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.add_folder(root, &path);
            } else if entry.metadata().is_ok_and(|m| m.len() <= MAX_FILE)
                && let (Ok(rel), Ok(data)) = (path.strip_prefix(root), std::fs::read(&path))
            {
                self.add(&rel.to_string_lossy(), data);
            }
        }
    }

    /// Reads every entry; returns the normalized names in archive order.
    fn add_zip(&mut self, bytes: &[u8]) -> Result<Vec<String>, PackageError> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| PackageError(format!("Not a skin archive: {e}")))?;
        let mut order = Vec::new();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| PackageError(format!("Damaged skin archive: {e}")))?;
            if entry.is_dir() || entry.size() > MAX_FILE {
                continue;
            }
            let name = String::from_utf8_lossy(entry.name_raw()).into_owned();
            let mut data = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut data).map_err(|e| PackageError(format!("Damaged skin archive: {e}")))?;
            order.push(normalize(&name));
            self.add(&name, data);
        }
        Ok(order)
    }

    fn add(&mut self, name: &str, data: Vec<u8>) {
        let key = normalize(name);
        let lower = key.to_lowercase();
        self.by_base_name.entry(base_name(&lower).to_string()).or_insert_with(|| lower.clone());
        self.files.insert(lower, data);
    }

    fn resolve(&self, name: &str) -> Option<&String> {
        if name.trim().is_empty() {
            return None;
        }
        let key = normalize(name).to_lowercase();
        if let Some((k, _)) = self.files.get_key_value(&key) {
            return Some(k);
        }
        self.by_base_name.get(base_name(&key))
    }

    pub fn exists(&self, name: &str) -> bool {
        self.resolve(name).is_some()
    }

    pub fn read_bytes(&self, name: &str) -> Option<&[u8]> {
        self.resolve(name).and_then(|k| self.files.get(k)).map(Vec::as_slice)
    }

    pub fn read_text(&self, name: &str) -> Option<String> {
        self.read_bytes(name).map(decode_text)
    }
}

fn normalize(name: &str) -> String {
    let mut n = name.replace('\\', "/").trim().to_string();
    while let Some(rest) = n.strip_prefix("./") {
        n = rest.to_string();
    }
    n.trim_start_matches('/').to_string()
}

fn base_name(key: &str) -> &str {
    key.rsplit('/').next().unwrap_or(key)
}

fn file_stem(key: &str) -> &str {
    let base = base_name(key);
    base.rsplit_once('.').map_or(base, |(stem, _)| stem)
}

/// Skin text files are UTF-16 (with BOM), UTF-8 (with BOM) or the Windows ANSI code page.
pub fn decode_text(bytes: &[u8]) -> String {
    let utf16 = |b: &[u8], big: bool| {
        let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|&c| if big { u16::from_be_bytes(c) } else { u16::from_le_bytes(c) }).collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, false),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, true),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        // UTF-16 without a BOM: lots of zero high bytes.
        b if b.len() > 8 && b[1] == 0 && b[3] == 0 && b[0] != 0 => utf16(b, false),
        b => encoding_rs::WINDOWS_1252.decode_without_bom_handling(b).0.into_owned(),
    }
}

/// Rewrites "PK\x03\x04" at every local header offset listed in the central directory.
fn repair_local_headers(b: &mut [u8]) -> bool {
    let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]) as usize;
    let u32_at = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
    if b.len() < 22 {
        return false;
    }
    let low = b.len().saturating_sub(70_000);
    let Some(eocd) = (low..=b.len() - 22).rev().find(|&i| b[i..i + 4] == [0x50, 0x4B, 5, 6]) else {
        return false;
    };
    let count = u16_at(b, eocd + 10);
    let mut p = u32_at(b, eocd + 16);
    let mut changed = false;
    for _ in 0..count {
        if p + 46 > b.len() || u32_at(b, p) != 0x0201_4B50 {
            return changed;
        }
        let local = u32_at(b, p + 42);
        if local + 4 <= b.len() && u32_at(b, local) != 0x0403_4B50 {
            b[local..local + 4].copy_from_slice(&[0x50, 0x4B, 3, 4]);
            changed = true;
        }
        p += 46 + u16_at(b, p + 28) + u16_at(b, p + 30) + u16_at(b, p + 32);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_skin_text_encodings() {
        assert_eq!(decode_text(&[0xFF, 0xFE, b'h', 0, b'i', 0]), "hi");
        assert_eq!(decode_text(&[0xEF, 0xBB, 0xBF, b'o', b'k']), "ok");
        assert_eq!(decode_text(&[b'<', 0, b'V', 0, b'I', 0, b'E', 0, b'W', 0]), "<VIEW");
        assert_eq!(decode_text(&[b'c', b'a', b'f', 0xE9]), "café");
    }

    #[test]
    fn resolves_names_case_insensitively_and_by_base_name() {
        let mut pkg = SkinPackage { definition: String::new(), files: HashMap::new(), by_base_name: HashMap::new() };
        pkg.add("Images\\Play.BMP", vec![1]);
        assert_eq!(pkg.read_bytes("images/play.bmp"), Some(&[1u8][..]));
        assert_eq!(pkg.read_bytes("PLAY.bmp"), Some(&[1u8][..]));
        assert!(!pkg.exists("pause.bmp"));
    }

    #[test]
    fn repairs_a_damaged_local_header() {
        let mut data = Vec::new();
        {
            let mut w = zip::ZipWriter::new(Cursor::new(&mut data));
            w.start_file("skin.wms", zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut w, b"<THEME/>").unwrap();
            w.finish().unwrap();
        }
        data[0] = 0;
        assert!(zip::ZipArchive::new(Cursor::new(&data)).and_then(|mut z| z.by_index(0).map(|_| ())).is_err());
        assert!(repair_local_headers(&mut data));
        let mut z = zip::ZipArchive::new(Cursor::new(&data)).unwrap();
        assert_eq!(z.by_index(0).unwrap().name(), "skin.wms");
    }
}
