use std::io::{self, BufRead, Write};

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let request = serde_json::from_str::<serde_json::Value>(&line).unwrap_or_default();
        let id = request
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let method = request
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let response = match method {
            "query" => serde_json::json!({
                "id": id,
                "result": [{
                    "Title": "Native Flow fixture",
                    "SubTitle": "Executable_V2 JSON-RPC plugin",
                    "Score": 100,
                    "JsonRPCAction": {
                        "Method": "execute",
                        "Parameters": ["fixture"]
                    }
                }]
            }),
            "execute" => serde_json::json!({"id": id, "result": {"hide": true}}),
            _ => serde_json::json!({"id": id, "result": []}),
        };
        let _ = writeln!(stdout, "{response}");
        let _ = stdout.flush();
    }
}
