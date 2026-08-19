use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
#[cfg(windows)]
use std::process::Command;

const GOOGLE_SEARCH_URL: &str = "https://www.google.com/search?q=";

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
        return json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {"code": -32700, "message": "Invalid JSON"}
        });
    };
    let id = request.get("id").cloned().unwrap_or_else(|| json!(1));
    match request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "query" => {
            let query = query_from_request(&request);
            json!({"jsonrpc":"2.0","id":id,"result":query_results(&query)})
        }
        "execute" => {
            let url = request
                .get("params")
                .and_then(Value::as_array)
                .and_then(|params| params.first())
                .and_then(Value::as_str)
                .unwrap_or_default();
            json!({"jsonrpc":"2.0","id":id,"result":open_url(url)})
        }
        _ => json!({"jsonrpc":"2.0","id":id,"result":[]}),
    }
}

fn query_from_request(request: &Value) -> String {
    request
        .get("params")
        .and_then(Value::as_array)
        .and_then(|params| params.first())
        .and_then(|params| params.get("search"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn query_results(query: &str) -> Vec<Value> {
    if query.is_empty() {
        return Vec::new();
    }
    let url = format!("{GOOGLE_SEARCH_URL}{}", encode_uri_component(query));
    vec![json!({
        "Title": format!("Search Google: {query}"),
        "SubTitle": "Open the Google search in the default browser",
        "Score": 51,
        "JsonRPCAction": {"Method": "execute", "Parameters": [url]}
    })]
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

fn open_url(url: &str) -> bool {
    #[cfg(windows)]
    {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .is_ok()
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_query_creates_encoded_search_action() {
        let results = query_results("space exploration");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["Title"], "Search Google: space exploration");
        assert_eq!(
            results[0]["JsonRPCAction"]["Parameters"][0],
            "https://www.google.com/search?q=space%20exploration"
        );
    }

    #[test]
    fn query_request_uses_alias_stripped_search_field() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "query",
            "params": [{"rawQuery":"g space exploration", "search":"space exploration", "actionKeyword":"g"}, {}]
        });
        let response = handle_request(&serde_json::to_string(&request).unwrap());
        assert_eq!(response["id"], 9);
        assert_eq!(
            response["result"][0]["Title"],
            "Search Google: space exploration"
        );
    }

    #[test]
    fn empty_query_returns_no_results() {
        assert!(query_results("").is_empty());
    }

    #[test]
    fn invalid_json_returns_json_rpc_error() {
        let response = handle_request("not-json");
        assert_eq!(response["error"]["code"], -32700);
    }
}
