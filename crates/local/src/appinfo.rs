//! A small, tolerant reader for Steam's binary app cache (`appcache/appinfo.vdf`).
//!
//! The cache holds what the Steam client knows about every app it has seen. Only `common.name` and
//! `common.type` of the apps asked for are kept: other apps are skipped without being parsed, and the other
//! fields of a wanted app are walked over without being kept. Versions 28 and 29 are understood (29 moves key
//! names to a string table at the end of the file); other versions are an error, so callers do without the
//! cache.

use crate::LocalError;
use std::{
    collections::{BTreeSet, HashMap},
    fs::File,
    io::{BufReader, ErrorKind, Read, Seek, SeekFrom},
    path::Path,
};

const MAGIC_V28: u32 = 0x0756_4428;
const MAGIC_V29: u32 = 0x0756_4429;
/// Bytes between an entry's size and its key-values: state, update time, access token, text hash, change
/// number and binary hash.
const ENTRY_HEADER: usize = 4 + 4 + 8 + 20 + 4 + 20;
/// Larger entries and string tables are treated as damage rather than loaded.
const MAX_BLOCK: u64 = 64 << 20;
const MAX_DEPTH: usize = 64;

// Binary KeyValues tags.
const MAP: u8 = 0x00;
const STRING: u8 = 0x01;
const INT32: u8 = 0x02;
const FLOAT32: u8 = 0x03;
const POINTER: u8 = 0x04;
const WIDE_STRING: u8 = 0x05;
const COLOR: u8 = 0x06;
const UINT64: u8 = 0x07;
const END: u8 = 0x08;
const INT64: u8 = 0x0a;
const END_ALT: u8 = 0x0b;

/// What the cache says about one app.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct App {
    /// `common.name`.
    pub name: Option<String>,
    /// `common.type` in lower case: `game`, `dlc`, `application`, `tool`, `demo`, `music`, ...
    pub app_type: Option<String>,
}

/// Reads the apps in `wanted` from an `appinfo.vdf` file. A file cut short yields the apps read before the cut;
/// an unknown version or a damaged header is an error.
pub fn read(path: &Path, wanted: &BTreeSet<u64>) -> Result<HashMap<u64, App>, LocalError> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut file = BufReader::with_capacity(1 << 16, file);
    let magic = u32_le(&mut file)?;
    let _universe = u32_le(&mut file)?;
    let table = match magic {
        MAGIC_V28 => None,
        MAGIC_V29 => {
            let mut offset = [0; 8];
            file.read_exact(&mut offset)?;
            let start = file.stream_position()?;
            let table = string_table(&mut file, i64::from_le_bytes(offset), len)?;
            file.seek(SeekFrom::Start(start))?;
            Some(table)
        }
        other => return Err(bad(format!("unsupported app cache version {other:#010x}"))),
    };

    let keys = match &table {
        Some(t) => Keys::Table(t),
        None => Keys::Inline,
    };
    let mut apps = HashMap::new();
    let mut complete = false;
    while apps.len() < wanted.len() {
        let Ok(app_id) = u32_le(&mut file) else { break };
        if app_id == 0 {
            complete = true;
            break;
        }
        let Ok(size) = u32_le(&mut file) else { break };
        if !wanted.contains(&u64::from(app_id)) || u64::from(size) > MAX_BLOCK {
            if file.seek_relative(i64::from(size)).is_err() {
                break;
            }
            continue;
        }
        let mut entry = vec![0; size as usize];
        if file.read_exact(&mut entry).is_err() {
            break;
        }
        match entry.get(ENTRY_HEADER..).and_then(|kv| common(kv, keys)) {
            Some(app) => {
                apps.insert(u64::from(app_id), app);
            }
            None => tracing::debug!(app_id, "skipping an unreadable app in the Steam app cache"),
        }
    }
    if !complete && apps.len() < wanted.len() {
        tracing::warn!(file = %path.display(), "the Steam app cache ends early; keeping the apps read so far");
    }
    Ok(apps)
}

fn bad(message: impl Into<String>) -> LocalError {
    LocalError::Format(message.into())
}

fn u32_le(r: &mut impl Read) -> Result<u32, LocalError> {
    let mut b = [0; 4];
    r.read_exact(&mut b).map_err(|e| match e.kind() {
        ErrorKind::UnexpectedEof => bad("the app cache ends early"),
        _ => e.into(),
    })?;
    Ok(u32::from_le_bytes(b))
}

