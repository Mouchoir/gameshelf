//! A small, tolerant reader for Valve's text KeyValues format (`.vdf`, `.acf`).
//!
//! Steam is not consistent about key case between versions, so lookups ignore ASCII case.

use crate::LocalError;

const MAX_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Str(String),
    Obj(Obj),
}

/// An object, keeping keys in file order (keys may repeat).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Obj(pub Vec<(String, Value)>);

impl Obj {
    /// First value under `key`, ignoring ASCII case.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    pub fn obj(&self, key: &str) -> Option<&Obj> {
        match self.get(key)? {
            Value::Obj(o) => Some(o),
            Value::Str(_) => None,
        }
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Str(s) => Some(s),
            Value::Obj(_) => None,
        }
    }

    /// Parses a numeric value; empty or malformed values read as `None`.
    pub fn num(&self, key: &str) -> Option<u64> {
        self.str(key)?.trim().parse().ok()
    }

    /// Follows nested objects.
    pub fn path(&self, keys: &[&str]) -> Option<&Obj> {
        keys.iter().try_fold(self, |o, k| o.obj(k))
    }

    /// Nested objects, with their keys.
    pub fn objects(&self) -> impl Iterator<Item = (&str, &Obj)> {
        self.0.iter().filter_map(|(k, v)| match v {
            Value::Obj(o) => Some((k.as_str(), o)),
            Value::Str(_) => None,
        })
    }
}

/// Parses a whole file. The result is the top-level object (usually holding one root key).
pub fn parse(text: &str) -> Result<Obj, LocalError> {
    let mut p = Parser {
        chars: text.trim_start_matches('\u{feff}').chars().collect(),
        pos: 0,
    };
    let obj = p.object(0)?;
    match p.token()? {
        None => Ok(obj),
        Some(Token::Close) => Err(LocalError::Format("unbalanced closing brace".into())),
        Some(_) => Err(LocalError::Format("unexpected data after the end".into())),
    }
}

enum Token {
    Str(String),
    Open,
    Close,
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn object(&mut self, depth: usize) -> Result<Obj, LocalError> {
        if depth > MAX_DEPTH {
            return Err(LocalError::Format("nesting too deep".into()));
        }
        let mut obj = Obj::default();
        loop {
            let key = match self.token()? {
                None => return Ok(obj),
                Some(Token::Close) => {
                    self.pos -= 1;
                    return Ok(obj);
                }
                Some(Token::Open) => return Err(LocalError::Format("missing key before '{'".into())),
                Some(Token::Str(k)) => k,
            };
            let value = match self.token()? {
                Some(Token::Str(v)) => Value::Str(v),
                Some(Token::Open) => {
                    let inner = self.object(depth + 1)?;
                    match self.token()? {
                        Some(Token::Close) => Value::Obj(inner),
                        _ => return Err(LocalError::Format("missing closing brace".into())),
                    }
                }
                _ => return Err(LocalError::Format(format!("missing value for key {key:?}"))),
            };
            obj.0.push((key, value));
        }
    }

    /// Next token, skipping whitespace, `//` comments and platform conditionals such as `[$WIN32]`.
    fn token(&mut self) -> Result<Option<Token>, LocalError> {
        loop {
            while self.pos < self.chars.len() && self.chars[self.pos].is_whitespace() {
                self.pos += 1;
            }
            let Some(&c) = self.chars.get(self.pos) else {
                return Ok(None);
            };
            if c == '/' && self.chars.get(self.pos + 1) == Some(&'/') {
                while self.pos < self.chars.len() && self.chars[self.pos] != '\n' {
                    self.pos += 1;
                }
                continue;
            }
            if c == '[' {
                while self.pos < self.chars.len() && self.chars[self.pos] != ']' {
                    self.pos += 1;
                }
                self.pos += 1;
                continue;
            }
            self.pos += 1;
            return match c {
                '{' => Ok(Some(Token::Open)),
                '}' => Ok(Some(Token::Close)),
                '"' => self.quoted().map(|s| Some(Token::Str(s))),
                _ => {
                    let start = self.pos - 1;
                    while self
                        .chars
                        .get(self.pos)
                        .is_some_and(|c| !c.is_whitespace() && !matches!(c, '{' | '}' | '"'))
                    {
                        self.pos += 1;
                    }
                    Ok(Some(Token::Str(self.chars[start..self.pos].iter().collect())))
                }
            };
        }
    }

    fn quoted(&mut self) -> Result<String, LocalError> {
        let mut s = String::new();
        loop {
            let Some(&c) = self.chars.get(self.pos) else {
                return Err(LocalError::Format("unterminated string".into()));
            };
            self.pos += 1;
            match c {
                '"' => return Ok(s),
                '\\' => {
                    let next = self.chars.get(self.pos).copied();
                    self.pos += 1;
                    match next {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some(other) => s.push(other),
                        None => return Err(LocalError::Format("unterminated string".into())),
                    }
                }
                _ => s.push(c),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_objects_escapes_and_comments() {
        let text = "// header\n\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\Games\\\\Steam\"\n\t\t\"apps\" { \"730\" \"123\" }\n\t}\n}\n";
        let root = parse(text).expect("parse");
        let lib = root.path(&["LibraryFolders", "0"]).expect("lib");
        assert_eq!(lib.str("PATH"), Some("D:\\Games\\Steam"));
        assert_eq!(lib.obj("apps").and_then(|a| a.num("730")), Some(123));
    }

    #[test]
    fn conditionals_and_unquoted_tokens_are_tolerated() {
        let root = parse("root { key value [$WIN32] other \"x\" }").expect("parse");
        let o = root.obj("root").expect("root");
        assert_eq!(o.str("key"), Some("value"));
        assert_eq!(o.str("other"), Some("x"));
    }

    #[test]
    fn malformed_input_is_an_error_not_a_panic() {
        for bad in ["\"a\" {", "\"a\"", "}", "\"a\" \"b", "{ \"a\" \"b\" }"] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        let deep = "\"a\" {".repeat(200);
        assert!(parse(&deep).is_err());
    }
}
