use axum::{http::StatusCode, Json};
use serde::Deserialize;
use tokio::process::Command;
use std::sync::Mutex;
use std::time::Instant;
use std::collections::HashMap;

// Cache simple TTL 2s para evitar nft call en cada GET
static CACHE: Mutex<Option<HashMap<String, (Instant, serde_json::Value)>>> = Mutex::new(None);

fn cache_get(key: &str) -> Option<serde_json::Value> {
    let map = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ref m) = *map {
        if let Some((t, v)) = m.get(key) {
            if t.elapsed() < std::time::Duration::from_secs(2) {
                return Some(v.clone());
            }
        }
    }
    None
}

fn cache_set(key: &str, val: serde_json::Value) {
    let mut map = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let m = map.get_or_insert_with(HashMap::new);
    m.insert(key.to_string(), (Instant::now(), val));
}

fn cache_invalidate(key: &str) {
    let mut map = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ref mut m) = *map {
        m.remove(key);
    }
}

#[derive(Deserialize)]
pub struct NatRuleCreate {
    pub chain: String,
    pub action: Option<String>,
    pub out_interface: Option<String>,
    pub in_interface: Option<String>,
    pub src_address: Option<String>,
    pub dst_address: Option<String>,
    pub protocol: Option<String>,
    pub dport: Option<String>,
    pub to_src: Option<String>,
    pub to_dst: Option<String>,
    pub comment: Option<String>,
    pub action_suffix: Option<String>,
}

#[derive(Deserialize)]
pub struct NatRuleDelete {
    pub chain: String,
    pub handle: u64,
}

fn nft_val_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        _ => v.to_string(),
    }
}

/// GET /api/firewall/nat — lista reglas NAT (incluye hotspot/prerouting y hotspot/postrouting)
pub async fn list_nat_rules() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut all_chains: Vec<String> = Vec::new();
    let mut all_rules = Vec::new();

    // Reglas de tabla nat nativa
    if let Ok(result) = list_nft_table_rules("nat").await {
        if let Some(chains) = result.get(0).and_then(|v| v.get("chains")).and_then(|a| a.as_array()) {
            for c in chains {
                if let Some(name) = c.as_str() { all_chains.push(name.to_string()); }
            }
        }
        if let Some(rules) = result.get(1).and_then(|v| v.get("rules")).and_then(|a| a.as_array()) {
            all_rules.extend_from_slice(rules);
        }
    }

    // Reglas de hotspot (prerouting NAT, postrouting NAT)
    if let Ok(result) = list_nft_table_rules("hotspot").await {
        if let Some(rules) = result.get(1).and_then(|v| v.get("rules")).and_then(|a| a.as_array()) {
            for r in rules {
                let chain = r.get("chain").and_then(|c| c.as_str()).unwrap_or("");
                if chain == "prerouting" || chain == "postrouting" {
                    let mut r2 = r.clone();
                    r2["chain"] = serde_json::json!(format!("hotspot/{}", chain));
                    let cname = format!("hotspot/{}", chain);
                    if !all_chains.contains(&cname) { all_chains.push(cname); }
                    all_rules.push(r2);
                }
            }
        }
    }

    Ok(Json(serde_json::json!({
        "rules": all_rules,
        "chains": all_chains,
    })))
}


