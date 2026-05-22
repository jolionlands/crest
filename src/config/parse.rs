use anyhow::{bail, Result};
use std::collections::HashMap;

use super::types::*;

/// Parse a KDL configuration string into a `Config`.
///
/// Supports:
///   - `bar { position "top" height 32 ... }`
///   - `modules-left { workspaces { format "{name}" ... } }`
///   - `modules-center { ... }`
///   - `modules-right { ... }`
///   - `style { background "#1e1e1e" ... }`
///   - Top-level `log-level "debug"`
///
/// Comments: `//` line comments are stripped.  Inline `//` after a value is
/// also stripped (not inside strings).
pub fn parse_kdl_config(input: &str) -> Result<Config> {
    let mut config = Config::default();

    // Stack of open section names joined with dots, e.g. "modules-left.workspaces"
    let mut section_stack: Vec<String> = Vec::new();

    // Accumulate a module entry while we're inside a module block.
    let mut current_module: Option<ModuleEntry> = None;

    for raw_line in input.lines() {
        let line = raw_line.trim();

        // Skip empty lines and full-line comments
        if line.is_empty() || line.starts_with("//") {
            continue;
        }

        // Strip inline comments (outside quoted strings)
        let line = strip_inline_comment(line);

        // Section open: `section-name {` or `section-name "arg" {`
        if let Some(section_name) = try_parse_section_open(line) {
            let depth = section_stack.len();
            let parent = section_stack.last().map(|s| s.as_str()).unwrap_or("");

            // Starting a new module inside a modules-* block
            if depth == 1
                && (parent == "modules-left"
                    || parent == "modules-center"
                    || parent == "modules-right")
            {
                let mut entry = ModuleEntry::default();
                entry.kind = section_name.clone();
                current_module = Some(entry);
            }

            section_stack.push(section_name);
            continue;
        }

        // Section close: `}`
        if line.starts_with('}') {
            if let Some(closed) = section_stack.pop() {
                let depth = section_stack.len();
                let parent = section_stack.last().map(|s| s.as_str()).unwrap_or("");

                // Finalise a module entry when its block closes
                if depth == 1
                    && (parent == "modules-left"
                        || parent == "modules-center"
                        || parent == "modules-right")
                {
                    if let Some(module) = current_module.take() {
                        match parent {
                            "modules-left" => config.modules.left.push(module),
                            "modules-center" => config.modules.center.push(module),
                            "modules-right" => config.modules.right.push(module),
                            _ => {}
                        }
                    }
                }

                let _ = closed;
            }
            continue;
        }

        // Build a dotted section path for dispatch
        let section = section_stack.join(".");

        // Parse key-value pairs
        if let Some((key, value)) = parse_property(line) {
            match section.as_str() {
                // Top-level directives
                "" => match key.as_str() {
                    "log-level" | "log_level" => config.log_level = value,
                    _ => {}
                },

                // bar { ... }
                "bar" => apply_bar_property(&key, &value, &mut config.bar),

                // style { ... }
                "style" => apply_style_property(&key, &value, &mut config.style),

                // Properties directly inside a module block
                s if is_module_section(s) => {
                    if let Some(ref mut module) = current_module {
                        apply_module_property(&key, &value, module);
                    }
                }

                _ => {}
            }
        }
    }

    Ok(config)
}

/// Returns true when the section path is a module block,
/// e.g. "modules-left.workspaces" or "modules-right.clock".
fn is_module_section(section: &str) -> bool {
    section.starts_with("modules-left.")
        || section.starts_with("modules-center.")
        || section.starts_with("modules-right.")
}

// ---------------------------------------------------------------------------
// Property applicators
// ---------------------------------------------------------------------------

fn apply_bar_property(key: &str, value: &str, bar: &mut BarConfig) {
    match key {
        "position" => bar.position = value.to_string(),
        "height" => {
            if let Ok(v) = value.parse() {
                bar.height = v;
            }
        }
        "multi-monitor" | "multi_monitor" => {
            bar.multi_monitor = parse_bool(value);
        }
        "click-through" | "click_through" => {
            bar.click_through = parse_bool(value);
        }
        _ => {}
    }
}

fn apply_style_property(key: &str, value: &str, style: &mut StyleConfig) {
    match key {
        "background" => style.background = value.to_string(),
        "foreground" => style.foreground = value.to_string(),
        "accent" => style.accent = value.to_string(),
        "font-family" | "font_family" => style.font_family = value.to_string(),
        "font-size-pt" | "font_size_pt" => {
            if let Ok(v) = value.parse() {
                style.font_size_pt = v;
            }
        }
        "padding-px" | "padding_px" => {
            if let Ok(v) = value.parse() {
                style.padding_px = v;
            }
        }
        "module-spacing-px" | "module_spacing_px" => {
            if let Ok(v) = value.parse() {
                style.module_spacing_px = v;
            }
        }
        _ => {}
    }
}

