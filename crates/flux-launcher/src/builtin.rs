use std::fs;
use std::path::{Path, PathBuf};

use flux_core::{ResultKind, ResultSource, SearchResult};

const MAX_RESULTS: usize = 16;
const MAX_FILES_PER_VAULT: usize = 20_000;
const SEARCHABLE_EXTENSIONS: &[&str] = &[
    "md",
    "canvas",
    "excalidraw",
    "png",
    "jpg",
    "jpeg",
    "gif",
    "bmp",
    "svg",
    "webp",
    "pdf",
    "json",
    "csv",
];

#[derive(Clone, Debug)]
pub struct BuiltinQuery {
    pub query: String,
    pub google_enabled: bool,
    pub google_keyword: String,
    pub obsidian_enabled: bool,
    pub obsidian_keyword: String,
}

#[derive(Clone, Debug)]
pub enum BuiltinAction {
    OpenUrl(String),
    CopyText(String),
}

#[derive(Clone, Debug)]
pub struct BuiltinResult {
    pub result: SearchResult,
    pub action: Option<BuiltinAction>,
}

pub trait BuiltinProvider: Send + Sync {
    fn query(&self, request: &BuiltinQuery) -> Vec<BuiltinResult>;
}

pub fn query_builtin_providers(request: &BuiltinQuery) -> Vec<BuiltinResult> {
    let providers: [&dyn BuiltinProvider; 3] =
        [&CalculatorProvider, &GoogleProvider, &ObsidianProvider];
    providers
        .into_iter()
        .flat_map(|provider| provider.query(request))
        .take(MAX_RESULTS)
        .collect()
}

struct CalculatorProvider;

impl BuiltinProvider for CalculatorProvider {
    fn query(&self, request: &BuiltinQuery) -> Vec<BuiltinResult> {
        let expression = request.query.trim();
        if expression.len() > CALCULATOR_MAX_QUERY_LENGTH
            || !expression.chars().any(is_calculator_operator)
        {
            return Vec::new();
        }
        let Ok(value) = evaluate_expression(expression) else {
            return Vec::new();
        };
        let formatted = format_calculator_value(value);
        vec![BuiltinResult {
            result: SearchResult {
                id: String::from("builtin:calculator"),
                title: t!("builtin.calculator.result", value = formatted).into_owned(),
                subtitle: t!("builtin.calculator.subtitle", expression = expression).into_owned(),
                kind: ResultKind::Placeholder,
                source: ResultSource::Plugin,
                target: None,
            },
            action: Some(BuiltinAction::CopyText(formatted)),
        }]
    }
}

const CALCULATOR_MAX_QUERY_LENGTH: usize = 128;

fn is_calculator_operator(character: char) -> bool {
    matches!(character, '+' | '-' | '*' | '/' | '%' | '^' | '(' | ')')
}

fn evaluate_expression(expression: &str) -> Result<f64, ()> {
    let mut parser = CalculatorParser::new(expression);
    let value = parser.parse_expression()?;
    if parser.has_remaining_input() || !value.is_finite() {
        return Err(());
    }
    Ok(if value == 0.0 { 0.0 } else { value })
}

fn format_calculator_value(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() <= 9_007_199_254_740_991.0 {
        return format!("{}", value as i64);
    }
    let mut formatted = format!("{value:.12}");
    while formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    formatted
}

struct CalculatorParser {
    characters: Vec<char>,
    position: usize,
}

impl CalculatorParser {
    fn new(expression: &str) -> Self {
        Self {
            characters: expression.chars().collect(),
            position: 0,
        }
    }

    fn parse_expression(&mut self) -> Result<f64, ()> {
        self.parse_add_subtract()
    }

