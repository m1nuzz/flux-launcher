use std::fs;
use std::path::{Path, PathBuf};

use flux_core::{ResultKind, ResultSource, SearchResult};

use super::builtin::{BuiltinAction, BuiltinProvider, BuiltinQuery, BuiltinResult, MAX_RESULTS};

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

pub(crate) struct ObsidianProvider;

impl BuiltinProvider for ObsidianProvider {
    fn query(&self, request: &BuiltinQuery) -> Vec<BuiltinResult> {
        if !request.obsidian_enabled {
            return Vec::new();
        }
        let keyword = super::builtin::normalized_keyword(&request.obsidian_keyword, "ob");
        let Some(search) = super::builtin::search_for_keyword(&request.query, keyword) else {
            return Vec::new();
        };
        if search.is_empty() {
            return Vec::new();
        }
        let vaults = discover_vaults();
        obsidian_results_for_vaults(&vaults, search)
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
            subtitle: format!("Obsidian • {} • {}", file.vault_name, file.relative_path),
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
                title: format!("Create note: {note_name}"),
                subtitle: format!("Obsidian • {}", vault.name),
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
        super::builtin::encode_uri_component(vault_id),
        super::builtin::encode_uri_component(relative_path)
    )
}

fn new_note_uri(vault_id: &str, note_name: &str) -> String {
    format!(
        "obsidian://new?vault={}&name={}",
        super::builtin::encode_uri_component(vault_id),
        super::builtin::encode_uri_component(note_name)
    )
}

fn path_to_slash_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn obsidian_create_prefix_returns_one_option_per_vault() {
        let vaults = vec![fake_vault("a", "Notes"), fake_vault("b", "Work")];
        let results = obsidian_results_for_vaults(&vaults, "create Daily log");
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|result| {
            result.result.title == "Create note: Daily log"
                && result.result.subtitle.starts_with("Obsidian • ")
        }));
        let uris: Vec<String> = results
            .iter()
            .filter_map(|result| match &result.action {
                Some(BuiltinAction::OpenUrl(url)) => Some(url.clone()),
                _ => None,
            })
            .collect();
        assert!(uris.contains(&"obsidian://new?vault=a&name=Daily%20log".to_owned()));
        assert!(uris.contains(&"obsidian://new?vault=b&name=Daily%20log".to_owned()));
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

    #[test]
    fn obsidian_search_with_matches_does_not_fall_back_to_create() {
        let dir = tempdir();
        let note = dir.path().join("welcome.md");
        std::fs::write(&note, "---\naliases: [home]\n---\n").expect("write note");
        let vaults = vec![Vault {
            id: String::from("a"),
            name: String::from("Notes"),
            path: dir.path().to_path_buf(),
        }];
        let results = obsidian_results_for_vaults(&vaults, "welcome");
        assert_eq!(results.len(), 1);
        assert!(results[0].result.title == "welcome");
        assert!(!results[0].result.id.starts_with("builtin:obsidian:create:"));
    }

    fn tempdir() -> TempDir {
        let base = std::env::temp_dir();
        let unique = format!(
            "flux-launcher-obsidian-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or_default()
        );
        let path = base.join(unique);
        std::fs::create_dir_all(&path).expect("create temp dir");
        TempDir(path)
    }

    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    impl TempDir {
        fn path(&self) -> &Path {
            &self.0
        }
    }
}
