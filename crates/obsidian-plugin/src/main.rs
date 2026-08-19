use serde_json::{json, Value};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;

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

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines().map_while(Result::ok) {
        let response = handle_request(&line);
        if serde_json::to_writer(&mut stdout, &response).is_ok() {
            let _ = stdout.write_all(b"\n");
            let _ = stdout.flush();
        }
    }
}

fn handle_request(line: &str) -> Value {
    let Ok(request) = serde_json::from_str::<Value>(line) else {
        return json!({"jsonrpc":"2.0","id":1,"error":{"code":-32700,"message":"Invalid JSON"}});
    };
    let id = request.get("id").cloned().unwrap_or_else(|| json!(1));
    match request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "query" => {
            json!({"jsonrpc":"2.0","id":id,"result":query_results(query_from_request(&request))})
        }
        "execute" => {
            let params = request
                .get("params")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let success = params.first().and_then(Value::as_str).is_some_and(open_uri);
            json!({"jsonrpc":"2.0","id":id,"result":success})
        }
        _ => json!({"jsonrpc":"2.0","id":id,"result":[]}),
    }
}

fn query_from_request(request: &Value) -> String {
    request
        .get("params")
        .and_then(Value::as_array)
        .and_then(|params| params.first())
        .and_then(|value| value.get("search"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn query_results(query: String) -> Vec<Value> {
    if query.is_empty() {
        return Vec::new();
    }
    let vaults = discover_vaults();
    if let Some(note_name) = query
        .strip_prefix("create ")
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        return create_note_results(note_name, &vaults);
    }
    let files = vaults
        .iter()
        .flat_map(collect_vault_files)
        .collect::<Vec<_>>();
    let terms = query
        .split_whitespace()
        .map(|term| term.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let mut scored = files
        .into_iter()
        .filter_map(|file| score_file(&file, &terms).map(|score| (score, file)))
        .collect::<Vec<_>>();
    scored.sort_by(|(left_score, left), (right_score, right)| {
        right_score.cmp(left_score).then_with(|| {
            left.relative_path
                .to_ascii_lowercase()
                .cmp(&right.relative_path.to_ascii_lowercase())
        })
    });
    scored
        .into_iter()
        .take(MAX_RESULTS)
        .map(|(_, file)| file_result(file))
        .collect()
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
    let Ok(root) = serde_json::from_str::<Value>(&content) else {
        return Vec::new();
    };
    root.get("vaults")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|vaults| vaults.iter())
        .filter_map(|(id, value)| {
            let path = value
                .get("path")
                .and_then(Value::as_str)
                .map(PathBuf::from)?;
            if !path.is_dir() {
                return None;
            }
            let name = value
                .get("name")
                .and_then(Value::as_str)
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
        .map(|extension| {
            SEARCHABLE_EXTENSIONS
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        })
        .unwrap_or(false)
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

fn score_file(file: &VaultFile, terms: &[String]) -> Option<i32> {
    let stem = file
        .path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let relative = file.relative_path.to_ascii_lowercase();
    let aliases = file
        .aliases
        .iter()
        .map(|alias| alias.to_ascii_lowercase())
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
        } else if stem.starts_with(term) {
            score += 700;
        } else if joined_aliases.contains(term) {
            score += 550;
        } else if relative.contains(term) {
            score += 350;
        } else if haystack.contains(term) {
            score += 200;
        }
    }
    Some(score)
}

fn file_result(file: VaultFile) -> Value {
    let title = file
        .path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(&file.relative_path)
        .to_owned();
    let uri = open_uri_for_file(&file.vault_id, &file.relative_path);
    json!({
        "Title": title,
        "SubTitle": format!("Obsidian • {} • {}", file.vault_name, file.relative_path),
        "Score": 100,
        "JsonRPCAction": {"Method": "execute", "Parameters": [uri]}
    })
}

fn create_note_results(note_name: &str, vaults: &[Vault]) -> Vec<Value> {
    vaults
        .iter()
        .take(MAX_RESULTS)
        .map(|vault| {
            let uri = new_note_uri(&vault.id, note_name);
            json!({
                "Title": format!("Create note: {note_name}"),
                "SubTitle": format!("Obsidian • {}", vault.name),
                "Score": 200,
                "JsonRPCAction": {"Method": "execute", "Parameters": [uri]}
            })
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

fn open_uri(uri: &str) -> bool {
    #[cfg(windows)]
    {
        Command::new("cmd")
            .args(["/C", "start", "", uri])
            .spawn()
            .is_ok()
    }
    #[cfg(not(windows))]
    {
        let _ = uri;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_components_are_percent_encoded() {
        assert_eq!(encode_uri_component("Vault Notes"), "Vault%20Notes");
        assert_eq!(encode_uri_component("folder/note.md"), "folder%2Fnote.md");
        assert_eq!(
            open_uri_for_file("vault id", "folder/note.md"),
            "obsidian://open?vault=vault%20id&file=folder%2Fnote.md"
        );
    }

    #[test]
    fn score_requires_all_terms_and_prefers_exact_stem() {
        let file = VaultFile {
            vault_id: String::from("vault"),
            vault_name: String::from("Notes"),
            path: PathBuf::from("C:/Vault/Meeting notes.md"),
            relative_path: String::from("Meeting notes.md"),
            aliases: vec![String::from("sync")],
        };
        assert!(score_file(&file, &[String::from("meeting"), String::from("sync")]).is_some());
        assert!(score_file(&file, &[String::from("missing")]).is_none());
        assert!(score_file(&file, &[String::from("meeting")]).unwrap() > 100);
    }

    #[test]
    fn query_request_returns_flow_result_action() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "query",
            "params": [{"rawQuery":"ob create plan", "search":"create plan", "actionKeyword":"ob"}, {}]
        });
        assert_eq!(query_from_request(&request), "create plan");
        let response = handle_request(&serde_json::to_string(&request).unwrap());
        assert_eq!(response["id"], 7);
        assert!(response["result"].is_array());

        let results = create_note_results(
            "plan",
            &[Vault {
                id: String::from("vault"),
                name: String::from("Notes"),
                path: PathBuf::from("C:/Vault"),
            }],
        );
        assert_eq!(results[0]["JsonRPCAction"]["Method"], "execute");
        assert!(results[0]["JsonRPCAction"]["Parameters"][0]
            .as_str()
            .unwrap()
            .starts_with("obsidian://new?"));
    }

    #[test]
    fn invalid_request_is_json_rpc_error() {
        let response = handle_request("not-json");
        assert_eq!(response["error"]["code"], -32700);
    }
}
