use std::collections::BTreeMap;
use std::path::Path;

/// A parsed `.emc` (Englishmen Configuration) file.
///
/// The format is a minimal TOML-style key/value + `[section]` format —
/// intentionally not a natural-language parser.
#[derive(Debug, Clone, Default)]
pub struct EmConfig {
    pub root: BTreeMap<String, String>,
    pub sections: BTreeMap<String, BTreeMap<String, String>>,
}

impl EmConfig {
    pub fn parse(text: &str) -> EmConfig {
        let mut config = EmConfig::default();
        let mut current_section: Option<String> = None;

        for raw_line in text.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                let name = line[1..line.len() - 1].trim().to_string();
                config.sections.entry(name.clone()).or_default();
                current_section = Some(name);
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_string();
                let value = parse_value(value.trim());
                match &current_section {
                    Some(section) => {
                        config
                            .sections
                            .entry(section.clone())
                            .or_default()
                            .insert(key, value);
                    }
                    None => {
                        config.root.insert(key, value);
                    }
                }
            }
        }

        config
    }

    pub fn load(path: &Path) -> std::io::Result<EmConfig> {
        let text = std::fs::read_to_string(path)?;
        Ok(EmConfig::parse(&text))
    }

    #[allow(dead_code)]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.root.get(key).map(|s| s.as_str())
    }

    #[allow(dead_code)]
    pub fn name(&self) -> Option<&str> {
        self.get("name")
    }

    #[allow(dead_code)]
    pub fn main_file(&self) -> Option<&str> {
        self.get("main")
    }
}

fn parse_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn default_emc(project_name: &str) -> String {
    format!("name = \"{project_name}\"\nversion = \"1.0.0\"\nmain = \"main.em\"\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_root_key_values() {
        let config =
            EmConfig::parse("name = \"MyProject\"\nversion = \"1.0.0\"\nmain = \"main.em\"\n");
        assert_eq!(config.name(), Some("MyProject"));
        assert_eq!(config.get("version"), Some("1.0.0"));
        assert_eq!(config.main_file(), Some("main.em"));
    }

    #[test]
    fn parses_sections() {
        let config =
            EmConfig::parse("[installer]\ninstall_dir = \"C:/em\"\nprojects_enabled = \"true\"\n");
        let section = config.sections.get("installer").unwrap();
        assert_eq!(section.get("install_dir").unwrap(), "C:/em");
        assert_eq!(section.get("projects_enabled").unwrap(), "true");
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let config = EmConfig::parse("# a comment\n\nname = \"X\"\n");
        assert_eq!(config.name(), Some("X"));
    }

    #[test]
    fn default_emc_contains_expected_keys() {
        let text = default_emc("MyProject");
        let config = EmConfig::parse(&text);
        assert_eq!(config.name(), Some("MyProject"));
        assert_eq!(config.main_file(), Some("main.em"));
    }
}
