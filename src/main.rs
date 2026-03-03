use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write, copy};
use std::path::{Path, PathBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::accept_async;
use futures_util::{StreamExt, SinkExt};
use zip;
use zip::write::FileOptions;
use chrono::Utc;
use encoding_rs::GBK;

fn read_gbk_file<P: AsRef<Path>>(path: P) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("Error reading file: {}", e))?;
    let (cow, _, _) = GBK.decode(&bytes);
    Ok(cow.into_owned())
}

fn extract_and_find_txt(zip_path: &str) -> Result<String, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    let temp_dir = std::env::temp_dir().join(format!("sch_project_{}", Utc::now().timestamp()));
    std::fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;

    let mut possible_txt_files = Vec::new();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let outpath = match file.enclosed_name() {
            Some(path) => temp_dir.join(path),
            None => continue,
        };

        if (*file.name()).ends_with('/') {
            std::fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() { std::fs::create_dir_all(&p).map_err(|e| e.to_string())?; }
            }
            let mut outfile = std::fs::File::create(&outpath).map_err(|e| e.to_string())?;
            copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;

            let file_name = outpath.file_name().unwrap_or_default().to_string_lossy();
            let path_str = outpath.to_string_lossy().to_string();
            
            if path_str.to_lowercase().ends_with(".txt") && !file_name.starts_with("._") && !path_str.contains("__MACOSX") {
                let depth = file.name().matches('/').count() + file.name().matches('\\').count();
                possible_txt_files.push((depth, path_str));
            }
        }
    }

    if possible_txt_files.is_empty() { return Err("No .txt found".to_string()); }
    possible_txt_files.sort_by_key(|k| k.0);
    Ok(possible_txt_files[0].1.clone())
}

#[derive(Debug, Default, Serialize)]
struct SchData {
    parts: HashMap<String, HashMap<String, String>>,
    nets: HashMap<String, Vec<String>>,
    lines: Vec<Vec<f64>>, // 存储所有的图形布线和电气连线坐标
}

