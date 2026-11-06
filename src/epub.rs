// rustmix-wave=v0.17.0-parser-doc-repair-v2
// rustmix-wave=epub-xml-attribute-tokenizer-repair-ready
// rustmix-wave=epub-parser-stack-isolation-ready
// rustmix-wave=epub-chapter-aware-presentation-ready
// rustmix-wave=epub-watchdog-memory-pressure-repair-ready
//! Bounded reflowable EPUB reader foundation.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use miniz_oxide::inflate::decompress_to_vec;

/// Maximum EPUB archive bytes accepted from removable storage.
pub const EPUB_ARCHIVE_BYTES_LIMIT: usize = 16 * 1024 * 1024;
/// Maximum central-directory records accepted from one EPUB.
pub const EPUB_ARCHIVE_ENTRY_LIMIT: usize = 512;
/// Maximum compressed bytes extracted for one EPUB member.
pub const EPUB_MEMBER_COMPRESSED_LIMIT: usize = 2 * 1024 * 1024;
/// Maximum decompressed bytes extracted for one EPUB member.
pub const EPUB_MEMBER_UNCOMPRESSED_LIMIT: usize = 4 * 1024 * 1024;
/// Maximum flattened reflowable text retained in RAM for one EPUB.
pub const EPUB_REFLOW_TEXT_LIMIT: usize = 2 * 1024 * 1024;
/// Maximum manifest records retained from one OPF package.
pub const EPUB_MANIFEST_LIMIT: usize = 256;
/// Maximum spine records retained from one OPF package.
pub const EPUB_SPINE_LIMIT: usize = 128;
/// Maximum TOC records rendered by the Reader UI.
pub const EPUB_TOC_LIMIT: usize = 128;
/// Dedicated parser-worker stack budget.
pub const EPUB_PARSER_WORKER_STACK_BYTES: usize = 64 * 1024;
/// Lightweight OPF-title worker stack budget.
pub const EPUB_TITLE_WORKER_STACK_BYTES: usize = 32 * 1024;

/// AO3 work metadata for the mobile AO3 Meta Box & Preface layout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpubAo3Meta {
    pub author: String,
    pub rating_text: String,
    pub warning_text: String,
    pub category_text: String,
    pub fandoms: String,
    pub relationships: String,
    pub characters: String,
    pub additional_tags: String,
    pub language: String,
    pub published: String,
    pub words: String,
    pub chapters: String,
    pub summary: String,
    pub notes: String,
}

impl Default for EpubAo3Meta {
    fn default() -> Self {
        Self {
            author: "Anonymous".into(),
            rating_text: "Not Rated".into(),
            warning_text: "No Archive Warnings Apply".into(),
            category_text: "Gen".into(),
            fandoms: "Original Work".into(),
            relationships: "None".into(),
            characters: "Original Characters".into(),
            additional_tags: "AO3, Fanfiction".into(),
            language: "English".into(),
            published: "2025-01-01".into(),
            words: "0".into(),
            chapters: "1/1".into(),
            summary: String::new(),
            notes: String::new(),
        }
    }
}

/// One reflowable EPUB TOC destination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpubTocEntry {
    pub label: String,
    pub text_offset: u64,
    pub spine_index: usize,
}

/// One readable spine chapter retained alongside flattened EPUB text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpubChapter {
    pub number: usize,
    pub label: String,
    pub text_offset: u64,
    pub text_end_offset: u64,
    pub spine_index: usize,
}

/// One bounded, reflowable EPUB book retained while the Reader session is open.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpubDocument {
    pub title: String,
    pub ao3: EpubAo3Meta,
    pub text: String,
    pub toc: Vec<EpubTocEntry>,
    pub chapters: Vec<EpubChapter>,
    pub spine_count: usize,
}

impl EpubDocument {
    #[must_use]
    pub fn text_size_bytes(&self) -> u64 {
        self.text.len() as u64
    }