/// POST /api/firewall/nat — crear regla NAT
pub async fn create_nat_rule(body: Json<NatRuleCreate>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let chain = &body.chain;
    if chain != "prerouting" && chain != "postrouting" && chain != "input" && chain != "output" {
        return Err((StatusCode::BAD_REQUEST, "chain debe ser: prerouting, postrouting, input, output".into()));
    }

    let _ = Command::new("nft").args(["add", "table", "inet", "nat"]).output().await;
    let _ = Command::new("nft")
        .args(["add", "chain", "inet", "nat", "prerouting", "{ type nat hook prerouting priority 0; policy accept; }"])
        .output().await;
    let _ = Command::new("nft")
        .args(["add", "chain", "inet", "nat", "postrouting", "{ type nat hook postrouting priority 100; policy accept; }"])
        .output().await;

    let action = body.action.as_deref().unwrap_or("");
    let suffix = body.action_suffix.as_deref();
    let action_verb = if action.is_empty() { None } else { Some(action) };

    // BUG-2 FIX: construir Vec<String> directamente en vez de
    // string + split_whitespace. split_whitespace rompe comentarios
    // con espacios (ej: "comment \"mi regla\"" -> args rotos).
    let mut args: Vec<String> = vec!["add".into(), "rule".into(), "inet".into(), "nat".into(), chain.into()];
    if let Some(verb) = action_verb {
        args.push(verb.into());
    }
    if let Some(ref oif) = body.out_interface {
        args.push("oif".into()); args.push(oif.clone());
    }
    if let Some(ref iif) = body.in_interface {
        args.push("iif".into()); args.push(iif.clone());
    }
    if let Some(ref src) = body.src_address {
        args.push("ip".into()); args.push("saddr".into()); args.push(src.clone());
    }
    if let Some(ref dst) = body.dst_address {
        args.push("ip".into()); args.push("daddr".into()); args.push(dst.clone());
    }
    if let Some(ref proto) = body.protocol {
        args.push(proto.clone());
    }
    if let Some(ref dport) = body.dport {
        args.push("dport".into()); args.push(dport.clone());
    }
    if let Some(ref to_src) = body.to_src {
        args.push("snat".into()); args.push("to".into()); args.push(to_src.clone());
    }
    if let Some(ref to_dst) = body.to_dst {
        args.push("dnat".into()); args.push("to".into()); args.push(to_dst.clone());
    }
    if let Some(s) = suffix {
        args.push(s.into());
    }
    if let Some(ref comment) = body.comment {
        args.push("comment".into()); args.push(comment.clone());
    }

    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let output = Command::new("nft")
        .args(&args_ref)
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Error ejecutando nft: {}", e)))?;

    if !output.status.success() {
        return Err((StatusCode::BAD_REQUEST, format!("nft error: {}", String::from_utf8_lossy(&output.stderr))));
    }

    Ok(Json(serde_json::json!({"success": true, "rule": args.join(" ")})))
}

/// DELETE /api/firewall/nat — eliminar regla NAT (nat nativa o hotspot)
pub async fn delete_nat_rule(body: Json<NatRuleDelete>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let chain_raw = &body.chain;
    let handle = body.handle;

    // Detectar si es regla de hotspot (chain "hotspot/prerouting" -> table "hotspot", chain "prerouting")
    let (table, chain) = if let Some(stripped) = chain_raw.strip_prefix("hotspot/") {
        ("hotspot", stripped)
    } else {
        ("nat", chain_raw.as_str())
    };

    // P1: proteger reglas CRITICAS del hotspot (masquerade, redirect, drop
    // 8081) — antes delete_nat_rule las borraba sin freno y los clientes
    // perdian internet silenciosamente (misma logica que delete_filter_rule)
    let list_output = Command::new("nft").args(["-a", "list", "chain", "inet", table, chain]).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let txt = String::from_utf8_lossy(&list_output.stdout);
    let critical = ["8081", "redirect", "masquerade", "dnat", "drop", "accept", "return"];
    // P2: el "# handle N" va en la MISMA linea de la regla (nft list).
    // Antes miraba la linea i-1 (la regla ANTERIOR) => falsos positivos
    // (regla inofensiva tras un drop siempre negada) y falsos negativos
    // (podria borrar un drop si la anterior era inocua).
    let is_critical = txt.lines()
        .find(|l| l.contains(&format!("# handle {}", handle)))
        .map(|rule| critical.iter().any(|c| rule.contains(c)))
        .unwrap_or(true); // si no se puede leer la regla, negar por seguridad
    if is_critical {
        return Err((StatusCode::BAD_REQUEST,
            "regla protegida (critica del firewall) — borrado negado".into()));
    }

    let output = Command::new("nft")
        .args(["delete", "rule", "inet", table, chain, "handle", &handle.to_string()])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Error ejecutando nft: {}\n", e)))?;

    if !output.status.success() {
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&output.stderr).to_string()));
    }

    Ok(Json(serde_json::json!({"success": true})))
}