/// 强壮的原理图解析器
fn parse_sch_txt_content(content: &str) -> SchData {
    let mut data = SchData::default();
    let mut current_section = "";
    let mut current_part: Option<String> = None;
    let mut current_net: Option<String> = None;
    let mut current_line_points: Vec<f64> = Vec::new();

    // 辅助闭包：将收集到的折点输出为独立线段
    let mut flush_lines = |points: &mut Vec<f64>, lines: &mut Vec<Vec<f64>>| {
        if points.len() >= 4 {
            for i in (0..points.len()-2).step_by(2) {
                lines.push(vec![points[i], points[i+1], points[i+2], points[i+3]]);
            }
        }
        points.clear();
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        
        // --- 1. 识别并切换大段落 ---
        if line.starts_with("*PART*") {
            current_section = "PART"; current_part = None; continue;
        } else if line.starts_with("*NET*") {
            current_section = "NET"; current_part = None; continue;
        } else if line.starts_with("*CONNECTION*") { // 新增：识别走线网络段
            current_section = "CONNECTION"; current_part = None; continue;
        } else if line.starts_with("*SCH*") {
            current_section = "SCH"; current_part = None; continue;
        } else if line.starts_with("*LINES*") {
            current_section = "LINES"; current_part = None; continue;
        } else if line.starts_with("*") && !line.starts_with("*SIGNAL*") {
            current_section = "IGNORE"; current_part = None; continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() { continue; }

        // 判断当前行是不是纯坐标系
        let is_coord = parts.len() >= 2 && 
            (parts[0].parse::<f64>().is_ok() || (parts[0].starts_with('-') && parts[0].len() > 1 && parts[0][1..].parse::<f64>().is_ok()));

        // --- 2. 处理纯图形线段 (边框等) ---
        if current_section == "LINES" {
            if is_coord {
                if let (Ok(x), Ok(y)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                    current_line_points.push(x);
                    current_line_points.push(y);
                }
            } else {
                flush_lines(&mut current_line_points, &mut data.lines);
            }
            continue; 
        }

        // --- 3. 【核心修正】处理真实的电气走线与逻辑网络 ---
        if current_section == "CONNECTION" || current_section == "NET" {
            if line.starts_with("*SIGNAL* ") {
                flush_lines(&mut current_line_points, &mut data.lines);
                if parts.len() >= 2 {
                    let net_name = parts[1].to_string();
                    current_net = Some(net_name.clone());
                    data.nets.entry(net_name).or_default();
                }
            } else if !line.starts_with("*") {
                if is_coord {
                    if let (Ok(x), Ok(y)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                        current_line_points.push(x);
                        current_line_points.push(y);
                    }
                } else {
                    // 遇到连接描述符 (例如: C18.2 @@@D0 2 0)，先把上一段走线刷入
                    flush_lines(&mut current_line_points, &mut data.lines);

                    // 解析引脚存入逻辑网络
                    if let Some(ref net_name) = current_net {
                        let net_pins = data.nets.get_mut(net_name).unwrap();
                        for i in 0..=1 {
                            if parts.len() > i && parts[i] != "OPEN" && !parts[i].starts_with("@@@") {
                                let p = parts[i].to_string();
                                if !net_pins.contains(&p) { net_pins.push(p); }
                            }
                        }
                    }
                }
            }
            continue;
        }

        // --- 4. 识别干扰指令，及时清除当前器件上下文 ---
        if line.starts_with("TEXT") || line.starts_with("OPEN") || line.starts_with("LINE") || line.starts_with("BORDER") {
            current_part = None; continue;
        }

        // --- 5. 提取器件基本信息 ---
        if line.starts_with("PART ") {
            let p = line["PART ".len()..].trim().to_string();
            current_part = Some(p.clone());
            data.parts.entry(p).or_default();
            continue;
        }

        if let Some(ref p_name) = current_part {
            if line.starts_with('"') {
                if let Some(end_quote_idx) = line[1..].find('"') {
                    let key = &line[1..=end_quote_idx];
                    let value = line[end_quote_idx + 2..].trim().trim_matches('"');
                    data.parts.get_mut(p_name).unwrap().insert(key.to_string(), value.to_string());
                }
                continue; 
            }
            if is_coord {
                let entry = data.parts.get_mut(p_name).unwrap();
                if !entry.contains_key("x") {
                    entry.insert("x".to_string(), parts[0].to_string());
                    entry.insert("y".to_string(), parts[1].to_string());
                }
                continue;
            }
        }

        // 器件列表初步定义段
        if current_section == "PART" {
            let first_char = parts[0].chars().next().unwrap_or(' ');
            if first_char.is_ascii_alphabetic() && parts.len() >= 2 && !line.starts_with("PART ") {
                let designator = parts[0].to_string();
                current_part = Some(designator.clone());
                let entry = data.parts.entry(designator).or_default();
                let device_and_fp: Vec<&str> = parts[1].split('@').collect();
                if !device_and_fp.is_empty() { entry.insert("Device".to_string(), device_and_fp[0].to_string()); }
                if device_and_fp.len() > 1 { entry.insert("Footprint".to_string(), device_and_fp[1].to_string()); }
            }
        }
    }

    // 收尾清空连线
    flush_lines(&mut current_line_points, &mut data.lines);

    // 清理垃圾数据
    data.parts.retain(|k, v| {
        if k.contains('"') || k.contains('-') { return false; } 
        v.contains_key("x") || v.contains_key("Device") 
    });

    data
}

fn handle_get_full_data(args: &Value) -> Value {
    let mut file_path = args.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
    if file_path.starts_with("file://") { file_path = &file_path[7..]; }
    
    let mut actual_path = file_path.to_string();
    if actual_path.to_lowercase().ends_with(".zip") {
        match extract_and_find_txt(&actual_path) {
            Ok(extracted) => actual_path = extracted,
            Err(e) => return json!([{"type": "text", "text": format!("Error extracting ZIP: {}", e)}]),
        }
    }

    match read_gbk_file(&actual_path) { 
        Ok(content) => {
            let sch_data = parse_sch_txt_content(&content);
            let mut parts = HashMap::new();
            for (designator, attrs) in sch_data.parts {
                parts.insert(designator, json!({
                    "Device": attrs.get("Device").cloned().unwrap_or_default(),
                    "Footprint": attrs.get("Footprint").cloned().unwrap_or_default(),
                    "x": attrs.get("x").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0),
                    "y": attrs.get("y").and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0)
                }));
            }
            json!({ "parts": parts, "nets": sch_data.nets, "lines": sch_data.lines, "actual_path": actual_path })
        },
        Err(e) => json!({"error": format!("Read error: {}", e)})
    }
}