    fn parse_add_subtract(&mut self) -> Result<f64, ()> {
        let mut value = self.parse_multiply_divide()?;
        loop {
            if self.consume('+') {
                value += self.parse_multiply_divide()?;
            } else if self.consume('-') {
                value -= self.parse_multiply_divide()?;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_multiply_divide(&mut self) -> Result<f64, ()> {
        let mut value = self.parse_unary()?;
        loop {
            if self.consume('*') {
                value *= self.parse_unary()?;
            } else if self.consume('/') {
                let divisor = self.parse_unary()?;
                if divisor == 0.0 {
                    return Err(());
                }
                value /= divisor;
            } else if self.consume('%') {
                let divisor = self.parse_unary()?;
                if divisor == 0.0 {
                    return Err(());
                }
                value %= divisor;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_unary(&mut self) -> Result<f64, ()> {
        if self.consume('+') {
            return self.parse_unary();
        }
        if self.consume('-') {
            return Ok(-self.parse_unary()?);
        }
        self.parse_power()
    }

    fn parse_power(&mut self) -> Result<f64, ()> {
        let base = self.parse_primary()?;
        if self.consume('^') {
            let exponent = self.parse_unary()?;
            let value = base.powf(exponent);
            if !value.is_finite() {
                return Err(());
            }
            return Ok(value);
        }
        Ok(base)
    }

    fn parse_primary(&mut self) -> Result<f64, ()> {
        self.skip_whitespace();
        if self.consume('(') {
            let value = self.parse_add_subtract()?;
            if !self.consume(')') {
                return Err(());
            }
            return Ok(value);
        }
        let start = self.position;
        let mut digits = 0_usize;
        let mut dots = 0_usize;
        while let Some(character) = self.characters.get(self.position).copied() {
            if character.is_ascii_digit() {
                digits += 1;
                self.position += 1;
            } else if character == '.' {
                dots += 1;
                if dots > 1 {
                    return Err(());
                }
                self.position += 1;
            } else {
                break;
            }
        }
        if digits == 0 {
            return Err(());
        }
        self.characters[start..self.position]
            .iter()
            .collect::<String>()
            .parse::<f64>()
            .map_err(|_| ())
    }

    fn consume(&mut self, expected: char) -> bool {
        self.skip_whitespace();
        if self.characters.get(self.position) == Some(&expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn skip_whitespace(&mut self) {
        while self
            .characters
            .get(self.position)
            .is_some_and(|character| character.is_whitespace())
        {
            self.position += 1;
        }
    }

    fn has_remaining_input(&mut self) -> bool {
        self.skip_whitespace();
        self.position != self.characters.len()
    }
}

struct GoogleProvider;

impl BuiltinProvider for GoogleProvider {
    fn query(&self, request: &BuiltinQuery) -> Vec<BuiltinResult> {
        if !request.google_enabled {
            return Vec::new();
        }
        let keyword = normalized_keyword(&request.google_keyword, "g");
        let Some(search) = search_for_keyword(&request.query, keyword) else {
            return Vec::new();
        };
        if search.is_empty() {
            return Vec::new();
        }
        let url = format!(
            "https://www.google.com/search?q={}",
            encode_uri_component(search)
        );
        vec![BuiltinResult {
            result: SearchResult {
                id: String::from("builtin:google-search"),
                title: t!("builtin.google.title", query = search).into_owned(),
                subtitle: t!("builtin.google.subtitle").into_owned(),
                kind: ResultKind::Placeholder,
                source: ResultSource::Plugin,
                target: None,
            },
            action: Some(BuiltinAction::OpenUrl(url)),
        }]
    }
}

struct ObsidianProvider;

impl BuiltinProvider for ObsidianProvider {
    fn query(&self, request: &BuiltinQuery) -> Vec<BuiltinResult> {
        if !request.obsidian_enabled {
            return Vec::new();
        }
        let keyword = normalized_keyword(&request.obsidian_keyword, "ob");
        let Some(search) = search_for_keyword(&request.query, keyword) else {
            return Vec::new();
        };
        if search.is_empty() {
            return Vec::new();
        }
        let vaults = discover_vaults();
        if let Some(note_name) = search
            .strip_prefix("create ")
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            return create_note_results(note_name, &vaults);
        }
        let terms = search
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let files = vaults
            .iter()
            .flat_map(collect_vault_files)
            .collect::<Vec<_>>();
        let mut scored = files
            .into_iter()
            .filter_map(|file| score_file(&file, &terms).map(|score| (score, file)))
            .collect::<Vec<_>>();
        scored.sort_by(|(left_score, left), (right_score, right)| {
            right_score.cmp(left_score).then_with(|| {
                normalize_search_text(&left.relative_path)
                    .cmp(&normalize_search_text(&right.relative_path))
            })
        });
        let results: Vec<BuiltinResult> = scored
            .into_iter()
            .take(MAX_RESULTS)
            .map(|(_, file)| file_result(file))
            .collect();
        if results.is_empty() && !vaults.is_empty() {
            return create_note_results(search.trim(), &vaults);
        }
        results
    }
}

fn normalized_keyword<'a>(keyword: &'a str, fallback: &'a str) -> &'a str {
    let keyword = keyword.trim();
    if keyword.is_empty() {
        fallback
    } else {
        keyword
    }
}

fn search_for_keyword<'a>(query: &'a str, keyword: &str) -> Option<&'a str> {
    if query != keyword
        && !query.strip_prefix(keyword).is_some_and(|rest| {
            rest.starts_with(':') || rest.chars().next().is_some_and(char::is_whitespace)
        })
    {
        return None;
    }
    Some(
        query
            .strip_prefix(keyword)
            .unwrap_or_default()
            .trim_start_matches(|character: char| character == ':' || character.is_whitespace())
            .trim(),
    )
}

fn discover_vaults() -> Vec<Vault> {
    let Some(app_data) = std::env::var_os("APPDATA") else {
        return Vec::new();
    };
    let path = PathBuf::from(app_data)
        .join("obsidian")
        .join("obsidian.json");
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    root.get("vaults")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|vaults| vaults.iter())
        .filter_map(|(id, value)| {
            let path = value
                .get("path")
                .and_then(serde_json::Value::as_str)
                .map(PathBuf::from)?;
            if !path.is_dir() {
                return None;
            }
            let name = value
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| id.clone());
            Some(Vault {
                id: id.clone(),
                name,
                path,
            })
        })
        .collect()
}

#[derive(Clone, Debug)]
struct Vault {
    id: String,
    name: String,
    path: PathBuf,
}

#[derive(Clone, Debug)]
struct VaultFile {
    vault_id: String,
    vault_name: String,
    path: PathBuf,
    relative_path: String,
    aliases: Vec<String>,
}

fn collect_vault_files(vault: &Vault) -> Vec<VaultFile> {
    let mut files = Vec::new();
    collect_files_recursive(vault, &vault.path, &mut files);
    files
}

fn collect_files_recursive(vault: &Vault, directory: &Path, files: &mut Vec<VaultFile>) {
    if files.len() >= MAX_FILES_PER_VAULT {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if files.len() >= MAX_FILES_PER_VAULT {
            return;
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name == ".obsidian" || name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_files_recursive(vault, &path, files);
            continue;
        }
        if !path.is_file() || !is_searchable_file(&path) {
            continue;
        }
        let relative_path = path
            .strip_prefix(&vault.path)
            .ok()
            .map(path_to_slash_string)
            .unwrap_or_else(|| name.to_owned());
        let aliases = if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            read_aliases(&path)
        } else {
            Vec::new()
        };
        files.push(VaultFile {
            vault_id: vault.id.clone(),
            vault_name: vault.name.clone(),
            path,
            relative_path,
            aliases,
        });
    }
}

fn is_searchable_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            SEARCHABLE_EXTENSIONS
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        })
}