    #[must_use]
    pub fn chapter_for_offset(&self, offset: u64) -> Option<&EpubChapter> {
        self.chapters.iter().find(|chapter| {
            offset >= chapter.text_offset
                && (offset < chapter.text_end_offset
                    || (offset == chapter.text_end_offset
                        && chapter.text_end_offset == self.text_size_bytes()))
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ZipEntry {
    name: String,
    flags: u16,
    method: u16,
    compressed_size: usize,
    uncompressed_size: usize,
    local_header_offset: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ZipArchive {
    bytes: Vec<u8>,
    entries: Vec<ZipEntry>,
}

impl ZipArchive {
    fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let bytes =
            fs::read(path.as_ref()).map_err(|error| format!("EPUB open failed: {error}"))?;
        if bytes.len() > EPUB_ARCHIVE_BYTES_LIMIT {
            return Err(format!(
                "EPUB archive exceeds {} byte limit",
                EPUB_ARCHIVE_BYTES_LIMIT
            ));
        }
        let eocd = find_eocd(&bytes).ok_or_else(|| "EPUB ZIP end record missing".to_string())?;
        let entry_count = read_u16(&bytes, eocd + 10)? as usize;
        let central_size = read_u32(&bytes, eocd + 12)? as usize;
        let central_offset = read_u32(&bytes, eocd + 16)? as usize;
        if entry_count > EPUB_ARCHIVE_ENTRY_LIMIT {
            return Err(format!("EPUB ZIP has too many entries: {entry_count}"));
        }
        let central_end = central_offset
            .checked_add(central_size)
            .ok_or_else(|| "EPUB ZIP directory overflow".to_string())?;
        if central_end > bytes.len() {
            return Err("EPUB ZIP directory exceeds archive".into());
        }
        let mut entries = Vec::new();
        let mut cursor = central_offset;
        for _ in 0..entry_count {
            if read_u32(&bytes, cursor)? != 0x0201_4B50 {
                return Err("EPUB ZIP central record signature mismatch".into());
            }
            let flags = read_u16(&bytes, cursor + 8)?;
            let method = read_u16(&bytes, cursor + 10)?;
            let compressed_size = read_u32(&bytes, cursor + 20)? as usize;
            let uncompressed_size = read_u32(&bytes, cursor + 24)? as usize;
            let name_len = read_u16(&bytes, cursor + 28)? as usize;
            let extra_len = read_u16(&bytes, cursor + 30)? as usize;
            let comment_len = read_u16(&bytes, cursor + 32)? as usize;
            let local_header_offset = read_u32(&bytes, cursor + 42)? as usize;
            let name_start = cursor + 46;
            let name_end = name_start
                .checked_add(name_len)
                .ok_or_else(|| "EPUB ZIP filename overflow".to_string())?;
            if name_end > central_end {
                return Err("EPUB ZIP filename exceeds directory".into());
            }
            let name = String::from_utf8_lossy(&bytes[name_start..name_end]).replace('\\', "/");
            entries.push(ZipEntry {
                name,
                flags,
                method,
                compressed_size,
                uncompressed_size,
                local_header_offset,
            });
            cursor = name_end
                .checked_add(extra_len)
                .and_then(|value| value.checked_add(comment_len))
                .ok_or_else(|| "EPUB ZIP central record overflow".to_string())?;
            if cursor > central_end {
                return Err("EPUB ZIP central record exceeds directory".into());
            }
        }
        Ok(Self { bytes, entries })
    }

    fn entry(&self, name: &str) -> Option<&ZipEntry> {
        self.entries
            .iter()
            .find(|entry| entry.name == name)
            .or_else(|| {
                self.entries
                    .iter()
                    .find(|entry| entry.name.eq_ignore_ascii_case(name))
            })
    }

    fn extract(&self, name: &str) -> Result<Vec<u8>, String> {
        let entry = self
            .entry(name)
            .ok_or_else(|| format!("EPUB member missing: {name}"))?;
        if entry.flags & 0x0001 != 0 {
            return Err(format!(
                "Encrypted EPUB member is unsupported: {}",
                entry.name
            ));
        }
        if entry.compressed_size > EPUB_MEMBER_COMPRESSED_LIMIT {
            return Err(format!(
                "EPUB member compressed size is too large: {}",
                entry.name
            ));
        }
        if entry.uncompressed_size > EPUB_MEMBER_UNCOMPRESSED_LIMIT {
            return Err(format!(
                "EPUB member expanded size is too large: {}",
                entry.name
            ));
        }
        let offset = entry.local_header_offset;
        if read_u32(&self.bytes, offset)? != 0x0403_4B50 {
            return Err(format!("EPUB local ZIP header mismatch: {}", entry.name));
        }
        let name_len = read_u16(&self.bytes, offset + 26)? as usize;
        let extra_len = read_u16(&self.bytes, offset + 28)? as usize;
        let data_start = offset
            .checked_add(30)
            .and_then(|value| value.checked_add(name_len))
            .and_then(|value| value.checked_add(extra_len))
            .ok_or_else(|| "EPUB ZIP data offset overflow".to_string())?;
        let data_end = data_start
            .checked_add(entry.compressed_size)
            .ok_or_else(|| "EPUB ZIP member overflow".to_string())?;
        if data_end > self.bytes.len() {
            return Err(format!("EPUB ZIP member exceeds archive: {}", entry.name));
        }
        let compressed = &self.bytes[data_start..data_end];
        let output = match entry.method {
            0 => compressed.to_vec(),
            8 => decompress_to_vec(compressed)
                .map_err(|error| format!("EPUB deflate failed for {}: {error:?}", entry.name))?,
            method => {
                return Err(format!(
                    "Unsupported EPUB compression method {method} for {}",
                    entry.name
                ))
            }
        };
        if output.len() > EPUB_MEMBER_UNCOMPRESSED_LIMIT {
            return Err(format!("EPUB member expanded beyond limit: {}", entry.name));
        }
        if entry.uncompressed_size != 0 && output.len() != entry.uncompressed_size {
            return Err(format!("EPUB member size mismatch: {}", entry.name));
        }
        Ok(output)
    }
}

// ---------------------------------------------------------------------------
// ZIP primitives
// ---------------------------------------------------------------------------

fn find_eocd(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 22 {
        return None;
    }
    let min = bytes.len().saturating_sub(22 + 0xFFFF);
    (min..=bytes.len() - 22)
        .rev()
        .find(|&index| bytes[index..index + 4] == [0x50, 0x4B, 0x05, 0x06])
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| "EPUB ZIP read overflow".to_string())?;
    let slice = bytes
        .get(offset..end)
        .ok_or_else(|| "EPUB ZIP read out of bounds".to_string())?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| "EPUB ZIP read overflow".to_string())?;
    let slice = bytes
        .get(offset..end)
        .ok_or_else(|| "EPUB ZIP read out of bounds".to_string())?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn utf8_member(archive: &ZipArchive, name: &str) -> Result<String, String> {
    let bytes = archive.extract(name)?;
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
    };
    Ok(text.trim_start_matches('\u{FEFF}').to_string())
}

// ---------------------------------------------------------------------------
// OPF package
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct ManifestItem {
    id: String,
    href: String,
    media_type: String,
    properties: String,
}

/// Returns `(opf_path, opf_xml, opf_directory)`.
fn epub_package(archive: &ZipArchive) -> Result<(String, String, String), String> {
    let container = utf8_member(archive, "META-INF/container.xml")?;
    let rootfile = first_open_tag(&container, "rootfile")
        .and_then(|tag| attribute(tag, "full-path"))
        .ok_or_else(|| "EPUB container rootfile missing".to_string())?;
    let opf_path = rootfile.trim_start_matches('/').replace('\\', "/");
    let package = utf8_member(archive, &opf_path)?;
    let package_dir = opf_path
        .rsplit_once('/')
        .map_or_else(String::new, |(dir, _)| dir.to_string());
    Ok((opf_path, package, package_dir))
}

fn package_title(package: &str) -> String {
    first_element_text(package, "title")
        .map(|title| title.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

fn parse_manifest(package: &str) -> Result<BTreeMap<String, ManifestItem>, String> {
    let mut manifest = BTreeMap::new();
    for tag in all_open_tags(package, "item") {
        if manifest.len() >= EPUB_MANIFEST_LIMIT {
            break;
        }
        let (Some(id), Some(href)) = (attribute(tag, "id"), attribute(tag, "href")) else {
            continue;
        };
        let item = ManifestItem {
            id: id.clone(),
            href,
            media_type: attribute(tag, "media-type").unwrap_or_default(),
            properties: attribute(tag, "properties").unwrap_or_default(),
        };
        manifest.insert(id, item);
    }
    if manifest.is_empty() {
        return Err("EPUB manifest is empty".into());
    }
    Ok(manifest)
}

fn parse_spine_ids(package: &str) -> Result<Vec<String>, String> {
    Ok(all_open_tags(package, "itemref")
        .into_iter()
        .filter_map(|tag| attribute(tag, "idref"))
        .take(EPUB_SPINE_LIMIT)
        .collect())
}

fn normalize_archive_path(base_dir: &str, href: &str) -> String {
    let href = percent_decode(href.split('#').next().unwrap_or(""));
    let joined = if href.starts_with('/') {
        href.trim_start_matches('/').to_string()
    } else if base_dir.is_empty() {
        href.clone()
    } else {
        format!("{base_dir}/{href}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value),
        }
    }
    parts.join("/")
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = (bytes[index + 1] as char).to_digit(16);
            let low = (bytes[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                output.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn fallback_chapter_label(xhtml: &str, index: usize) -> String {
    for name in ["h1", "h2", "h3", "title"] {
        if let Some(text) = first_element_text(xhtml, name) {
            let label = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if !label.is_empty() {
                return label.chars().take(60).collect();
            }
        }
    }
    format!("Chapter {}", index + 1)
}

// ---------------------------------------------------------------------------
// Navigation (EPUB 3 nav / EPUB 2 NCX)
// ---------------------------------------------------------------------------

fn parse_navigation_toc(
    archive: &ZipArchive,
    package: &str,
    package_dir: &str,
    manifest: &BTreeMap<String, ManifestItem>,
    chapter_offsets: &BTreeMap<String, (usize, u64)>,
) -> Result<Vec<EpubTocEntry>, String> {
    let mut candidates: Vec<(&ManifestItem, bool)> = Vec::new();
    if let Some(item) = manifest
        .values()
        .find(|item| item.properties.split_whitespace().any(|value| value == "nav"))
    {
        candidates.push((item, true));
    }
    let spine_toc = first_open_tag(package, "spine").and_then(|tag| attribute(tag, "toc"));
    if let Some(item) = spine_toc
        .as_ref()
        .and_then(|id| manifest.get(id))
        .or_else(|| {
            manifest
                .values()
                .find(|item| item.media_type == "application/x-dtbncx+xml")
        })
    {
        candidates.push((item, false));
    }

    for (item, is_nav) in candidates {
        let member = normalize_archive_path(package_dir, &item.href);
        let Ok(source) = utf8_member(archive, &member) else {
            continue;
        };
        let links = if is_nav {
            anchor_links(toc_nav_region(&source))
        } else {
            ncx_links(&source)
        };
        let base_dir = member.rsplit_once('/').map_or("", |(dir, _)| dir);
        let mut toc = Vec::new();
        for (href, label) in links {
            if toc.len() >= EPUB_TOC_LIMIT {
                break;
            }
            let target = normalize_archive_path(base_dir, &href);
            if let Some(&(spine_index, text_offset)) = chapter_offsets.get(&target) {
                toc.push(EpubTocEntry {
                    label,
                    text_offset,
                    spine_index,
                });
            }
        }
        if !toc.is_empty() {
            return Ok(toc);
        }
    }
    Ok(Vec::new())
}

fn toc_nav_region(xhtml: &str) -> &str {
    for marker in ["epub:type=\"toc\"", "epub:type='toc'"] {
        if let Some(position) = xhtml.find(marker) {
            let start = xhtml[..position].rfind('<').unwrap_or(0);
            let end = xhtml[position..]
                .find("</nav>")
                .map_or(xhtml.len(), |relative| position + relative);
            return &xhtml[start..end];
        }
    }
    xhtml
}

fn anchor_links(xml: &str) -> Vec<(String, String)> {
    let mut links = Vec::new();
    let mut cursor = 0;
    while let Some(start_rel) = xml[cursor..].find('<') {
        let start = cursor + start_rel;
        let Some(end_rel) = xml[start + 1..].find('>') else {
            break;
        };
        let end = start + 1 + end_rel;
        let tag = xml[start + 1..end].trim_start();
        cursor = end + 1;
        if tag.starts_with('/') {
            continue;
        }
        let name = tag
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("");
        if name.rsplit(':').next() != Some("a") {
            continue;
        }
        let Some(href) = attribute(tag, "href") else {
            continue;
        };
        let close = format!("</{name}>");
        let Some(close_rel) = xml[cursor..].find(&close) else {
            continue;
        };
        let label = html_to_text(&xml[cursor..cursor + close_rel])
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        cursor += close_rel + close.len();
        if !label.is_empty() {
            links.push((href, label));
        }
    }
    links
}

fn ncx_links(xml: &str) -> Vec<(String, String)> {
    let starts: Vec<usize> = xml.match_indices("<navPoint").map(|(index, _)| index).collect();
    let mut links = Vec::new();
    for (position, &start) in starts.iter().enumerate() {
        let end = starts.get(position + 1).copied().unwrap_or(xml.len());
        let segment = &xml[start..end];
        let label = first_element_text(segment, "text");
        let src = first_open_tag(segment, "content").and_then(|tag| attribute(tag, "src"));
        if let (Some(label), Some(src)) = (label, src) {
            links.push((src, label));
        }
    }
    links
}

fn dedupe_toc(toc: &mut Vec<EpubTocEntry>) {
    let mut seen = BTreeSet::new();
    toc.retain(|entry| seen.insert(entry.text_offset));
}

// ---------------------------------------------------------------------------
// Workers
// ---------------------------------------------------------------------------

pub fn open_epub_on_worker(path: impl AsRef<Path>) -> Result<EpubDocument, String> {
    let path = path.as_ref().to_path_buf();
    log::info!(
        "rustmix-wave=epub-parser-worker status=starting stack-bytes={}",
        EPUB_PARSER_WORKER_STACK_BYTES
    );
    let worker = std::thread::Builder::new()
        .name("epub-parser".into())
        .stack_size(EPUB_PARSER_WORKER_STACK_BYTES)
        .spawn(move || open_epub(path))
        .map_err(|error| {
            let message = format!("EPUB parser worker start failed: {error}");
            log::warn!("rustmix-wave=epub-parser-worker status=start-failed error={message}");
            message
        })?;
    let result = worker.join().map_err(|_| {
        let message = "EPUB parser worker panicked".to_string();
        log::warn!("rustmix-wave=epub-parser-worker status=panicked");
        message
    })?;
    match &result {
        Ok(document) => log::info!(
            "rustmix-wave=epub-parser-worker status=completed spine-items={} toc-entries={} text-bytes={}",
            document.spine_count,
            document.toc.len(),
            document.text_size_bytes()
        ),
        Err(error) => log::warn!("rustmix-wave=epub-parser-worker status=failed error={error}"),
    }
    result
}

fn read_epub_title(path: impl AsRef<Path>) -> Result<String, String> {
    let archive = ZipArchive::open(path)?;
    let (_, package, _) = epub_package(&archive)?;
    Ok(package_title(&package))
}

pub fn read_epub_title_on_worker(path: impl AsRef<Path>) -> Result<String, String> {
    let path = path.as_ref().to_path_buf();
    let worker = std::thread::Builder::new()
        .name("epub-title".into())
        .stack_size(EPUB_TITLE_WORKER_STACK_BYTES)
        .spawn(move || read_epub_title(path))
        .map_err(|error| format!("EPUB title worker start failed: {error}"))?;
    worker
        .join()
        .map_err(|_| "EPUB title worker panicked".to_string())?
}

pub fn read_epub_ao3_on_worker(path: impl AsRef<Path>) -> Result<(String, EpubAo3Meta), String> {
    let path = path.as_ref().to_path_buf();
    let worker = std::thread::Builder::new()
        .name("epub-ao3".into())
        .stack_size(EPUB_TITLE_WORKER_STACK_BYTES)
        .spawn(move || -> Result<(String, EpubAo3Meta), String> {
            let archive = ZipArchive::open(path)?;
            let (_, package, package_dir) = epub_package(&archive)?;
            let title = package_title(&package);
            let mut preface_text = String::new();
            if let (Ok(manifest), Ok(spine_ids)) =
                (parse_manifest(&package), parse_spine_ids(&package))
            {
                if let Some(first_id) = spine_ids.first() {
                    if let Some(item) = manifest.get(first_id) {
                        let member = normalize_archive_path(&package_dir, &item.href);
                        if let Ok(xhtml) = utf8_member(&archive, &member) {
                            preface_text = html_to_text(&xhtml);
                        }
                    }
                }
            }
            let ao3 = parse_ao3_opf_metadata(&package, &preface_text, 0, 1);
            Ok((title, ao3))
        })
        .map_err(|error| format!("EPUB AO3 worker start failed: {error}"))?;
    worker
        .join()
        .map_err(|_| "EPUB AO3 worker panicked".to_string())?
}

/// Detect whether spine item 0 is AO3's auto-generated preface page (`*_split_000.xhtml`).
/// Detect whether an initial AO3 spine item (`split_000.xhtml` or `split_001.xhtml`)
/// is part of AO3's auto-generated Preface (tags table or Title/Byline/Summary page)
/// so we extract metadata from it without repeating it inside Chapter 1.
fn is_ao3_preface_spine(
    spine_index: usize,
    member: &str,
    chapter_text: &str,
    total_spine_items: usize,
) -> bool {
    if total_spine_items <= 1 || spine_index > 1 {
        return false;
    }
    let lower_member = member.to_ascii_lowercase();
    // Spine 0: AO3 metadata table (split_000.xhtml)
    if spine_index == 0
        && (lower_member.ends_with("split_000.xhtml")
            || lower_member.ends_with("split_000.html")
            || ((chapter_text.contains("Archive Warning:") || chapter_text.contains("Rating:"))
                && (chapter_text.contains("Fandom:") || chapter_text.contains("Fandoms:"))))
    {
        return true;
    }
    // Spine 1: AO3 title/summary/notes preface sheet (split_001.xhtml) when short (< 2500 chars)
    if spine_index == 1
        && chapter_text.len() < 2500
        && (lower_member.ends_with("split_001.xhtml")
            || lower_member.ends_with("split_001.html")
            || chapter_text.contains("Summary")
            || chapter_text.contains("\nby "))
    {
        return true;
    }
    false
}

#[inline(never)]
pub fn open_epub(path: impl AsRef<Path>) -> Result<EpubDocument, String> {
    let archive = ZipArchive::open(path)?;
    let (_, package, package_dir) = epub_package(&archive)?;
    let title = package_title(&package);

    let manifest = parse_manifest(&package)?;
    let spine_ids = parse_spine_ids(&package)?;
    if spine_ids.is_empty() {
        return Err("EPUB spine is empty".into());
    }

    let mut preface_text = String::new();
    let mut text = String::new();
    let mut chapter_offsets = BTreeMap::new();
    let mut chapter_labels = Vec::new();
    let mut chapters = Vec::new();

    for (spine_index, idref) in spine_ids.iter().enumerate() {
        let item = manifest
            .get(idref)
            .ok_or_else(|| format!("EPUB spine item missing from manifest: {idref}"))?;
        let member = normalize_archive_path(&package_dir, &item.href);
        let xhtml = utf8_member(&archive, &member)?;
        let chapter = html_to_text(&xhtml);
        if chapter.trim().is_empty() {
            continue;
        }

        // Route AO3 preface spine items (split_000 and split_001) into preface_text only!
        if is_ao3_preface_spine(spine_index, &member, &chapter, spine_ids.len()) {
            if !preface_text.is_empty() {
                preface_text.push_str("\n\n");
            }
            preface_text.push_str(chapter.trim());
            continue;
        }

        // Also strip duplicate leading Chapter Title header if the chapter starts with it
        let cleaned_chapter = strip_leading_duplicate_preface(&chapter, &title);

        if !text.is_empty() {
            text.push_str("\n\n");
        }
        let offset = text.len() as u64;
        let label = fallback_chapter_label(&xhtml, chapters.len());
        chapter_offsets.insert(member.clone(), (spine_index, offset));
        chapter_labels.push((spine_index, offset, label.clone()));
        text.push_str(cleaned_chapter.trim());
        let text_end_offset = text.len() as u64;
        chapters.push(EpubChapter {
            number: chapters.len() + 1,
            label,
            text_offset: offset,
            text_end_offset,
            spine_index,
        });
        if text.len() > EPUB_REFLOW_TEXT_LIMIT {
            return Err(format!(
                "EPUB reflow text exceeds {} byte limit",
                EPUB_REFLOW_TEXT_LIMIT
            ));
        }
    }
    if text.trim().is_empty() {
        return Err("EPUB spine did not contain readable text".into());
    }

    let metadata_source = if preface_text.is_empty() {
        text.as_str()
    } else {
        preface_text.as_str()
    };
    let word_count = text.split_whitespace().count();
    let ao3 = parse_ao3_opf_metadata(&package, metadata_source, word_count, chapters.len());

    let mut toc = parse_navigation_toc(
        &archive,
        &package,
        &package_dir,
        &manifest,
        &chapter_offsets,
    )?;
    if toc.is_empty() {
        toc = chapter_labels
            .into_iter()
            .take(EPUB_TOC_LIMIT)
            .map(|(spine_index, text_offset, label)| EpubTocEntry {
                label,
                text_offset,
                spine_index,
            })
            .collect();
    }
    dedupe_toc(&mut toc);
    toc.truncate(EPUB_TOC_LIMIT);
    Ok(EpubDocument {
        title,
        ao3,
        text,
        toc,
        chapters,
        spine_count: spine_ids.len(),
    })
}

/// Removes repeated title/author/chapter-notes lines at the top of the first story chapter.
fn strip_leading_duplicate_preface<'a>(chapter: &'a str, title: &str) -> &'a str {
    let mut rest = chapter.trim();
    if let Some(after_title) = rest.strip_prefix(title) {
        rest = after_title.trim_start();
    }
    if let Some(notes_end) = rest.find("(See the end of the chapter for notes.)") {
        rest = rest[notes_end + "(See the end of the chapter for notes.)".len()..].trim_start();
    } else if let Some(notes_end) = rest.find("(See the end of the work for") {
        if let Some(close_paren) = rest[notes_end..].find(')') {
            rest = rest[notes_end + close_paren + 1..].trim_start();
        }
    }
    rest
}

// ---------------------------------------------------------------------------
// AO3 metadata
// ---------------------------------------------------------------------------

fn strip_any<'a>(line: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    prefixes.iter().find_map(|prefix| line.strip_prefix(*prefix))
}

fn stat_value(line: &str, next: &str, key: &str) -> Option<String> {
    let position = line.find(key)?;
    let rest = line[position + key.len()..].trim();
    rest.split_whitespace()
        .next()
        .or_else(|| next.split_whitespace().next())
        .map(str::to_string)
}

fn parse_ao3_opf_metadata(
    package: &str,
    flattened_text: &str,
    word_count: usize,
    chapter_count: usize,
) -> EpubAo3Meta {
    let mut meta = EpubAo3Meta::default();
    if word_count > 0 {
        meta.words = word_count.to_string();
    }
    let ch_total = chapter_count.max(1);
    meta.chapters = format!("{ch_total}/{ch_total}");

    if let Some(creator) = first_element_text(package, "creator").filter(|s| !s.trim().is_empty()) {
        meta.author = creator.trim().to_string();
    }
    if let Some(lang) = first_element_text(package, "language").filter(|s| !s.trim().is_empty()) {
        meta.language = if lang.trim().eq_ignore_ascii_case("en") {
            "English".into()
        } else {
            lang.trim().to_string()
        };
    }
    if let Some(date) = first_element_text(package, "date").filter(|s| !s.trim().is_empty()) {
        meta.published = date.trim().chars().take(10).collect();
    }
    if let Some(desc) = first_element_text(package, "description").filter(|s| !s.trim().is_empty())
    {
        // OPF descriptions are usually escaped HTML; flatten them a second time.
        let clean_desc = html_to_text(&desc).replace('\n', " ");
        meta.summary = clean_desc.split_whitespace().collect::<Vec<_>>().join(" ");
    }

    let subjects: Vec<String> = all_element_texts(package, "subject");
    let mut fandoms = Vec::new();
    let mut ships = Vec::new();
    let mut freeform = Vec::new();

    for subj in subjects {
        let lower: String = subj.to_ascii_lowercase();
        if lower.contains("explicit")
            || lower.contains("mature")
            || lower.contains("teen and up")
            || lower.contains("general audiences")
            || lower.contains("not rated")
        {
            meta.rating_text = subj;
        } else if lower.contains("archive warning")
            || lower.contains("depictions of violence")
            || lower.contains("character death")
            || lower.contains("underage")
            || lower.contains("non-con")
        {
            meta.warning_text = subj;
        } else if matches!(subj.as_str(), "Gen" | "F/F" | "F/M" | "M/M" | "Multi" | "Other") {
            meta.category_text = subj;
        } else if subj.contains('/') || subj.contains(" & ") {
            ships.push(subj);
        } else if lower != "fanfiction" && !lower.starts_with("ao3") {
            if fandoms.is_empty() {
                fandoms.push(subj);
            } else {
                freeform.push(subj);
            }
        }
    }
    if !fandoms.is_empty() {
        meta.fandoms = fandoms.join(", ");
    }
    if !ships.is_empty() {
        meta.relationships = ships.join(", ");
    }
    if !freeform.is_empty() {
        meta.additional_tags = freeform.join(", ");
    }

    // Scan AO3's preface text (split_000.xhtml) for exact tags, stats, summary and notes.
    let preface: String = flattened_text.chars().take(4000).collect();
    let lines: Vec<&str> = preface
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    for (i, line) in lines.iter().copied().enumerate() {
        let next = lines.get(i + 1).copied().unwrap_or("");
        let pick = |value: &str| -> String {
            if value.trim().is_empty() {
                next.to_string()
            } else {
                value.trim().to_string()
            }
        };
        if let Some(v) = strip_any(line, &["Rating:"]) {
            meta.rating_text = pick(v);
        } else if let Some(v) = strip_any(line, &["Archive Warnings:", "Archive Warning:"]) {
            meta.warning_text = pick(v);
        } else if let Some(v) = strip_any(line, &["Categories:", "Category:"]) {
            meta.category_text = pick(v);
        } else if let Some(v) = strip_any(line, &["Fandoms:", "Fandom:"]) {
            meta.fandoms = pick(v);
        } else if let Some(v) = strip_any(line, &["Relationships:", "Relationship:"]) {
            meta.relationships = pick(v);
        } else if let Some(v) = strip_any(line, &["Characters:", "Character:"]) {
            meta.characters = pick(v);
        } else if let Some(v) = strip_any(line, &["Additional Tags:"]) {
            meta.additional_tags = pick(v);
        } else if line == "Summary" || line == "Summary:" {
            if !next.is_empty() {
                meta.summary = next.into();
            }
        } else if line == "Notes" || line == "Notes:" {
            let mut note_lines = Vec::new();
            for n_line in lines.iter().skip(i + 1).take(8) {
                if n_line.starts_with("Chapter ") || n_line.ends_with("Notes:") {
                    break;
                }
                note_lines.push(*n_line);
            }
            if !note_lines.is_empty() {
                meta.notes = note_lines.join(" ");
            }
        }

        if let Some(value) = stat_value(line, next, "Words:")
            .filter(|v| v.chars().all(|c| c.is_ascii_digit() || c == ','))
        {
            meta.words = value;
        }
        if let Some(value) = stat_value(line, next, "Chapters:").filter(|v| v.contains('/')) {
            meta.chapters = value;
        }
        if let Some(value) = stat_value(line, next, "Published:")
            .filter(|v| v.len() >= 8 && v.chars().all(|c| c.is_ascii_digit() || c == '-'))
        {
            meta.published = value;
        }
    }
    meta
}

// ---------------------------------------------------------------------------
// XHTML / XML helpers
// ---------------------------------------------------------------------------

pub fn html_to_text(html: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    while cursor < html.len() {
        let rest = &html[cursor..];
        if rest.starts_with('<') {
            let Some(end_rel) = rest.find('>') else { break };
            let tag = rest[1..end_rel].trim();
            let closing = tag.starts_with('/');
            let name = tag
                .trim_start_matches('/')
                .split(|character: char| character.is_whitespace() || character == '/')
                .next()
                .unwrap_or("")
                .rsplit(':')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(
                name.as_str(),
                "p" | "div"
                    | "section"
                    | "article"
                    | "blockquote"
                    | "li"
                    | "ul"
                    | "ol"
                    | "dl"
                    | "dt"
                    | "dd"
                    | "tr"
                    | "table"
                    | "hr"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "br"
            ) {
                push_newline(&mut output);
                if closing || name == "br" {
                    push_newline(&mut output);
                }
            }
            cursor += end_rel + 1;
            continue;
        }
        if rest.starts_with('&') {
            if let Some(end_rel) = rest.find(';').filter(|value| *value <= 12) {
                let decoded = decode_entity(&rest[1..end_rel]);
                for character in decoded.chars() {
                    push_text_character(&mut output, character);
                }
                cursor += end_rel + 1;
                continue;
            }
        }
        let character = rest.chars().next().unwrap_or(' ');
        push_text_character(&mut output, character);
        cursor += character.len_utf8();
    }
    output.trim().to_string()
}

fn push_text_character(output: &mut String, character: char) {
    if character == '\r' {
        return;
    }
    if character == '\n' || character == '\t' || character.is_whitespace() {
        if !output.ends_with(' ') && !output.ends_with('\n') && !output.is_empty() {
            output.push(' ');
        }
    } else {
        output.push(character);
    }
}

fn push_newline(output: &mut String) {
    while output.ends_with(' ') {
        output.pop();
    }
    if !output.ends_with("\n\n") && !output.is_empty() {
        output.push('\n');
    }
}

fn decode_entities(value: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    while cursor < value.len() {
        let rest = &value[cursor..];
        if rest.starts_with('&') {
            if let Some(end_rel) = rest.find(';').filter(|value| *value <= 12) {
                output.push_str(&decode_entity(&rest[1..end_rel]));
                cursor += end_rel + 1;
                continue;
            }
        }
        let character = rest.chars().next().unwrap_or(' ');
        output.push(character);
        cursor += character.len_utf8();
    }
    output
}

fn decode_entity(entity: &str) -> String {
    match entity {
        "amp" => "&".into(),
        "lt" => "<".into(),
        "gt" => ">".into(),
        "quot" => "\"".into(),
        "apos" => "'".into(),
        "nbsp" => " ".into(),
        value if value.starts_with("#x") || value.starts_with("#X") => {
            u32::from_str_radix(&value[2..], 16)
                .ok()
                .and_then(char::from_u32)
                .map_or_else(|| "?".into(), |character| character.to_string())
        }
        value if value.starts_with('#') => value[1..]
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map_or_else(|| "?".into(), |character| character.to_string()),
        _ => "?".into(),
    }
}

/// Every opening (or self-closing) tag whose local name matches, returned
/// without the surrounding `<` / `>`.
fn all_open_tags<'a>(xml: &'a str, local_name: &str) -> Vec<&'a str> {
    let mut tags = Vec::new();
    let mut cursor = 0;
    while let Some(start_rel) = xml[cursor..].find('<') {
        let start = cursor + start_rel;
        let Some(end_rel) = xml[start + 1..].find('>') else {
            break;
        };
        let end = start + 1 + end_rel;
        let tag = xml[start + 1..end].trim_start();
        cursor = end + 1;
        if tag.starts_with('/') || tag.starts_with('!') || tag.starts_with('?') {
            continue;
        }
        let name = tag
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("");
        if name.rsplit(':').next() == Some(local_name) {
            tags.push(tag);
        }
    }
    tags
}

fn first_open_tag<'a>(xml: &'a str, local_name: &str) -> Option<&'a str> {
    all_open_tags(xml, local_name).into_iter().next()
}

