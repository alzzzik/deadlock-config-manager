//! Valve KeyValues tokenizer and parser (gameinfo.gi, video.txt, *.vdf).
//!
//! Tokens keep their byte offsets so callers can edit the original text in
//! place without reformatting the parts they don't touch.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Str(String),
    Open,
    Close,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub start: usize,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Str(String),
    /// `open` / `close` are byte offsets of the `{` and `}` characters.
    Block { children: Vec<Node>, open: usize, close: usize },
}

#[derive(Debug, Clone)]
pub struct Node {
    pub key: String,
    pub value: Value,
    pub line: usize,
    /// Byte offset where the key token starts (including its opening quote).
    pub key_start: usize,
}

impl Node {
    pub fn children(&self) -> &[Node] {
        match &self.value {
            Value::Block { children, .. } => children,
            Value::Str(_) => &[],
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match &self.value {
            Value::Str(s) => Some(s),
            Value::Block { .. } => None,
        }
    }

    pub fn is_block(&self) -> bool {
        matches!(self.value, Value::Block { .. })
    }
}

/// First node whose key matches case-insensitively.
pub fn get<'a>(nodes: &'a [Node], key: &str) -> Option<&'a Node> {
    nodes.iter().find(|n| n.key.eq_ignore_ascii_case(key))
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, ParseError> {
    let bytes = src.as_bytes();
    let mut tokens = Vec::new();
    let mut i = if src.starts_with('\u{feff}') { 3 } else { 0 };
    let mut line = 1;

    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b' ' | b'\t' | b'\r' => i += 1,
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'{' => {
                tokens.push(Token { tok: Tok::Open, start: i, line });
                i += 1;
            }
            b'}' => {
                tokens.push(Token { tok: Tok::Close, start: i, line });
                i += 1;
            }
            b'[' => {
                // Platform conditional such as [$WIN64]; it only qualifies the
                // previous entry, so it carries no data we need.
                while i < bytes.len() && bytes[i] != b']' && bytes[i] != b'\n' {
                    i += 1;
                }
                i += 1;
            }
            b'"' => {
                let (text, next, lines) = read_quoted(src, i, line)?;
                tokens.push(Token { tok: Tok::Str(text), start: i, line });
                line += lines;
                i = next;
            }
            _ => {
                let start = i;
                while i < bytes.len() && !ends_unquoted(bytes, i) {
                    i += 1;
                }
                tokens.push(Token { tok: Tok::Str(src[start..i].to_string()), start, line });
            }
        }
    }
    Ok(tokens)
}