/// 处理更新组件的请求
fn handle_update_component(args: &Value) -> Value {
    let file_path = args.get("file_path").unwrap().as_str().unwrap();
    let old_id = args.get("old_id").unwrap().as_str().unwrap();
    let new_id = args.get("new_id").unwrap().as_str().unwrap();
    let new_device = args.get("new_device").unwrap().as_str().unwrap();

    // 1. 读取并解析当前文件内容到内存结构 SchData
    let content = read_gbk_file(file_path).unwrap();
    let mut data = parse_sch_txt_content(&content);

    // 2. 在内存中修改数据
    if let Some(attr) = data.parts.remove(old_id) {
        let mut new_attr = attr;
        new_attr.insert("Device".to_string(), new_device.to_string());
        data.parts.insert(new_id.to_string(), new_attr);
    }

    // 3. 将 SchData 格式化为字符串
    let new_content_str = format_sch_content(&data);

    // 4. 【重要】使用 encoding_rs 将字符串转回 GBK 字节流并写入文件
    let (gbk_bytes, _, _) = encoding_rs::GBK.encode(&new_content_str);
    std::fs::write(file_path, gbk_bytes).unwrap();

    json!([{"type": "text", "text": "Update Success"}])
}

/// 处理更新组件位置的请求
fn handle_update_position(args: &Value) -> Value {
    let file_path = args.get("file_path").unwrap().as_str().unwrap();
    let component_id = args.get("component_id").unwrap().as_str().unwrap();
    let new_x = args.get("new_x").unwrap().as_f64().unwrap();
    let new_y = args.get("new_y").unwrap().as_f64().unwrap();

    // 1. 读取并解析当前文件内容到内存结构 SchData
    let content = read_gbk_file(file_path).unwrap();
    let mut data = parse_sch_txt_content(&content);

    // 2. 在内存中修改组件坐标
    if let Some(attrs) = data.parts.get_mut(component_id) {
        attrs.insert("x".to_string(), new_x.to_string());
        attrs.insert("y".to_string(), new_y.to_string());
    }

    // 3. 将 SchData 格式化为字符串
    let new_content_str = format_sch_content(&data);

    // 4. 【重要】使用 encoding_rs 将字符串转回 GBK 字节流并写入文件
    let (gbk_bytes, _, _) = encoding_rs::GBK.encode(&new_content_str);
    std::fs::write(file_path, gbk_bytes).unwrap();

    json!([{"type": "text", "text": "Position Update Success"}])
}