fn apply_module_property(key: &str, value: &str, module: &mut ModuleEntry) {
    match key {
        "format" => module.format = value.to_string(),
        "interval-ms" | "interval_ms" => {
            if let Ok(v) = value.parse() {
                module.interval_ms = v;
            }
        }
        "on-click" | "on_click" => module.on_click = Some(value.to_string()),
        "on-scroll-up" | "on_scroll_up" => module.on_scroll_up = Some(value.to_string()),
        "on-scroll-down" | "on_scroll_down" => module.on_scroll_down = Some(value.to_string()),
        other => {
            module.extra.insert(other.to_string(), value.to_string());
        }
    }
}

// ---------------------------------------------------------------------------
// Lexer helpers
// ---------------------------------------------------------------------------

/// Detect a section-open line: `name {` or `name "arg" {`.
/// Returns just the section name (without the quoted arg for now — module kind
/// is the name, not the arg).
fn try_parse_section_open(line: &str) -> Option<String> {
    let line = line.trim();
    if !line.ends_with('{') {
        return None;
    }
    let inner = line[..line.len() - 1].trim();
    if inner.is_empty() {
        return None;
    }
    // The section name is the first token
    let name = inner
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    if name.is_empty() {
        return None;
    }
    Some(name)
}

/// Strip `//` inline comments, but not those inside quoted strings.
fn strip_inline_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    for (i, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if !in_string
            && ch == '/'
            && i + 1 < line.len()
            && line.as_bytes().get(i + 1) == Some(&b'/')
        {
            return line[..i].trim_end();
        }
    }
    line
}

/// Parse a KDL-style key–value pair.
///
/// Accepted forms:
///   - `key "value"`   (KDL string argument)
///   - `key value`     (bare word — number, bool, or unquoted string)
///   - `key=value`     (equals style)
///   - `key="value"`
fn parse_property(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    // key=value or key="value"
    if let Some(pos) = line.find('=') {
        let key = line[..pos].trim().to_string();
        let value = line[pos + 1..].trim().trim_matches('"').to_string();
        if !key.is_empty() && !key.contains('{') && !key.contains('}') {
            return Some((key, value));
        }
    }

    // key "value" or key bare_value
    let parts: Vec<&str> = line.splitn(2, |c: char| c.is_whitespace()).collect();
    if parts.len() == 2 {
        let key = parts[0].trim().to_string();
        let value = parts[1].trim().trim_matches('"').to_string();
        if !key.is_empty() && !key.contains('{') && !key.contains('}') {
            return Some((key, value));
        }
    }

    None
}

fn parse_bool(value: &str) -> bool {
    matches!(value.trim().to_lowercase().as_str(), "true" | "1" | "yes" | "on")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_KDL: &str = include_str!("../../resources/default_config.kdl");

    #[test]
    fn test_config_default_parses() {
        let cfg = parse_kdl_config(DEFAULT_KDL).expect("default config should parse");
        assert_eq!(cfg.bar.position, "top");
        assert_eq!(cfg.bar.height, 32);
        assert!(!cfg.bar.multi_monitor);
        assert_eq!(cfg.style.font_family, "Segoe UI");
        assert!((cfg.style.font_size_pt - 10.0).abs() < f32::EPSILON);
        assert_eq!(cfg.style.padding_px, 8);
        assert_eq!(cfg.modules.left.len(), 1);
        assert_eq!(cfg.modules.left[0].kind, "workspaces");
        assert_eq!(cfg.modules.center.len(), 1);
        assert_eq!(cfg.modules.center[0].kind, "focused-window");
        assert_eq!(cfg.modules.right.len(), 2);
        assert_eq!(cfg.modules.right[0].kind, "clock");
        assert_eq!(cfg.modules.right[1].kind, "battery");
    }

    #[test]
    fn test_inline_comment_stripped() {
        let line = r#"height 32 // this is a comment"#;
        let stripped = strip_inline_comment(line);
        assert_eq!(stripped.trim(), "height 32");
    }

    #[test]
    fn test_parse_property_kdl_style() {
        let result = parse_property(r#"font-family "Segoe UI""#);
        assert_eq!(result, Some(("font-family".to_string(), "Segoe UI".to_string())));
    }

    #[test]
    fn test_parse_property_equals_style() {
        let result = parse_property(r#"height=32"#);
        assert_eq!(result, Some(("height".to_string(), "32".to_string())));
    }
}
