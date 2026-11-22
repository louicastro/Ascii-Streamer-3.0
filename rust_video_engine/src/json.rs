use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(BTreeMap<String, JsonValue>),
}

impl JsonValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            JsonValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        self.as_f64().map(|n| n as i64)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(map) => map.get(key),
            _ => None,
        }
    }

    pub fn parse(input: &str) -> Result<JsonValue, String> {
        let chars: Vec<char> = input.chars().collect();
        let mut idx = 0;
        skip_ws(&chars, &mut idx);
        let val = parse_val(&chars, &mut idx)?;
        Ok(val)
    }

    pub fn to_string(&self) -> String {
        let mut out = String::new();
        self.serialize_into(&mut out);
        out
    }

    fn serialize_into(&self, out: &mut String) {
        match self {
            JsonValue::Null => out.push_str("null"),
            JsonValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            JsonValue::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    out.push_str(&format!("{}", *n as i64));
                } else {
                    out.push_str(&format!("{:.2}", n));
                }
            }
            JsonValue::String(s) => {
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        _ => out.push(c),
                    }
                }
                out.push('"');
            }
            JsonValue::Array(arr) => {
                out.push('[');
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.serialize_into(out);
                }
                out.push(']');
            }
            JsonValue::Object(map) => {
                out.push('{');
                let mut first = true;
                for (k, v) in map {
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    out.push('"');
                    out.push_str(k);
                    out.push_str("\":");
                    v.serialize_into(out);
                }
                out.push('}');
            }
        }
    }
}

fn skip_ws(chars: &[char], idx: &mut usize) {
    while *idx < chars.len() && chars[*idx].is_whitespace() {
        *idx += 1;
    }
}

fn parse_val(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    skip_ws(chars, idx);
    if *idx >= chars.len() {
        return Err("Unexpected EOF".to_string());
    }

    match chars[*idx] {
        '{' => parse_obj(chars, idx),
        '[' => parse_arr(chars, idx),
        '"' => parse_str(chars, idx).map(JsonValue::String),
        't' | 'f' => parse_bool(chars, idx),
        'n' => parse_null(chars, idx),
        '-' | '0'..='9' => parse_num(chars, idx),
        c => Err(format!("Unexpected character '{}' at offset {}", c, idx)),
    }
}

fn parse_obj(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    *idx += 1; // skip '{'
    skip_ws(chars, idx);
    let mut map = BTreeMap::new();

    if *idx < chars.len() && chars[*idx] == '}' {
        *idx += 1;
        return Ok(JsonValue::Object(map));
    }

    loop {
        skip_ws(chars, idx);
        if *idx >= chars.len() || chars[*idx] != '"' {
            return Err("Expected string key in object".to_string());
        }
        let key = parse_str(chars, idx)?;
        skip_ws(chars, idx);
        if *idx >= chars.len() || chars[*idx] != ':' {
            return Err("Expected ':' after key in object".to_string());
        }
        *idx += 1; // skip ':'
        let val = parse_val(chars, idx)?;
        map.insert(key, val);

        skip_ws(chars, idx);
        if *idx >= chars.len() {
            return Err("Unterminated object".to_string());
        }
        if chars[*idx] == ',' {
            *idx += 1;
        } else if chars[*idx] == '}' {
            *idx += 1;
            break;
        } else {
            return Err(format!("Expected ',' or '}}' at offset {}", idx));
        }
    }
    Ok(JsonValue::Object(map))
}

fn parse_arr(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    *idx += 1; // skip '['
    skip_ws(chars, idx);
    let mut arr = Vec::new();

    if *idx < chars.len() && chars[*idx] == ']' {
        *idx += 1;
        return Ok(JsonValue::Array(arr));
    }

    loop {
        let val = parse_val(chars, idx)?;
        arr.push(val);
        skip_ws(chars, idx);
        if *idx >= chars.len() {
            return Err("Unterminated array".to_string());
        }
        if chars[*idx] == ',' {
            *idx += 1;
        } else if chars[*idx] == ']' {
            *idx += 1;
            break;
        } else {
            return Err(format!("Expected ',' or ']' at offset {}", idx));
        }
    }
    Ok(JsonValue::Array(arr))
}

fn parse_str(chars: &[char], idx: &mut usize) -> Result<String, String> {
    *idx += 1; // skip '"'
    let mut s = String::new();
    while *idx < chars.len() {
        let c = chars[*idx];
        *idx += 1;
        match c {
            '"' => return Ok(s),
            '\\' => {
                if *idx >= chars.len() {
                    return Err("Escape at EOF".to_string());
                }
                let esc = chars[*idx];
                *idx += 1;
                match esc {
                    '"' => s.push('"'),
                    '\\' => s.push('\\'),
                    '/' => s.push('/'),
                    'n' => s.push('\n'),
                    'r' => s.push('\r'),
                    't' => s.push('\t'),
                    _ => s.push(esc),
                }
            }
            _ => s.push(c),
        }
    }
    Err("Unterminated string".to_string())
}

fn parse_bool(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    if chars[*idx..].starts_with(&['t', 'r', 'u', 'e']) {
        *idx += 4;
        Ok(JsonValue::Bool(true))
    } else if chars[*idx..].starts_with(&['f', 'a', 'l', 's', 'e']) {
        *idx += 5;
        Ok(JsonValue::Bool(false))
    } else {
        Err("Expected boolean".to_string())
    }
}

fn parse_null(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    if chars[*idx..].starts_with(&['n', 'u', 'l', 'l']) {
        *idx += 4;
        Ok(JsonValue::Null)
    } else {
        Err("Expected null".to_string())
    }
}

fn parse_num(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    let start = *idx;
    while *idx < chars.len() {
        let c = chars[*idx];
        if c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E' || c.is_ascii_digit() {
            *idx += 1;
        } else {
            break;
        }
    }
    let s: String = chars[start..*idx].iter().collect();
    match s.parse::<f64>() {
        Ok(n) => Ok(JsonValue::Number(n)),
        Err(_) => Err(format!("Invalid number: {}", s)),
    }
}