/// 【新增】处理保存回 ZIP 或 TXT 的请求
fn handle_save_file(args: &Value) -> Value {
    let original_path = args.get("original_path").and_then(|v| v.as_str()).unwrap_or("");
    let modified_txt_path = args.get("modified_txt_path").and_then(|v| v.as_str()).unwrap_or("");

    if original_path.to_lowercase().ends_with(".zip") {
        // 1. 读取修改后的 txt 内容
        let modified_bytes = fs::read(modified_txt_path).unwrap_or_default();

        // 2. 打开原始 zip
        let original_file = match fs::File::open(original_path) {
            Ok(f) => f,
            Err(e) => return json!([{"type": "text", "text": format!("Error opening zip: {}", e)}]),
        };
        let mut archive = zip::ZipArchive::new(original_file).unwrap();

        // 3. 创建临时 zip
        let temp_zip_path = format!("{}.tmp", original_path);
        let temp_zip_file = fs::File::create(&temp_zip_path).unwrap();
        let mut zip_writer = zip::ZipWriter::new(temp_zip_file);

        // 4. 遍历并拷贝，如果是被修改的 txt 则替换内容
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).unwrap();
            let options = FileOptions::default()
                .compression_method(file.compression())
                .unix_permissions(file.unix_mode().unwrap_or(0o755));

            let file_name = file.name().to_string();
            zip_writer.start_file(&file_name, options).unwrap();

            // 判断当前文件是否是我们修改过的那个 txt (通过路径后缀匹配)
            let is_modified_file = if let Some(enclosed) = file.enclosed_name() {
                let enclosed_str = enclosed.to_string_lossy().to_string();
                // 统一路径分隔符进行比对
                modified_txt_path.replace('\\', "/").ends_with(&enclosed_str.replace('\\', "/"))
            } else {
                false
            };

            if is_modified_file {
                zip_writer.write_all(&modified_bytes).unwrap();
            } else {
                std::io::copy(&mut file, &mut zip_writer).unwrap();
            }
        }
        zip_writer.finish().unwrap();

        // 5. 将临时 zip 覆盖掉原始 zip
        if let Err(e) = fs::rename(&temp_zip_path, original_path) {
            return json!([{"type": "text", "text": format!("Error saving zip: {}", e)}]);
        }

        json!([{"type": "text", "text": "Successfully saved to ZIP"}])
    } else {
        // 如果是纯 TXT 文件，在 update_component 等操作时已经实时写入了，无需额外操作
        json!([{"type": "text", "text": "TXT file is already up-to-date"}])
    }
}

/// 【新增】清空所有网络
fn handle_clear_all_nets(args: &Value) -> Value {
    let file_path = args.get("file_path").unwrap().as_str().unwrap();

    // 1. 读取并解析
    let content = read_gbk_file(file_path).unwrap();
    let mut data = parse_sch_txt_content(&content);

    // 2. 清空内存中的 nets
    data.nets.clear();

    // 3. 写回文件
    let new_content_str = format_sch_content(&data);
    let (gbk_bytes, _, _) = encoding_rs::GBK.encode(&new_content_str);
    std::fs::write(file_path, gbk_bytes).unwrap();

    json!([{"type": "text", "text": "All nets cleared successfully"}])
}

/// 【新增】向特定网络添加引脚
fn handle_add_net_pin(args: &Value) -> Value {
    let file_path = args.get("file_path").unwrap().as_str().unwrap();
    let net_name = args.get("net_name").unwrap().as_str().unwrap();
    let pin = args.get("pin").unwrap().as_str().unwrap();

    // 1. 读取内容
    let content = read_gbk_file(file_path).unwrap();
    let mut data = parse_sch_txt_content(&content);

    // 2. 修改内存数据
    let pins = data.nets.entry(net_name.to_string()).or_insert(Vec::new());
    if !pins.contains(&pin.to_string()) {
        pins.push(pin.to_string());
    }

    // 3. 格式化并写回
    let new_content_str = format_sch_content(&data);
    let (gbk_bytes, _, _) = encoding_rs::GBK.encode(&new_content_str);
    std::fs::write(file_path, gbk_bytes).unwrap();

    json!([{"type": "text", "text": format!("Pin {} added to net {}", pin, net_name)}])
}