/// Helper: lista reglas de cualquier tabla nftables
async fn list_nft_table_rules(table: &str) -> Result<Vec<serde_json::Value>, String> {
    let output = Command::new("nft")
        .args(["-j", "list", "table", "inet", table])
        .output().await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let data: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Error parseando nft: {}", e))?;

    let mut chains = Vec::new();
    let mut rules = Vec::new();

    if let Some(nftables) = data.get("nftables").and_then(|a| a.as_array()) {
        for entry in nftables {
            if let Some(ch) = entry.get("chain") {
                if let Some(name) = ch.get("name").and_then(|v| v.as_str()) {
                    chains.push(name.to_string());
                }
            }
            if let Some(r) = entry.get("rule") {
                let chain = r.get("chain").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let handle = r.get("handle").and_then(|v| v.as_u64()).unwrap_or(0);
                // Obtener expr_text simplificado del expr
                let expr_text = if let Some(expr) = r.get("expr").and_then(|a| a.as_array()) {
                    let parts: Vec<String> = expr.iter().filter_map(|e| {
                        let eobj = e.as_object()?;
                        // match
                        if let Some(m_) = eobj.get("match") {
                            let right = m_.get("right").map(|v| nft_val_to_string(v)).unwrap_or_default();
                            let left = m_.get("left").and_then(|l| l.as_object());
                            let payload = left.and_then(|l| l.get("payload")).and_then(|p| p.as_object());
                            let proto = payload.and_then(|p| p.get("protocol")).and_then(|v| v.as_str()).unwrap_or("");
                            let field = payload.and_then(|p| p.get("field")).and_then(|v| v.as_str()).unwrap_or("");
                            if !proto.is_empty() && right.is_empty() {
                                return Some(format!("{} {}", proto, field));
                            } else if !proto.is_empty() {
                                return Some(format!("{} {} {}", proto, field, right));
                            }
                            return Some(right);
                        }
                        if let Some(oif) = eobj.get("oif") {
                            return Some(format!("oif \"{}\"", oif.get("name").and_then(|v| v.as_str()).unwrap_or("")));
                        }
                        if let Some(iif) = eobj.get("iif") {
                            return Some(format!("iif \"{}\"", iif.get("name").and_then(|v| v.as_str()).unwrap_or("")));
                        }
                        if let Some(v) = eobj.get("accept") { return Some("accept".into()); }
                        if let Some(v) = eobj.get("drop") { return Some("drop".into()); }
                        if let Some(v) = eobj.get("reject") { return Some("reject".into()); }
                        if let Some(v) = eobj.get("log") { return Some("log".into()); }
                        if let Some(v) = eobj.get("counter") {
                            let pkts = v.get("packets").and_then(|p| p.as_u64()).unwrap_or(0);
                            let bytes = v.get("bytes").and_then(|p| p.as_u64()).unwrap_or(0);
                            return Some(format!("counter {} pkts {} bytes", pkts, bytes));
                        }
                        if let Some(v) = eobj.get("limit") {
                            let rate = v.get("rate").and_then(|p| p.as_str()).unwrap_or("");
                            return Some(format!("limit {}", rate));
                        }
                        None
                    }).collect();
                    parts.join(" ")
                } else {
                    String::new()
                };
                rules.push(serde_json::json!({
                    "chain": chain,
                    "handle": handle,
                    "expr_text": expr_text,
                }));
            }
        }
    }

    // Formato con chains + rules
    Ok(vec![
        serde_json::json!({"chains": chains}),
        serde_json::json!({"rules": rules}),
    ])
}