fn ends_unquoted(bytes: &[u8], i: usize) -> bool {
    matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n' | b'{' | b'}' | b'"')
        || (bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/'))
}

/// Reads a quoted string starting at `start` (the opening quote).
/// Returns the unescaped text, the index after the closing quote and the number of newlines crossed.
fn read_quoted(src: &str, start: usize, line: usize) -> Result<(String, usize, usize), ParseError> {
    let bytes = src.as_bytes();
    let mut out = String::new();
    let mut i = start + 1;
    let mut seg = i;
    let mut newlines = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                out.push_str(&src[seg..i]);
                return Ok((out, i + 1, newlines));
            }
            b'\\' if i + 1 < bytes.len() => {
                let replacement = match bytes[i + 1] {
                    b'\\' => Some('\\'),
                    b'"' => Some('"'),
                    b'n' => Some('\n'),
                    b't' => Some('\t'),
                    _ => None,
                };
                if let Some(r) = replacement {
                    out.push_str(&src[seg..i]);
                    out.push(r);
                    i += 2;
                    seg = i;
                } else {
                    i += 1;
                }
            }
            b'\n' => {
                newlines += 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    Err(ParseError { line, message: "unterminated quoted string".into() })
}

/// Parses a whole document into its top-level nodes.
pub fn parse(src: &str) -> Result<Vec<Node>, ParseError> {
    let tokens = tokenize(src)?;
    let mut pos = 0;
    let nodes = parse_list(&tokens, &mut pos, None)?;
    Ok(nodes)
}

/// Parses and requires a non-empty root block (what every config file we touch looks like).
pub fn parse_document(src: &str) -> Result<Vec<Node>, ParseError> {
    let nodes = parse(src)?;
    match nodes.first() {
        Some(root) if root.is_block() && !root.children().is_empty() => Ok(nodes),
        Some(root) => Err(ParseError { line: root.line, message: "root block is empty or missing".into() }),
        None => Err(ParseError { line: 1, message: "file is empty".into() }),
    }
}

fn parse_list(tokens: &[Token], pos: &mut usize, opened_at: Option<usize>) -> Result<Vec<Node>, ParseError> {
    let mut nodes = Vec::new();
    loop {
        let Some(t) = tokens.get(*pos) else {
            return match opened_at {
                Some(l) => Err(ParseError { line: l, message: "block opened here is missing its closing }".into() }),
                None => Ok(nodes),
            };
        };
        let key = match &t.tok {
            Tok::Close => {
                return match opened_at {
                    Some(_) => Ok(nodes),
                    None => Err(ParseError { line: t.line, message: "unexpected }".into() }),
                };
            }
            Tok::Open => return Err(ParseError { line: t.line, message: "{ without a key".into() }),
            Tok::Str(s) => s.clone(),
        };
        let (key_start, line) = (t.start, t.line);
        *pos += 1;
        let value = match tokens.get(*pos) {
            Some(Token { tok: Tok::Str(v), .. }) => {
                *pos += 1;
                Value::Str(v.clone())
            }
            Some(Token { tok: Tok::Open, start: open, line: open_line }) => {
                let open = *open;
                *pos += 1;
                let children = parse_list(tokens, pos, Some(*open_line))?;
                let close = tokens[*pos].start;
                *pos += 1;
                Value::Block { children, open, close }
            }
            _ => return Err(ParseError { line, message: format!("key \"{key}\" has no value") }),
        };
        nodes.push(Node { key, value, line, key_start });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_comments_quotes_unquoted_nested_and_crlf() {
        let src = "// header\r\n\"root\"\r\n{\r\n\tkey \"quoted value\" // trailing\r\n\tpath citadel/addons\r\n\tinner\r\n\t{\r\n\t\t\"a\"\t\t\"1\"\r\n\t}\r\n\tcond \"x\" [$WIN64]\r\n}\r\n";
        let nodes = parse_document(src).unwrap();
        let root = &nodes[0];
        assert_eq!(root.key, "root");
        let kids = root.children();
        assert_eq!(get(kids, "KEY").unwrap().as_str(), Some("quoted value"));
        assert_eq!(get(kids, "path").unwrap().as_str(), Some("citadel/addons"));
        let inner = get(kids, "inner").unwrap();
        assert_eq!(get(inner.children(), "a").unwrap().as_str(), Some("1"));
        assert_eq!(inner.line, 6);
        assert_eq!(get(kids, "cond").unwrap().as_str(), Some("x"));
    }

    #[test]
    fn unescapes_vdf_paths() {
        let nodes = parse("\"p\" \"C:\\\\Program Files (x86)\\\\Steam\"").unwrap();
        assert_eq!(nodes[0].as_str(), Some("C:\\Program Files (x86)\\Steam"));
    }

    #[test]
    fn reports_line_of_unbalanced_braces() {
        let err = parse("a\n{\n b \"1\"\n").unwrap_err();
        assert_eq!(err.line, 2);
        let err = parse("a \"1\"\n}\n").unwrap_err();
        assert_eq!(err.line, 2);
    }

    #[test]
    fn rejects_empty_root() {
        assert!(parse_document("\"video.cfg\"\n{\n}\n").is_err());
        assert!(parse_document("").is_err());
    }
}
