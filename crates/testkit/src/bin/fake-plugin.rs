//! Plugin falso para probar el host (ADR-0015). Lee UNA línea JSON de stdin y, según el modo
//! (primer argumento), responde bien, mal, se cuelga o muere.
//!
//!   fake-plugin <modo> [nombre] [tipo]
// Binario de pruebas: CONSTRAINTS C3 permite unwrap/expect.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, Write};

use serde_json::{Value, json};

fn reply(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn ok(result: Value) -> Value {
    json!({"plugin_api": 1, "ok": true, "result": result})
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map_or("describe", String::as_str);
    let name = args.get(2).map_or("fake-plugin", String::as_str);
    let kind = args.get(3).map_or("validator", String::as_str);

    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    let request: Value = serde_json::from_str(line.trim()).unwrap_or(Value::Null);
    let method = request["method"].as_str().unwrap_or("");
    eprintln!("fake-plugin: modo={mode} método={method}");

    // `describe` lo contestan casi todos los modos igual.
    if method == "describe" && !matches!(mode, "crash" | "hang" | "garbage" | "huge") {
        let api = if mode == "wrong-api" { 2 } else { 1 };
        let advertised = if mode == "wrong-name" { "otro" } else { name };
        println!(
            "{}",
            json!({"plugin_api": api, "ok": true, "result": {
                "name": advertised, "version": "0.1.0", "plugin_api": api, "kind": kind }})
        );
        return;
    }

    match mode {
        "validate-ok" => reply(&ok(
            json!({"passed": true, "summary": "todo bien", "findings": []}),
        )),
        "validate-fail" => reply(&ok(json!({
            "passed": false,
            "summary": "se agregó un TODO sin issue",
            "findings": [{"severity": "medium", "file": "src/a.rs", "text": "TODO sin issue"}]
        }))),
        // Un secreto en la respuesta: el host lo tiene que redactar.
        "secret" => reply(&ok(json!({
            "passed": false,
            "summary": "clave sk-ant-api03-AbCdEfGhIjKlMnOpQrStUv encontrada",
            "findings": []
        }))),
        "transform-upper" => {
            let text = request["params"]["text"]
                .as_str()
                .unwrap_or("")
                .to_uppercase();
            reply(&ok(json!({ "text": text })));
        }
        "transform-huge" => {
            let text = request["params"]["text"].as_str().unwrap_or("x");
            reply(&ok(json!({ "text": text.repeat(500) })));
        }
        "error" => reply(&json!({"plugin_api": 1, "ok": false, "error": "no pude revisar"})),
        "wrong-api" => reply(&json!({"plugin_api": 2, "ok": true, "result": {}})),
        "garbage" => println!("esto no es JSON"),
        "huge" => {
            let mut out = std::io::stdout().lock();
            let _ = write!(out, "{{\"plugin_api\":1,\"ok\":true,\"result\":\"");
            let chunk = "x".repeat(64 * 1024);
            for _ in 0..24 {
                let _ = write!(out, "{chunk}");
            }
            let _ = writeln!(out, "\"}}");
        }
        "crash" => {
            eprintln!("fake-plugin: me muero");
            std::process::exit(134);
        }
        "hang" => loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        },
        // Sin respuesta: salida limpia sin escribir nada.
        "silent" => {}
        other => {
            eprintln!("modo desconocido: {other}");
            std::process::exit(2);
        }
    }
}