/// GET /api/firewall/filter — reglas de filter + hotspot/forward
pub async fn list_filter_rules() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut all_rules = Vec::new();
    let mut all_chains: Vec<String> = Vec::new();

    // Obtener reglas de tabla filter nativa (si existe)
    if let Ok(result) = list_nft_table_rules("filter").await {
        if let Some(chains) = result.get(0).and_then(|v| v.get("chains")).and_then(|a| a.as_array()) {
            for c in chains {
                if let Some(name) = c.as_str() { all_chains.push(name.to_string()); }
            }
        }
        if let Some(rules) = result.get(1).and_then(|v| v.get("rules")).and_then(|a| a.as_array()) {
            all_rules.extend_from_slice(rules);
        }
    }

    // Obtener reglas de hotspot/forward (aislamiento real)
    if let Ok(result) = list_nft_table_rules("hotspot").await {
        if let Some(chains) = result.get(0).and_then(|v| v.get("chains")).and_then(|a| a.as_array()) {
            for c in chains {
                if let Some(name) = c.as_str() {
                    let full = format!("hotspot/{}", name);
                    if !all_chains.contains(&full) { all_chains.push(full); }
                }
            }
        }
        if let Some(rules) = result.get(1).and_then(|v| v.get("rules")).and_then(|a| a.as_array()) {
            let mut hotspot_rules: Vec<serde_json::Value> = rules.iter().map(|r| {
                let mut r2 = r.clone();
                if let Some(chain) = r2.get("chain").and_then(|c| c.as_str()) {
                    r2["chain"] = serde_json::json!(format!("hotspot/{}", chain));
                }
                r2
            }).collect();
            all_rules.extend(hotspot_rules);
        }
    }

    Ok(Json(serde_json::json!({"chains": all_chains, "rules": all_rules})))
}

/// P2: tokeniza respetando comillas dobles — split_whitespace rompia
/// reglas con comentarios ("comment \"mi regla\"") en 3 args invalidos.
fn tokenize_nft_rule(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if in_quote {
                    in_quote = false;
                } else {
                    in_quote = true;
                    if !cur.is_empty() { out.push(std::mem::take(&mut cur)); }
                }
            }
            ' ' | '\t' if !in_quote => {
                if !cur.is_empty() { out.push(std::mem::take(&mut cur)); }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() { out.push(cur); }
    out
}

/// POST /api/firewall/rule — crear regla en cualquier tabla nftables
pub async fn create_nft_rule(Json(body): Json<serde_json::Value>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let table = body["table"].as_str().ok_or((StatusCode::BAD_REQUEST, "table required".to_string()))?;
    let chain = body["chain"].as_str().ok_or((StatusCode::BAD_REQUEST, "chain required".to_string()))?;
    let rule_str = body["rule"].as_str().ok_or((StatusCode::BAD_REQUEST, "rule required".to_string()))?;
    // P2: validar table/chain — antes cualquier string iba a `nft add rule inet
    // <table> <chain>` (tabla inexistente = error tonto; nombres con ; o \n =
    // confusión). Solo se permiten las tablas/estilos conocidos.
    if !["hotspot", "mwan", "filter"].contains(&table) {
        return Err((StatusCode::BAD_REQUEST, format!("table invalida: {} (solo hotspot/mwan/filter)", table)));
    }
    if chain.is_empty() || !chain.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        return Err((StatusCode::BAD_REQUEST, format!("chain invalida: {}", chain)));
    }
    if rule_str.is_empty() || rule_str.chars().any(|c| c.is_control()) {
        return Err((StatusCode::BAD_REQUEST, "rule invalida (vacía o con caracteres de control)".into()));
    }
    let position = body.get("position").and_then(|v| v.as_str()).unwrap_or("add");
    // P2: tokenizer respeta comillas (comentarios con espacios) — antes
    // split_whitespace rompia la regla en args invalidos
    let mut args = vec!["nft".to_string()];
    args.push(if position == "insert" { "insert".into() } else { "add".into() });
    args.push("rule".into());
    args.push("inet".into());
    args.push(table.to_string());
    args.push(chain.to_string());
    args.extend(tokenize_nft_rule(rule_str));
    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let output = Command::new("nft")
        .args(&args_ref[1..]) // saltar "nft" (Command ya lo añade)
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {}", e)))?;
    if !output.status.success() {
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Ok(Json(serde_json::json!({"success": true})))
}

