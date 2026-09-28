#![windows_subsystem = "windows"]
use std::io::{Read, Write};
fn main() {
    let directory = std::env::current_exe().unwrap().parent().unwrap().to_owned();
    let mode = std::fs::read_to_string(directory.join("mode.txt")).unwrap_or_default();
    if !mode.trim().is_empty() { let mut request=String::new(); let _=std::io::stdin().read_to_string(&mut request); }
    match mode.trim() {
        "valid" => print!(r#"{{"protocol_version":1,"entries":[{{"id":"native-probe","title":"原生协议探针","target":{{"kind":"url","url":"https://example.com"}}}}]}}"#),
        "invalid" => print!("invalid json"),
        "flood" => { let bytes = vec![b'x'; 9 * 1024 * 1024]; let _ = std::io::stdout().write_all(&bytes); }
        "hang" => std::thread::sleep(std::time::Duration::from_secs(60)),
        _ => {
            if let Ok(path) = std::env::var("PTOOLS_TEST_MARKER") { std::fs::write(path, "launched").unwrap(); }
        }
    }
}