fn read_aliases(path: &Path) -> Vec<String> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut aliases = Vec::new();
    let mut in_frontmatter = false;
    let mut in_aliases = false;
    for line in content.lines().take(160) {
        let trimmed = line.trim();
        if trimmed == "---" {
            if in_frontmatter {
                break;
            }
            in_frontmatter = true;
            continue;
        }
        if !in_frontmatter {
            continue;
        }
        if trimmed.starts_with("aliases:") || trimmed.starts_with("alias:") {
            in_aliases = true;
            let inline = trimmed
                .split_once(':')
                .map(|(_, value)| value.trim())
                .unwrap_or_default();
            aliases.extend(parse_alias_list(inline));
            continue;
        }
        if in_aliases && (trimmed.starts_with('-') || trimmed.starts_with('[')) {
            aliases.extend(parse_alias_list(trimmed));
            continue;
        }
        if in_aliases && !trimmed.is_empty() && !trimmed.starts_with('#') {
            in_aliases = false;
        }
    }
    aliases
}

fn parse_alias_list(value: &str) -> Vec<String> {
    value
        .trim_matches(['[', ']'])
        .split(',')
        .map(|item| {
            item.trim()
                .trim_start_matches('-')
                .trim()
                .trim_matches(['"', '\''])
        })
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn normalize_search_text(value: &str) -> String {
    value.to_lowercase()
}

fn score_file(file: &VaultFile, terms: &[String]) -> Option<i32> {
    let stem = file
        .path
        .file_stem()
        .and_then(|name| name.to_str())
        .map(normalize_search_text)
        .unwrap_or_default();
    let relative = normalize_search_text(&file.relative_path);
    let aliases = file
        .aliases
        .iter()
        .map(|alias| normalize_search_text(alias))
        .collect::<Vec<_>>();
    let terms = terms
        .iter()
        .map(|term| normalize_search_text(term))
        .collect::<Vec<_>>();
    if !terms.iter().all(|term| {
        stem.contains(term)
            || relative.contains(term)
            || aliases.iter().any(|alias| alias.contains(term))
    }) {
        return None;
    }
    let mut score = 100;
    let joined_aliases = aliases.join(" ");
    let haystack = format!("{stem} {relative} {joined_aliases}");
    for term in terms {
        if stem == *term {
            score += 1_000;
        } else if stem.starts_with(&term) {
            score += 700;
        } else if joined_aliases.contains(&term) {
            score += 550;
        } else if relative.contains(&term) {
            score += 350;
        } else if haystack.contains(&term) {
            score += 200;
        }
    }
    Some(score)
}

fn file_result(file: VaultFile) -> BuiltinResult {
    let title = file
        .path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(&file.relative_path)
        .to_owned();
    BuiltinResult {
        result: SearchResult {
            id: format!(
                "builtin:obsidian:{}",
                file.relative_path.to_ascii_lowercase()
            ),
            title,
            subtitle: t!(
                "builtin.obsidian.file_subtitle",
                vault = file.vault_name,
                path = file.relative_path
            )
            .into_owned(),
            kind: ResultKind::Placeholder,
            source: ResultSource::Plugin,
            target: None,
        },
        action: Some(BuiltinAction::OpenUrl(open_uri_for_file(
            &file.vault_id,
            &file.relative_path,
        ))),
    }
}

fn create_note_results(note_name: &str, vaults: &[Vault]) -> Vec<BuiltinResult> {
    vaults
        .iter()
        .take(MAX_RESULTS)
        .map(|vault| BuiltinResult {
            result: SearchResult {
                id: format!("builtin:obsidian:create:{}", vault.id),
                title: t!("builtin.obsidian.create_note", name = note_name).into_owned(),
                subtitle: t!("builtin.obsidian.vault_subtitle", vault = vault.name).into_owned(),
                kind: ResultKind::Placeholder,
                source: ResultSource::Plugin,
                target: None,
            },
            action: Some(BuiltinAction::OpenUrl(new_note_uri(&vault.id, note_name))),
        })
        .collect()
}

fn open_uri_for_file(vault_id: &str, relative_path: &str) -> String {
    format!(
        "obsidian://open?vault={}&file={}",
        encode_uri_component(vault_id),
        encode_uri_component(relative_path)
    )
}

fn new_note_uri(vault_id: &str, note_name: &str) -> String {
    format!(
        "obsidian://new?vault={}&name={}",
        encode_uri_component(vault_id),
        encode_uri_component(note_name)
    )
}

fn encode_uri_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn path_to_slash_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculator_provider_returns_flow_style_result_and_copy_action() {
        let request = BuiltinQuery {
            query: String::from("1 + 2 * 3"),
            google_enabled: false,
            google_keyword: String::from("g"),
            obsidian_enabled: false,
            obsidian_keyword: String::from("ob"),
        };
        let results = CalculatorProvider.query(&request);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].result.title, "= 7");
        assert_eq!(results[0].result.subtitle, "Calculator • 1 + 2 * 3");
        assert!(matches!(
            &results[0].action,
            Some(BuiltinAction::CopyText(value)) if value == "7"
        ));
    }

    #[test]
    fn calculator_supports_parentheses_unary_signs_and_decimals() {
        assert_eq!(evaluate_expression("-(2 + 3) * 0.5"), Ok(-2.5));
        assert_eq!(evaluate_expression("2^3^2"), Ok(512.0));
        assert_eq!(format_calculator_value(10.0 / 3.0), "3.333333333333");
    }

    #[test]
    fn calculator_date_like_queries_are_explicitly_covered_for_policy_review() {
        let results = CalculatorProvider.query(&BuiltinQuery {
            query: String::from("2026-08"),
            google_enabled: false,
            google_keyword: String::from("g"),
            obsidian_enabled: false,
            obsidian_keyword: String::from("ob"),
        });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].result.title, "= 2018");
        assert_eq!(results[0].result.subtitle, "Calculator • 2026-08");
    }

    #[test]
    fn calculator_subtraction_remains_supported() {
        let results = CalculatorProvider.query(&BuiltinQuery {
            query: String::from("1-1"),
            google_enabled: false,
            google_keyword: String::from("g"),
            obsidian_enabled: false,
            obsidian_keyword: String::from("ob"),
        });
        assert_eq!(results[0].result.title, "= 0");
    }

    #[test]
    fn calculator_rejects_invalid_or_unsafe_input() {
        assert!(evaluate_expression("1 / 0").is_err());
        assert!(evaluate_expression("1 +").is_err());
        assert!(evaluate_expression("1 + process").is_err());
        assert!(CalculatorProvider
            .query(&BuiltinQuery {
                query: String::from("hello"),
                google_enabled: false,
                google_keyword: String::from("g"),
                obsidian_enabled: false,
                obsidian_keyword: String::from("ob"),
            })
            .is_empty());
    }

    #[test]
    fn google_provider_matches_alias_and_encodes_url() {
        let request = BuiltinQuery {
            query: String::from("g space exploration"),
            google_enabled: true,
            google_keyword: String::from("g"),
            obsidian_enabled: true,
            obsidian_keyword: String::from("ob"),
        };
        let results = GoogleProvider.query(&request);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].result.title, "Search Google: space exploration");
        assert!(matches!(
            &results[0].action,
            Some(BuiltinAction::OpenUrl(url)) if url == "https://www.google.com/search?q=space%20exploration"
        ));
    }

    #[test]
    fn aliases_require_a_token_boundary() {
        assert!(search_for_keyword("ob meeting", "ob").is_some());
        assert!(search_for_keyword("ob:meeting", "ob").is_some());
        assert!(search_for_keyword("ob", "ob").is_some_and(str::is_empty));
        assert!(search_for_keyword("object", "ob").is_none());
    }

    #[test]
    fn obsidian_file_scoring_requires_all_terms() {
        let file = VaultFile {
            vault_id: String::from("vault"),
            vault_name: String::from("Notes"),
            path: PathBuf::from("C:/Vault/Meeting notes.md"),
            relative_path: String::from("Meeting notes.md"),
            aliases: vec![String::from("sync")],
        };
        assert!(score_file(&file, &[String::from("meeting"), String::from("sync")]).is_some());
        assert!(score_file(&file, &[String::from("missing")]).is_none());
    }

    #[test]
    fn obsidian_file_scoring_is_unicode_case_insensitive() {
        let file = VaultFile {
            vault_id: String::from("vault"),
            vault_name: String::from("Заметки"),
            path: PathBuf::from("C:/Vault/Контент-машина.md"),
            relative_path: String::from("Контент-машина.md"),
            aliases: Vec::new(),
        };
        assert!(score_file(&file, &[String::from("контент-машина")]).is_some());
        assert!(score_file(&file, &[String::from("КОНТЕНТ-МАШИНА")]).is_some());
    }

    fn fake_vault(id: &str, name: &str) -> Vault {
        Vault {
            id: id.to_owned(),
            name: name.to_owned(),
            path: PathBuf::from(format!("C:/vaults/{id}")),
        }
    }

    fn obsidian_results_for_vaults(vaults: &[Vault], search: &str) -> Vec<BuiltinResult> {
        if let Some(note_name) = search
            .strip_prefix("create ")
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            return create_note_results(note_name, vaults);
        }
        let terms = search
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let files = vaults
            .iter()
            .flat_map(collect_vault_files)
            .collect::<Vec<_>>();
        let mut scored = files
            .into_iter()
            .filter_map(|file| score_file(&file, &terms).map(|score| (score, file)))
            .collect::<Vec<_>>();
        scored.sort_by(|(left_score, left), (right_score, right)| {
            right_score.cmp(left_score).then_with(|| {
                normalize_search_text(&left.relative_path)
                    .cmp(&normalize_search_text(&right.relative_path))
            })
        });
        let results: Vec<BuiltinResult> = scored
            .into_iter()
            .take(MAX_RESULTS)
            .map(|(_, file)| file_result(file))
            .collect();
        if results.is_empty() && !vaults.is_empty() {
            return create_note_results(search.trim(), vaults);
        }
        results
    }

    #[test]
    fn obsidian_no_matches_falls_back_to_create_note_per_vault() {
        let vaults = vec![fake_vault("a", "Notes"), fake_vault("b", "Work")];
        let results = obsidian_results_for_vaults(&vaults, "totally-new-idea");
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|result| {
            result.result.title == "Create note: totally-new-idea"
                && result.result.id.starts_with("builtin:obsidian:create:")
        }));
    }

    #[test]
    fn obsidian_no_matches_with_no_vaults_returns_empty() {
        let results = obsidian_results_for_vaults(&[], "anything");
        assert!(results.is_empty());
    }
}
