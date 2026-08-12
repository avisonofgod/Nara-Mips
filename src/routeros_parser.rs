use std::collections::HashMap;

#[derive(Debug)]
pub struct ParsedCommand {
    pub path: String,
    pub args: HashMap<String, String>,
}

pub fn parse(input: &str) -> Option<ParsedCommand> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let mut parts = input.split_whitespace();
    let path = parts.next()?.to_string();
    let mut args = HashMap::new();
    for part in parts {
        if let Some((k, v)) = part.split_once('=') {
            args.insert(k.to_string(), v.trim_matches('"').to_string());
        } else if let Some((k, v)) = part.split_once(':') {
            args.insert(k.to_string(), v.trim_matches('"').to_string());
        }
    }
    Some(ParsedCommand { path, args })
}
