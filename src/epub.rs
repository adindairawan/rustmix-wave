// rustmix-wave=v0.17.0-parser-doc-repair-v2
// rustmix-wave=epub-xml-attribute-tokenizer-repair-ready
// rustmix-wave=epub-parser-stack-isolation-ready
// rustmix-wave=epub-chapter-aware-presentation-ready
// rustmix-wave=epub-watchdog-memory-pressure-repair-ready
//! Bounded reflowable EPUB reader foundation[cite: 15].

use std::{collections::BTreeMap, fs, path::Path};

use miniz_oxide::inflate::decompress_to_vec;

/// Maximum EPUB archive bytes accepted from removable storage[cite: 15].
pub const EPUB_ARCHIVE_BYTES_LIMIT: usize = 16 * 1024 * 1024;
/// Maximum central-directory records accepted from one EPUB[cite: 15].
pub const EPUB_ARCHIVE_ENTRY_LIMIT: usize = 512;
/// Maximum compressed bytes extracted for one EPUB member[cite: 15].
pub const EPUB_MEMBER_COMPRESSED_LIMIT: usize = 2 * 1024 * 1024;
/// Maximum decompressed bytes extracted for one EPUB member[cite: 15].
pub const EPUB_MEMBER_UNCOMPRESSED_LIMIT: usize = 4 * 1024 * 1024;
/// Maximum flattened reflowable text retained in RAM for one EPUB[cite: 15].
pub const EPUB_REFLOW_TEXT_LIMIT: usize = 2 * 1024 * 1024;
/// Maximum manifest records retained from one OPF package[cite: 15].
pub const EPUB_MANIFEST_LIMIT: usize = 256;
/// Maximum spine records retained from one OPF package[cite: 15].
pub const EPUB_SPINE_LIMIT: usize = 128;
/// Maximum TOC records rendered by the Reader UI[cite: 15].
pub const EPUB_TOC_LIMIT: usize = 128;
/// Dedicated parser-worker stack budget[cite: 15].
pub const EPUB_PARSER_WORKER_STACK_BYTES: usize = 64 * 1024;
/// Lightweight OPF-title worker stack budget[cite: 15].
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

/// One reflowable EPUB TOC destination[cite: 15].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpubTocEntry {
    pub label: String,
    pub text_offset: u64,
    pub spine_index: usize,
}

/// One readable spine chapter retained alongside flattened EPUB text[cite: 15].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpubChapter {
    pub number: usize,
    pub label: String,
    pub text_offset: u64,
    pub text_end_offset: u64,
    pub spine_index: usize,
}

/// One bounded, reflowable EPUB book retained while the Reader session is open[cite: 15].
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct ManifestItem {
    id: String,
    href: String,
    media_type: String,
    properties: String,
}

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
        .spawn(move || {
            let archive = ZipArchive::open(path)?;
            let (_, package, package_dir) = epub_package(&archive)?;
            let title = package_title(&package);
            let mut preface_text = String::new();
            if let (Ok(manifest), Ok(spine_ids)) = (parse_manifest(&package), parse_spine_ids(&package)) {
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
fn is_ao3_preface_spine(member: &str, chapter_text: &str, total_spine_items: usize) -> bool {
    if total_spine_items <= 1 {
        return false;
    }
    let lower_member = member.to_ascii_lowercase();
    if lower_member.ends_with("split_000.xhtml") || lower_member.ends_with("split_000.html") {
        return true;
    }
    (chapter_text.contains("Archive Warning:") || chapter_text.contains("Rating:"))
        && (chapter_text.contains("Fandom:") || chapter_text.contains("Fandoms:"))
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

        // If spine item 0 is AO3's preface page (split_000.xhtml), extract metadata
        // from it and skip adding it to the readable chapter buffer so Chapter 1 starts on spine item 1.
        if spine_index == 0 && is_ao3_preface_spine(&member, &chapter, spine_ids.len()) {
            preface_text = chapter;
            continue;
        }

        if !text.is_empty() {
            text.push_str("\n\n");
        }
        let offset = text.len() as u64;
        let label = fallback_chapter_label(&xhtml, chapters.len());
        chapter_offsets.insert(member.clone(), (spine_index, offset));
        chapter_labels.push((spine_index, offset, label.clone()));
        text.push_str(chapter.trim());
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

fn parse_ao3_opf_metadata(
    package: &str,
    preface_source: &str,
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
    if let Some(desc) = first_element_text(package, "description").filter(|s| !s.trim().is_empty()) {
        meta.summary = desc.replace('\n', " ").trim().to_string();
    }

    // Parse OPF <dc:subject> tags for Rating, Fandom, and Character fallbacks
    let subjects = all_element_texts(package, "subject");
    let mut fandoms = Vec::new();
    let mut chars = Vec::new();

    for subj in subjects {
        let lower = subj.to_ascii_lowercase();
        if lower.contains("explicit")
            || lower.contains("mature")
            || lower.contains("teen and up")
            || lower.contains("general audiences")
            || lower.contains("not rated")
        {
            meta.rating_text = subj;
        } else if lower != "fanfiction"
            && !lower.starts_with("ao3")
            && !lower.contains("archive warning")
            && !matches!(subj.as_str(), "Gen" | "F/F" | "F/M" | "M/M" | "Multi" | "Other")
            && !subj.contains('/')
            && !subj.contains(" & ")
        {
            if fandoms.is_empty() {
                fandoms.push(subj);
            } else if chars.len() < 6 {
                chars.push(subj);
            }
        }
    }
    if !fandoms.is_empty() {
        meta.fandoms = fandoms.join(", ");
    }
    if !chars.is_empty() {
        meta.characters = chars.join(", ");
    }

    // Scan AO3's preface text (split_000.xhtml) for exact Rating, Fandom, Character, Summary, and Notes
    let preface: String = preface_source.chars().take(4000).collect();
    let lines: Vec<&str> = preface.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    for (i, line) in lines.iter().enumerate() {
        let next = lines.get(i + 1).copied().unwrap_or("");
        if let Some(v) = line.strip_prefix("Rating:") {
            meta.rating_text = if v.trim().is_empty() { next.into() } else { v.trim().into() };
        } else if let Some(v) = line.strip_prefix("Fandom:").or_else(|| line.strip_prefix("Fandoms:")) {
            meta.fandoms = if v.trim().is_empty() { next.into() } else { v.trim().into() };
        } else if let Some(v) = line.strip_prefix("Character:").or_else(|| line.strip_prefix("Characters:")) {
            meta.characters = if v.trim().is_empty() { next.into() } else { v.trim().into() };
        } else if *line == "Summary" || *line == "Summary:" {
            if !next.is_empty() {
                meta.summary = next.into();
            }
        } else if *line == "Notes" || *line == "Notes:" {
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
    }
    meta
}

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