/// POST /api/firewall/filter/delete — eliminar regla filter
pub async fn delete_filter_rule(Json(body): Json<serde_json::Value>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let chain_raw = body["chain"].as_str().ok_or((StatusCode::BAD_REQUEST, "chain required".to_string()))?;
    let handle = body["handle"].as_u64().ok_or((StatusCode::BAD_REQUEST, "handle required".to_string()))?;
    let (table, chain) = if let Some(stripped) = chain_raw.strip_prefix("hotspot/") {
        ("hotspot", stripped)
    } else {
        ("filter", chain_raw)
    };
    // P0: proteger reglas CRITICAS del hotspot (drop 8081, redirect 80,
    // aislamiento, masquerade, drop final) — obtener el texto de la regla
    // por su handle y negar el borrado si es critica
    let list_output = Command::new("nft").args(["-a", "list", "chain", "inet", table, chain]).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let txt = String::from_utf8_lossy(&list_output.stdout);
    let critical = ["8081", "redirect", "masquerade", "drop", "accept", "return"];
    // P2: handle en la MISMA linea (ver delete_nat_rule)
    let is_critical = txt.lines()
        .find(|l| l.contains(&format!("# handle {}", handle)))
        .map(|rule| critical.iter().any(|c| rule.contains(c)))
        .unwrap_or(true); // si no se puede leer la regla, negar por seguridad
    if is_critical {
        return Err((StatusCode::BAD_REQUEST,
            "regla protegida (critica del firewall) — borrado negado".into()));
    }
    let output = Command::new("nft")
        .args(["delete", "rule", "inet", table, chain, "handle", &handle.to_string()])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {}", e)))?;
    if !output.status.success() {
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Ok(Json(serde_json::json!({"success": true})))
}

/// POST /api/firewall/rule/move - mover regla arriba/abajo
pub async fn move_nft_rule(Json(body): Json<serde_json::Value>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let table = body["table"].as_str().ok_or((StatusCode::BAD_REQUEST, "table required".to_string()))?;
    let chain = body["chain"].as_str().ok_or((StatusCode::BAD_REQUEST, "chain required".to_string()))?;
    let handle = body["handle"].as_u64().ok_or((StatusCode::BAD_REQUEST, "handle required".to_string()))?;
    let dir = body["direction"].as_str().ok_or((StatusCode::BAD_REQUEST, "direction required".to_string()))?;

    let out = Command::new("nft").args(["-j", "list", "chain", "inet", table, chain]).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !out.status.success() { return Err((StatusCode::BAD_REQUEST, "chain not found".to_string())); }
    let data: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("parse: {}", e)))?;
    let mut handles: Vec<u64> = Vec::new();
    if let Some(arr) = data.get("nftables").and_then(|a| a.as_array()) {
        for entry in arr {
            if let Some(r) = entry.get("rule") {
                if let Some(h) = r.get("handle").and_then(|v| v.as_u64()) { handles.push(h); }
            }
        }
    }
    let pos = handles.iter().position(|h| *h == handle).ok_or((StatusCode::NOT_FOUND, "handle not found".to_string()))?;
    if dir == "up" && pos == 0 { return Ok(Json(serde_json::json!({"success":true,"message":"already first"}))); }
    if dir == "down" && pos >= handles.len()-1 { return Ok(Json(serde_json::json!({"success":true,"message":"already last"}))); }
    let tgt = if dir == "up" { pos - 1 } else { pos + 1 };

    let list_output = Command::new("nft").args(["-a","list","chain","inet",table,chain]).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let txt = String::from_utf8_lossy(&list_output.stdout);
    // P2: el texto de la regla y su "# handle N" estan en la MISMA linea
    // (antes cogia la linea i-1 = la regla ANTERIOR -> movia la regla
    // equivocada o "could not extract rule")
    let rule_text = txt.lines()
        .find(|l| l.contains(&format!("# handle {}", handle)))
        .map(|l| l.trim().trim_end_matches(&format!("# handle {}", handle)).trim().to_string())
        .unwrap_or_default();
    if rule_text.is_empty() { return Err((StatusCode::INTERNAL_SERVER_ERROR, "could not extract rule".to_string())); }

    // P0: verificar el delete y el re-add — antes con errores ignorados la
    // regla se perdia silenciosamente si el re-add fallaba
    let del_out = Command::new("nft").args(["delete","rule","inet",table,chain,"handle",&handle.to_string()]).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !del_out.status.success() {
        return Err((StatusCode::BAD_REQUEST, format!("delete fallo: {}", String::from_utf8_lossy(&del_out.stderr).trim())));
    }
    let add_args: Vec<String> = if dir == "up" {
        if tgt == 0 {
            vec!["insert".into(), "rule".into(), "inet".into(), table.into(), chain.into(), rule_text.clone()]
        } else {
            vec!["add".into(), "rule".into(), "inet".into(), table.into(), chain.into(), "position".into(), handles[tgt-1].to_string(), rule_text.clone()]
        }
    } else {
        vec!["add".into(), "rule".into(), "inet".into(), table.into(), chain.into(), "position".into(), handles[tgt].to_string(), rule_text.clone()]
    };
    let add_out = Command::new("nft").args(&add_args).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !add_out.status.success() {
        return Err((StatusCode::INTERNAL_SERVER_ERROR,
            format!("re-add fallo — regla {} ELIMINADA y NO restaurada: {}", handle, String::from_utf8_lossy(&add_out.stderr).trim())));
    }
    Ok(Json(serde_json::json!({"success": true})))
}