/// Key names of version 29: a count, then that many NUL-terminated strings.
fn string_table(file: &mut BufReader<File>, offset: i64, len: u64) -> Result<Vec<Vec<u8>>, LocalError> {
    let offset = u64::try_from(offset).map_err(|_| bad("negative string table offset"))?;
    if offset >= len {
        return Err(bad("the app cache ends before its string table"));
    }
    if len - offset > MAX_BLOCK {
        return Err(bad("string table too large"));
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut rest = Vec::new();
    file.read_to_end(&mut rest)?;
    let mut cur = Cursor { data: &rest, pos: 0 };
    let count = cur
        .u32()
        .ok_or_else(|| bad("the app cache ends before its string table"))?;
    let mut table = Vec::new();
    for _ in 0..count {
        let s = cur
            .cstr()
            .ok_or_else(|| bad("the app cache string table is cut short"))?;
        table.push(s.to_vec());
    }
    Ok(table)
}

#[derive(Clone, Copy)]
enum Keys<'a> {
    /// Version 28: each key is a NUL-terminated string.
    Inline,
    /// Version 29: each key is an index into the string table.
    Table(&'a [Vec<u8>]),
}

/// `common.name` and `common.type` of one entry's key-values, or `None` when neither could be read.
fn common(kv: &[u8], keys: Keys<'_>) -> Option<App> {
    let mut walker = Walker {
        cur: Cursor { data: kv, pos: 0 },
        keys,
        app: App::default(),
    };
    // A damaged or unknown field after the `common` section does not lose what was read before it.
    let _ = walker.map(Section::Root, 0);
    (walker.app != App::default()).then_some(walker.app)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Root,
    AppInfo,
    Common,
    Other,
}

struct Walker<'a> {
    cur: Cursor<'a>,
    keys: Keys<'a>,
    app: App,
}

impl<'a> Walker<'a> {
    fn key(&mut self) -> Option<&'a [u8]> {
        match self.keys {
            Keys::Inline => self.cur.cstr(),
            Keys::Table(t) => t.get(usize::try_from(self.cur.u32()?).ok()?).map(Vec::as_slice),
        }
    }

    /// Walks one map up to its end marker, keeping `name` and `type` when it is the `common` section.
    fn map(&mut self, section: Section, depth: usize) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        loop {
            let tag = self.cur.byte()?;
            if tag == END || tag == END_ALT {
                return Some(());
            }
            let key = self.key()?;
            match tag {
                MAP => {
                    let inner = match section {
                        Section::Root if key.eq_ignore_ascii_case(b"appinfo") => Section::AppInfo,
                        Section::Root | Section::AppInfo if key.eq_ignore_ascii_case(b"common") => {
                            Section::Common
                        }
                        _ => Section::Other,
                    };
                    self.map(inner, depth + 1)?;
                }
                STRING => {
                    let value = self.cur.cstr()?;
                    if section == Section::Common {
                        if key.eq_ignore_ascii_case(b"name") {
                            self.app.name = text(value);
                        } else if key.eq_ignore_ascii_case(b"type") {
                            self.app.app_type = text(value).map(|t| t.to_ascii_lowercase());
                        }
                    }
                }
                INT32 | FLOAT32 | POINTER | COLOR => {
                    self.cur.take(4)?;
                }
                UINT64 | INT64 => {
                    self.cur.take(8)?;
                }
                WIDE_STRING => self.cur.wide()?,
                _ => return None,
            }
        }
    }
}

fn text(bytes: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(bytes);
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let bytes = self.data.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(bytes)
    }

    fn byte(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4)?.try_into().ok().map(u32::from_le_bytes)
    }

    /// A NUL-terminated string, without its terminator.
    fn cstr(&mut self) -> Option<&'a [u8]> {
        let rest = self.data.get(self.pos..)?;
        let end = rest.iter().position(|b| *b == 0)?;
        self.pos += end + 1;
        Some(&rest[..end])
    }

    /// Skips a NUL-terminated UTF-16 string.
    fn wide(&mut self) -> Option<()> {
        while self.take(2)? != [0, 0] {}
        Some(())
    }
}