/// 将 SchData 结构格式化为 PADS Logic TXT 格式的字符串
fn format_sch_content(data: &SchData) -> String {
    // 1. 必须添加 PADS 标准文件头，否则 LCEDA 无法识别
    // 这里使用 V3.0 版本，它是兼容性最好的版本之一
    let mut result = String::from("!PADS-POWERLOGIC-V3.0-ANSIC! DESIGN-UNITS-ENGLISH\r\n\r\n");
    
    // 换行符统一使用 \r\n (Windows 标准)
    let nl = "\r\n";

    // 写入器件信息
    result.push_str(&format!("*PART*{}", nl));
    for (designator, attrs) in &data.parts {
        let device = attrs.get("Device").cloned().unwrap_or_default();
        let footprint = attrs.get("Footprint").cloned().unwrap_or_default();
        
        if !footprint.is_empty() {
            result.push_str(&format!("{} {}@{}{}", designator, device, footprint, nl));
        } else {
            result.push_str(&format!("{} {}{}", designator, device, nl));
        }
    }
    result.push_str(nl);
    
    // 写入网络连接信息
    result.push_str(&format!("*NET*{}", nl));
    for (net_name, pins) in &data.nets {
        result.push_str(&format!("*SIGNAL* {}{}", net_name, nl));
        // PADS 格式通常每行两个引脚，或者每行一个。这里为了稳妥每行一个。
        for pin in pins {
            result.push_str(&format!("{}{}", pin, nl));
        }
    }
    result.push_str(nl);
    
    // 写入位置和属性信息 (LCEDA 识别坐标的关键)
    result.push_str(&format!("*SCH*{}", nl));
    for (designator, attrs) in &data.parts {
        let x = attrs.get("x").cloned().unwrap_or_else(|| "0".to_string());
        let y = attrs.get("y").cloned().unwrap_or_else(|| "0".to_string());
        
        // 关键格式：PART 位号 坐标X 坐标Y 角度 镜像
        result.push_str(&format!("PART {} {} {} 0 0{}", designator, x, y, nl));
        
        // 写入 ATTRIBUTE
        result.push_str(&format!("\"Device\" \"{}\"{}", attrs.get("Device").cloned().unwrap_or_default(), nl));
    }
    result.push_str(nl);

    // 2. 必须以 *END* 结尾
    result.push_str(&format!("*END*{}", nl));
    
    result
}

#[tokio::main]
async fn main() {
    let addr = "127.0.0.1:8080"; 
    let listener = TcpListener::bind(addr).await.expect("Failed to bind");
    println!("MCP Server listening on ws://{}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(handle_connection(stream));
    }
}

async fn handle_connection(raw_stream: TcpStream) {
    let ws_stream = accept_async(raw_stream).await.expect("Error during ws handshake");
    let (mut writer, mut reader) = ws_stream.split();
    while let Some(msg) = reader.next().await {
        let msg = msg.expect("Error reading message");
        if msg.is_text() {
            let req: Value = serde_json::from_str(msg.to_text().unwrap()).unwrap_or(json!({}));
            let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let id = req.get("id").cloned();
            let response = match method {
                "initialize" => json!({ "jsonrpc": "2.0", "id": id, "result": { "capabilities": {"tools": {}}, "serverInfo": {"name": "sch-server", "version": "1.0.0"}} }),
                "tools/call" => {
                    let params = req.get("params").unwrap();
                    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let args = params.get("arguments").unwrap();
                    let result = match tool_name {
                        "get_full_data" => handle_get_full_data(args),
                        "update_component" => handle_update_component(args),
                        "update_position" => handle_update_position(args),
                        "save_file" => handle_save_file(args), // 【新增】路由分发
                        "clear_all_nets" => handle_clear_all_nets(args),
                        "add_net_pin" => handle_add_net_pin(args),
                        _ => json!([{"type": "text", "text": "OK"}])
                    };
                    json!({ "jsonrpc": "2.0", "id": id, "result": { "content": result } })
                },
                _ => json!({ "jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "Method not found"} }),
            };
            writer.send(tokio_tungstenite::tungstenite::Message::Text(response.to_string())).await.unwrap();
        }
    }
}