/// POST /api/firewall/rule/move-to — mover regla a posicion exacta (multi-paso)
pub async fn move_nft_rule_to(Json(body): Json<serde_json::Value>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let table = body["table"].as_str().ok_or((StatusCode::BAD_REQUEST, "table required".to_string()))?;
    let chain = body["chain"].as_str().ok_or((StatusCode::BAD_REQUEST, "chain required".to_string()))?;
    let handle = body["handle"].as_u64().ok_or((StatusCode::BAD_REQUEST, "handle required".to_string()))?;
    let target_idx = body["target_index"].as_u64().ok_or((StatusCode::BAD_REQUEST, "target_index required".to_string()))?;

    let out = Command::new("nft").args(["-j", "list", "chain", "inet", table, chain]).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !out.status.success() { return Err((StatusCode::BAD_REQUEST, "chain not found".to_string())); }
    let data: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("parse: {}", e)))?;
    let mut handles: Vec<u64> = Vec::new();
    if let Some(arr) = data.get("nftables").and_then(|a| a.as_array()) {
        for entry in arr {
            if let Some(r) = entry.get("rule") {
                if let Some(h) = r.get("handle").and_then(|v| v.as_u64()) { handles.push(h); }
            }
        }
    }
    let from_idx = handles.iter().position(|h| *h == handle)
        .ok_or((StatusCode::NOT_FOUND, "handle not found".to_string()))?;
    let to_idx = target_idx as usize;
    if to_idx >= handles.len() { return Err((StatusCode::BAD_REQUEST, "target_index out of range".to_string())); }
    if from_idx == to_idx { return Ok(Json(serde_json::json!({"success":true,"message":"same position"}))); }

    // Extraer texto de la regla (misma linea que "# handle N", ver move_nft_rule)
    let list_out = Command::new("nft").args(["-a","list","chain","inet",table,chain]).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let txt = String::from_utf8_lossy(&list_out.stdout);
    let rule_text = txt.lines()
        .find(|l| l.contains(&format!("# handle {}", handle)))
        .map(|l| l.trim().trim_end_matches(&format!("# handle {}", handle)).trim().to_string())
        .unwrap_or_default();
    if rule_text.is_empty() { return Err((StatusCode::INTERNAL_SERVER_ERROR, "could not extract rule".to_string())); }

    // Eliminar original (P1: verificar status — antes `let _` y si el
    // re-add fallaba la regla se PERDIA silenciosamente)
    let del_out = Command::new("nft").args(["delete","rule","inet",table,chain,"handle",&handle.to_string()]).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !del_out.status.success() {
        return Err((StatusCode::BAD_REQUEST, format!("delete fallo: {}", String::from_utf8_lossy(&del_out.stderr).trim())));
    }

    // Insertar en posicion destino (verificar status)
    let add_out = if to_idx == 0 {
        Command::new("nft").args(["insert","rule","inet",table,chain,&rule_text]).output().await
    } else {
        let prev_handle = if to_idx <= from_idx { handles[to_idx - 1] } else { handles[to_idx] };
        Command::new("nft").args(["add","rule","inet",table,chain,"position",&prev_handle.to_string(),&rule_text]).output().await
    }.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !add_out.status.success() {
        return Err((StatusCode::INTERNAL_SERVER_ERROR,
            format!("re-add fallo — regla {} ELIMINADA y NO restaurada: {}", handle, String::from_utf8_lossy(&add_out.stderr).trim())));
    }
    Ok(Json(serde_json::json!({"success": true})))
}