/// Read one attribute value from a tag body such as `item id='x' href="y"/`.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut i = 0;
    // Skip the element name.
    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'/' {
        i += 1;
    }
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        let key_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'='
            && bytes[i] != b'/'
        {
            i += 1;
        }
        let key = &tag[key_start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            if key.is_empty() {
                i += 1;
            }
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let value = if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i] as char;
            let start = i + 1;
            let end = tag[start..]
                .find(quote)
                .map_or(tag.len(), |relative| start + relative);
            i = (end + 1).min(tag.len());
            &tag[start..end]
        } else {
            let start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            tag[start..i].trim_end_matches('/')
        };
        let key_local = key.rsplit(':').next().unwrap_or(key);
        if key == name || key_local == name {
            return Some(decode_entities(value));
        }
    }
    None
}

fn first_element_text(xml: &str, local_name: &str) -> Option<String> {
    all_element_texts(xml, local_name).into_iter().next()
}

fn all_element_texts(xml: &str, local_name: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut cursor = 0;
    while cursor < xml.len() {
        let Some(start_rel) = xml[cursor..].find('<') else { break };
        let start = cursor + start_rel;
        let Some(end_rel) = xml[start + 1..].find('>') else { break };
        let end = start + 1 + end_rel;
        let tag = &xml[start + 1..end];
        let trimmed = tag.trim_start();
        let name = trimmed
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("");
        if !trimmed.starts_with('/') && name.rsplit(':').next() == Some(local_name) {
            let close = format!("</{name}>");
            if let Some(close_rel) = xml[end + 1..].find(&close) {
                let text = html_to_text(&xml[end + 1..end + 1 + close_rel]);
                if !text.trim().is_empty() {
                    results.push(text.trim().to_string());
                }
                cursor = end + 1 + close_rel + close.len();
                continue;
            }
        }
        cursor = end + 1;
    }
    results
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        attribute, first_open_tag, html_to_text, open_epub, open_epub_on_worker,
        read_epub_title_on_worker, EPUB_PARSER_WORKER_STACK_BYTES, EPUB_TITLE_WORKER_STACK_BYTES,
    };

    fn temp_epub(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rustmix-{name}-{nonce}.epu"))
    }

    fn push_u16(output: &mut Vec<u8>, value: u16) {
        output.extend(value.to_le_bytes());
    }
    fn push_u32(output: &mut Vec<u8>, value: u32) {
        output.extend(value.to_le_bytes());
    }

    fn stored_zip(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut output = Vec::new();
        let mut central = Vec::new();
        for (name, body) in entries {
            let offset = output.len() as u32;
            push_u32(&mut output, 0x0403_4B50);
            push_u16(&mut output, 20);
            push_u16(&mut output, 0);
            push_u16(&mut output, 0);
            push_u16(&mut output, 0);
            push_u16(&mut output, 0);
            push_u32(&mut output, 0);
            push_u32(&mut output, body.len() as u32);
            push_u32(&mut output, body.len() as u32);
            push_u16(&mut output, name.len() as u16);
            push_u16(&mut output, 0);
            output.extend(name.as_bytes());
            output.extend(body.as_bytes());

            push_u32(&mut central, 0x0201_4B50);
            push_u16(&mut central, 20);
            push_u16(&mut central, 20);
            push_u16(&mut central, 0);
            push_u16(&mut central, 0);
            push_u16(&mut central, 0);
            push_u16(&mut central, 0);
            push_u32(&mut central, 0);
            push_u32(&mut central, body.len() as u32);
            push_u32(&mut central, body.len() as u32);
            push_u16(&mut central, name.len() as u16);
            push_u16(&mut central, 0);
            push_u16(&mut central, 0);
            push_u16(&mut central, 0);
            push_u16(&mut central, 0);
            push_u32(&mut central, 0);
            push_u32(&mut central, offset);
            central.extend(name.as_bytes());
        }
        let central_offset = output.len() as u32;
        let central_size = central.len() as u32;
        output.extend(central);
        push_u32(&mut output, 0x0605_4B50);
        push_u16(&mut output, 0);
        push_u16(&mut output, 0);
        push_u16(&mut output, entries.len() as u16);
        push_u16(&mut output, entries.len() as u16);
        push_u32(&mut output, central_size);
        push_u32(&mut output, central_offset);
        push_u16(&mut output, 0);
        output
    }

    #[test]
    fn xml_attribute_tokenizer_reads_attributes_after_element_name() {
        let tag =
            "rootfile full-path='OEBPS/book.opf' media-type=\"application/oebps-package+xml\"/";
        assert_eq!(
            attribute(tag, "full-path").as_deref(),
            Some("OEBPS/book.opf")
        );
        assert_eq!(
            attribute(tag, "media-type").as_deref(),
            Some("application/oebps-package+xml")
        );
    }

    #[test]
    fn container_rootfile_lookup_ignores_plural_wrapper() {
        let container =
            "<container><rootfiles><rootfile full-path='OEBPS/book.opf'/></rootfiles></container>";
        let rootfile =
            first_open_tag(container, "rootfile").and_then(|tag| attribute(tag, "full-path"));
        assert_eq!(rootfile.as_deref(), Some("OEBPS/book.opf"));
    }

    #[test]
    fn parser_worker_stack_budget_is_explicit() {
        assert_eq!(EPUB_PARSER_WORKER_STACK_BYTES, 64 * 1024);
    }

    #[test]
    fn title_worker_uses_a_smaller_bounded_stack() {
        assert_eq!(EPUB_TITLE_WORKER_STACK_BYTES, 32 * 1024);
        assert!(EPUB_TITLE_WORKER_STACK_BYTES < EPUB_PARSER_WORKER_STACK_BYTES);
    }

    #[test]
    fn flattens_xhtml_into_reflowable_paragraphs() {
        assert_eq!(
            html_to_text("<h1>Title</h1><p>Hello &amp; goodbye.</p>"),
            "Title\n\nHello & goodbye."
        );
    }

    #[test]
    fn opens_stored_epub_manifest_spine_and_nav_toc() {
        let path = temp_epub("stored");
        let bytes = stored_zip(&[
            ("META-INF/container.xml", "<container><rootfiles><rootfile full-path='OEBPS/book.opf'/></rootfiles></container>"),
            ("OEBPS/book.opf", "<package><metadata><dc:title>Sample EPUB</dc:title></metadata><manifest><item id='nav' href='nav.xhtml' media-type='application/xhtml+xml' properties='nav'/><item id='c1' href='c1.xhtml' media-type='application/xhtml+xml'/><item id='c2' href='c2.xhtml' media-type='application/xhtml+xml'/></manifest><spine><itemref idref='c1'/><itemref idref='c2'/></spine></package>"),
            ("OEBPS/nav.xhtml", "<nav><ol><li><a href='c1.xhtml'>Start</a></li><li><a href='c2.xhtml'>Second</a></li></ol></nav>"),
            ("OEBPS/c1.xhtml", "<html><body><h1>Start</h1><p>First chapter.</p></body></html>"),
            ("OEBPS/c2.xhtml", "<html><body><h1>Second</h1><p>Second chapter.</p></body></html>"),
        ]);
        fs::write(&path, bytes).unwrap();
        let epub = open_epub(&path).unwrap();
        let worker_epub = open_epub_on_worker(&path).unwrap();
        assert_eq!(worker_epub, epub);
        assert_eq!(epub.title, "Sample EPUB");
        assert_eq!(epub.spine_count, 2);
        assert_eq!(epub.chapters.len(), 2);
        assert_eq!(epub.chapters[0].number, 1);
        assert_eq!(epub.chapters[1].number, 2);
        assert_eq!(
            epub.chapter_for_offset(epub.chapters[1].text_offset)
                .unwrap()
                .number,
            2
        );
        assert_eq!(read_epub_title_on_worker(&path).unwrap(), "Sample EPUB");
        assert!(epub.text.contains("First chapter."));
        assert!(epub.text.contains("Second chapter."));
        assert_eq!(epub.toc.len(), 2);
        assert_eq!(epub.toc[0].label, "Start");
        assert!(epub.toc[1].text_offset > epub.toc[0].text_offset);
        let _ = fs::remove_file(path);
    }
}
