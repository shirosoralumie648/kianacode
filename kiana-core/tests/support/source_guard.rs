//! Lexical views for source guards. Comments and test-only items must not supply evidence or
//! look like production bypasses; string literals in real code remain visible to deny checks.
#![allow(dead_code)]

fn blank(bytes: &mut [u8], start: usize, end: usize) {
    for byte in &mut bytes[start..end] {
        if !matches!(*byte, b'\n' | b'\r') {
            *byte = b' ';
        }
    }
}

fn lexical_view(source: &str, mask_literals: bool) -> String {
    let input = source.as_bytes();
    let mut output = input.to_vec();
    let mut i = 0;
    while i < input.len() {
        let start = i;
        if input[i..].starts_with(b"//") {
            i += 2;
            while i < input.len() && input[i] != b'\n' {
                i += 1;
            }
            blank(&mut output, start, i);
        } else if input[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while i < input.len() && depth > 0 {
                if input[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if input[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            assert_eq!(depth, 0, "unterminated Rust block comment");
            blank(&mut output, start, i);
        } else {
            let mut raw_quote = i;
            if input[i] == b'r' {
                raw_quote += 1;
                while raw_quote < input.len() && input[raw_quote] == b'#' {
                    raw_quote += 1;
                }
            }
            if input[i] == b'r' && input.get(raw_quote) == Some(&b'"') {
                let hashes = raw_quote - i - 1;
                i = raw_quote + 1;
                let mut closed = false;
                while i < input.len() {
                    if input[i] == b'"'
                        && input.get(i + 1..i + 1 + hashes) == Some(&input[start + 1..raw_quote])
                    {
                        i += 1 + hashes;
                        closed = true;
                        break;
                    }
                    i += 1;
                }
                if !closed {
                    // Some guarded files are not Rust (for example HTML embedding JS regex
                    // literals); a stray quote must degrade to an ordinary byte, not panic.
                    i = start + 1;
                } else if mask_literals {
                    blank(&mut output, start, i);
                }
            } else if input[i] == b'"' {
                i += 1;
                let mut closed = false;
                while i < input.len() {
                    if input[i] == b'\\' {
                        i += 2;
                    } else if input[i] == b'"' {
                        i += 1;
                        closed = true;
                        break;
                    } else {
                        i += 1;
                    }
                }
                if !closed {
                    i = start + 1;
                } else if mask_literals {
                    blank(&mut output, start, i);
                }
            } else if input[i] == b'\'' {
                // A lifetime has no immediately closing quote after one scalar/escape.
                let next = i + 1;
                let end = if input.get(next) == Some(&b'\\') {
                    input[next + 2..]
                        .iter()
                        .take(12)
                        .position(|byte| *byte == b'\'')
                        .map(|offset| next + 2 + offset)
                } else {
                    source.get(next..).and_then(|tail| {
                        let scalar = tail.chars().next()?;
                        let quote = next + scalar.len_utf8();
                        (input.get(quote) == Some(&b'\'')).then_some(quote)
                    })
                };
                if let Some(end) = end {
                    i = end + 1;
                    if mask_literals {
                        blank(&mut output, start, i);
                    }
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
    }
    String::from_utf8(output).expect("lexical view preserves UTF-8 outside masked spans")
}

fn block_end(mask: &str, open: usize) -> usize {
    let mut depth = 0;
    for (offset, byte) in mask.as_bytes()[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return open + offset + 1;
                }
            }
            _ => {}
        }
    }
    panic!("unterminated Rust item body");
}

pub fn production_source(source: &str) -> String {
    let mut output = lexical_view(source, false).into_bytes();
    let mask = lexical_view(source, true);
    let mut offset = 0;
    while let Some(found) = mask[offset..].find("#[cfg(test)]") {
        let start = offset + found;
        let item = start + "#[cfg(test)]".len();
        let delimiter = mask[item..]
            .find(['{', ';'])
            .map(|position| item + position)
            .expect("test-only item must have an item boundary");
        let end = if mask.as_bytes()[delimiter] == b'{' {
            block_end(&mask, delimiter)
        } else {
            delimiter + 1
        };
        blank(&mut output, start, end);
        offset = end;
    }
    String::from_utf8(output).expect("production view preserves UTF-8")
}

pub fn item_source(source: &str, signature: &str) -> String {
    let production = production_source(source);
    let mask = lexical_view(&production, true);
    let start = mask
        .find(signature)
        .unwrap_or_else(|| panic!("missing item {signature}"));
    let open = start + mask[start..].find('{').expect("item must have a body");
    production[start..block_end(&mask, open)].to_owned()
}

pub fn compact_code(source: &str) -> String {
    production_source(source).split_whitespace().collect()
}

/// True when `needle` appears as a standalone identifier, so detector functions such as
/// `scan_secret_value` or required fields such as `company_acceptance_required` cannot be
/// mistaken for the forbidden identifiers `secret_value` or `company_accept`.
pub fn contains_identifier(source: &str, needle: &str) -> bool {
    let is_identifier = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let bytes = source.as_bytes();
    let mut offset = 0;
    while let Some(found) = source[offset..].find(needle) {
        let start = offset + found;
        let end = start + needle.len();
        let boundary_before = start == 0 || !is_identifier(bytes[start - 1]);
        let boundary_after = end == bytes.len() || !is_identifier(bytes[end]);
        if boundary_before && boundary_after {
            return true;
        }
        offset = end;
    }
    false
}

#[test]
fn source_view_keeps_code_literals_and_removes_nested_comments_and_test_only_items() {
    let sample = r###"
// --share-net is denied, not a command argument.
/* outer /* nested download */ Runner */
fn owner<'a>(value: &'a str) { let text = r#"https://literal/{--share-net}"#; }
#[cfg(test)]
fn test_helper() { fake_bypass(); }
#[cfg(test)]
mod tests { fn child() { assert!("forbidden_marker"); } }
fn later() { real_guard(); }
"###;
    let production = production_source(sample);
    assert!(production.contains("https://literal/{--share-net}"));
    assert!(production.contains("real_guard()"));
    assert!(!production.contains("download"));
    assert!(!production.contains("Runner"));
    assert!(!production.contains("fake_bypass"));
    assert!(!production.contains("forbidden_marker"));
}

#[test]
fn item_scope_does_not_include_an_adjacent_effect_or_literal_braces() {
    let sample = "fn preview() { let a = \"}\"; let b = '\"'; read_only(); }\nfn commit() { commit_planned(); }";
    let item = item_source(sample, "fn preview(");
    assert!(item.contains("read_only()"));
    assert!(!item.contains("commit_planned"));
    assert!(!item.contains("fn commit"));
}

#[test]
fn identifier_check_sees_standalone_names_not_longer_identifiers() {
    assert!(contains_identifier(
        "pub secret_value: String",
        "secret_value"
    ));
    assert!(!contains_identifier(
        "scan_secret_value(payload)",
        "secret_value"
    ));
    assert!(!contains_identifier(
        "scan_secret_value_with_limit(payload)",
        "secret_value"
    ));
    assert!(contains_identifier("(company_accept)", "company_accept"));
    assert!(!contains_identifier(
        "company_acceptance_required",
        "company_accept"
    ));
}

/// Check the report validator rather than an obsolete prose spelling of `behavior_verified`.
pub fn platform_report_has_evidence_fence(source: &str) -> bool {
    let validate = item_source(source, "pub fn validate(");
    let code = compact_code(&validate);
    code.contains("ifself.disposition==PlatformBackendDisposition::Implemented&&!self.behavior_verified")
        && code.contains("platform_backend_implemented_requires_behavior")
        && code.contains("ifself.disposition!=PlatformBackendDisposition::Implemented&&self.behavior_verified")
        && code.contains("platform_backend_unverified_disposition_mismatch")
        && code.contains("if!self.no_host_fallback")
        && code.contains("platform_backend_host_fallback_forbidden")
        && code.contains("self.disposition!=PlatformBackendDisposition::Implemented&&self.limitations.is_empty()")
        && code.contains("platform_backend_limitation_required")
}
