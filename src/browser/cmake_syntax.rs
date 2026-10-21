//! A bounded static reader, never an interpreter of project CMake code.

pub(super) fn commands(text: &str) -> Vec<(String, Vec<String>)> {
    let tokens = tokens(text);
    let mut result = Vec::new();
    let mut index = 0;
    while index + 1 < tokens.len() {
        if tokens[index + 1] != "\0(" {
            index += 1;
            continue;
        }
        let name = tokens[index].to_ascii_lowercase();
        index += 2;
        let mut depth = 1;
        let mut args = Vec::new();
        while index < tokens.len() && depth > 0 {
            match tokens[index].as_str() {
                "\0(" => depth += 1,
                "\0)" => depth -= 1,
                _ => {}
            }
            if depth > 0 {
                args.push(tokens[index].clone());
            }
            index += 1;
        }
        if depth == 0 {
            result.push((name, args));
        }
    }
    result
}

fn bracket(text: &str) -> Option<(usize, usize)> {
    let rest = text.strip_prefix('[')?;
    let equals = rest.bytes().take_while(|&c| c == b'=').count();
    if rest.as_bytes().get(equals) != Some(&b'[') {
        return None;
    }
    let open = equals + 2;
    let close = format!("]{}]", "=".repeat(equals));
    let end = text[open..].find(&close)? + open;
    Some((open, end))
}

fn tokens(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut index = 0;
    let bytes = text.as_bytes();
    while index < bytes.len() {
        let start = index;
        match bytes[index] {
            b' ' | b'\t' | b'\r' | b'\n' | b';' => index += 1,
            b'#' => {
                index += 1;
                if let Some((open, end)) = bracket(&text[index..]) {
                    index += end + open;
                } else {
                    while index < bytes.len() && bytes[index] != b'\n' {
                        index += 1;
                    }
                }
            }
            b'(' | b')' => {
                result.push(format!("\0{}", &text[index..index + 1]));
                index += 1;
            }
            b'"' => {
                index += 1;
                let mut value = String::new();
                while index < bytes.len() && bytes[index] != b'"' {
                    if bytes[index] == b'\\' && index + 1 < bytes.len() {
                        index += 1;
                    }
                    let ch = text[index..].chars().next().unwrap();
                    value.push(ch);
                    index += ch.len_utf8();
                }
                index = (index + 1).min(bytes.len());
                result.push(value);
            }
            b'[' if bracket(&text[index..]).is_some() => {
                let (open, end) = bracket(&text[index..]).unwrap();
                result.push(text[index + open..index + end].into());
                index += end + open;
            }
            _ => {
                while index < bytes.len() && !b" \t\r\n();#".contains(&bytes[index]) {
                    index += 1;
                }
                result.push(text[start..index].into());
            }
        }
    }
    result
}

pub(super) fn expand(
    value: &str,
    variables: &std::collections::BTreeMap<String, String>,
) -> String {
    let mut value = value.to_owned();
    for _ in 0..16 {
        let before = value.clone();
        for (name, replacement) in variables {
            value = value.replace(&format!("${{{name}}}"), replacement);
        }
        if before == value || value.len() > 16_384 {
            break;
        }
    }
    value
}