/// GET /api/firewall/mangle — reglas de mangle table (incluye mwan/prerouting)
pub async fn list_mangle_rules() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut all_rules = Vec::new();
    let mut all_chains: Vec<String> = Vec::new();

    if let Ok(result) = list_nft_table_rules("mangle").await {
        if let Some(chains) = result.get(0).and_then(|v| v.get("chains")).and_then(|a| a.as_array()) {
            for c in chains { if let Some(name) = c.as_str() { all_chains.push(name.to_string()); } }
        }
        if let Some(rules) = result.get(1).and_then(|v| v.get("rules")).and_then(|a| a.as_array()) {
            all_rules.extend_from_slice(rules);
        }
    }

    // Reglas de mwan (balanceo)
    if let Ok(result) = list_nft_table_rules("mwan").await {
        if let Some(rules) = result.get(1).and_then(|v| v.get("rules")).and_then(|a| a.as_array()) {
            for r in rules {
                let mut r2 = r.clone();
                r2["chain"] = serde_json::json!(format!("mwan/{}", r.get("chain").and_then(|c| c.as_str()).unwrap_or("")));
                let cname = format!("mwan/{}", r.get("chain").and_then(|c| c.as_str()).unwrap_or(""));
                if !all_chains.contains(&cname) { all_chains.push(cname); }
                all_rules.push(r2);
            }
        }
    }

    Ok(Json(serde_json::json!({"chains": all_chains, "rules": all_rules})))
}

/// GET /api/firewall/sets — lista sets de nftables
pub async fn list_nft_sets() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let output = Command::new("nft")
        .args(["-j", "list", "sets", "inet"])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !output.status.success() {
        return Ok(Json(serde_json::json!({"sets": []})));
    }
    let data: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Error parseando nft: {}", e)))?;

    let mut sets = Vec::new();
    if let Some(nftables) = data.get("nftables").and_then(|a| a.as_array()) {
        for entry in nftables {
            if let Some(s) = entry.get("set") {
                sets.push(serde_json::json!({
                    "name": s.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "type": s.get("type").and_then(|v| v.as_str()).unwrap_or(""),
                    "elements": s.get("elem").or_else(|| s.get("elements")),
                }));
            }
        }
    }
    Ok(Json(serde_json::json!({"sets": sets})))
}

/// GET /api/firewall/conntrack — estado de conntrack
pub async fn conntrack_status() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // FIX 2026-08-08: Alpine conntrack-tools NO soporta `-o json`
    // ("Bad parameter `json`") => conntrack -L -o json devolvia [] SIEMPRE
    // (dashboard roto). Usar `conntrack -L` (texto): misma columna que
    // /proc/net/nf_conntrack. /proc/net/nf_conntrack NO existe en este
    // Alpine (solo nf_conntrack_count).
    let output = Command::new("conntrack")
        .args(["-L"])
        .output().await;
    let raw = match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).to_string(),
        _ => String::new(),
    };
    // Fallback: /proc/net/nf_conntrack si el binario conntrack no existe
    let raw = if raw.is_empty() {
        std::fs::read_to_string("/proc/net/nf_conntrack").unwrap_or_default()
    } else { raw };
    let mut count = 0u64;
    let mut by_state: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for line in raw.lines() {
        // Formato: tcp 6 119 TIME_WAIT src=... dst=...
        // estado = 4to campo (index 4 en el formato /proc: ipv4 2 tcp 6 431999 EST;
        // en conntrack -L texto: tcp 6 119 TIME_WAIT -> index 3)
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() >= 4 {
            *by_state.entry(f[3].to_string()).or_insert(0) += 1;
        }
        count += 1;
    }
    Ok(Json(serde_json::json!({
        "total": count,
        "by_state": by_state,
        "entries": [],
    })))
}
