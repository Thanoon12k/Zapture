//! Local MCP stdio bridge. stdout is JSON-RPC only. No network listener or shell tool.
use serde_json::{json, Value};
use std::{io::{BufRead, Read, Write}, sync::{mpsc, Mutex, OnceLock}, time::{Duration, Instant}};
use base64::Engine;
struct Request { name:String, args:Value, reply:mpsc::Sender<Result<Value,String>>, deadline:Instant }
static INBOX:OnceLock<Mutex<mpsc::Receiver<Request>>>=OnceLock::new();
static RECORDING_PATH:OnceLock<Mutex<Option<String>>>=OnceLock::new();
pub fn set_recording_path(path:String){if let Ok(mut p)=RECORDING_PATH.get_or_init(||Mutex::new(None)).lock(){*p=Some(path);}}
pub fn recording_path()->Option<String>{RECORDING_PATH.get_or_init(||Mutex::new(None)).lock().ok()?.clone()}
static LAST_ERROR:OnceLock<Mutex<Option<String>>>=OnceLock::new();
pub fn enabled()->bool {INBOX.get().is_some()}
pub fn report_error(s:&str) {if let Ok(mut e)=LAST_ERROR.get_or_init(||Mutex::new(None)).lock(){*e=Some(s.into());}eprintln!("Zapture: {s}");}
pub fn take_error()->Option<String>{LAST_ERROR.get_or_init(||Mutex::new(None)).lock().ok()?.take()}
pub fn audio(args:&Value)->Result<crate::config::AudioSource,String>{
    crate::config::AudioSource::from_code(args.get("audio").and_then(Value::as_str).unwrap_or("off")).ok_or_else(||"audio must be off, system, mic or both".into())
}
pub fn screenshot_result(path:&std::path::Path,inline:bool)->Result<Value,String>{
    let mut content=vec![json!({"type":"text","text":json!({"path":path,"saved":true}).to_string()})];
    if inline {
        let size=std::fs::metadata(path).map_err(|e|e.to_string())?.len();
        if size<=12*1024*1024 {
            let bytes=std::fs::read(path).map_err(|e|e.to_string())?;
            content.push(json!({"type":"image","mimeType":"image/png","data":base64::engine::general_purpose::STANDARD.encode(bytes)}));
        } else {content.push(json!({"type":"text","text":"Image is larger than the 12 MiB inline limit; use the saved path."}));}
    }
    Ok(json!({"content":content,"isError":false}))
}
pub fn tools()->Value {json!({"tools":[
    {"name":"zapture_status","description":"Get capture state, output folder and platform limitations. Does not take a screenshot.","inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":{"readOnlyHint":true,"openWorldHint":false}},
    {"name":"zapture_screenshot","description":"Capture the current desktop to PNG, returning its local path and optionally an inline image. No clipboard modification. Windows uses the primary monitor; Unix uses the desktop. Requires local screen permission.","inputSchema":{"type":"object","properties":{"inline":{"type":"boolean","default":true}},"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"openWorldHint":false}},
    {"name":"zapture_start_recording","description":"Start recording with visible controls. Audio defaults to off. Call stop to finalize, then poll status until idle. Windows uses the current capture frame; Unix uses the configured screen.","inputSchema":{"type":"object","properties":{"audio":{"type":"string","enum":["off","system","mic","both"],"default":"off"}},"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"openWorldHint":false}},
    {"name":"zapture_stop_recording","description":"Request graceful recording stop. Returns saving until the file is finalized; poll zapture_status. Idempotent when idle.","inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"openWorldHint":false}}
]})}
fn validate(name:&str,args:&Value)->Result<(),String>{
    let obj=args.as_object().ok_or("arguments must be an object")?;
    let allowed=match name {"zapture_status"|"zapture_stop_recording"=>None,"zapture_screenshot"=>Some("inline"),"zapture_start_recording"=>Some("audio"),_=>return Err("Unknown tool".into())};
    if obj.keys().any(|k|Some(k.as_str())!=allowed){return Err("Unexpected argument".into());}
    if name=="zapture_screenshot" && args.get("inline").is_some_and(|v|!v.is_boolean()){return Err("inline must be boolean".into());}
    if name=="zapture_start_recording" {
        if args.get("audio").is_some_and(|v|!v.is_string()){return Err("audio must be a string".into());}
        audio(args)?;
    }
    Ok(())
}
fn route(v:Value,initialized:&mut bool,call:&mut impl FnMut(&str,Value)->Result<Value,String>)->Option<Value>{
    let id=v.get("id").cloned();
    let method=v.get("method").and_then(Value::as_str);
    if v.get("jsonrpc")!=Some(&json!("2.0")) || method.is_none() || !v.is_object(){return Some(json!({"jsonrpc":"2.0","id":id.unwrap_or(Value::Null),"error":{"code":-32600,"message":"Invalid Request"}}));}
    let id=id?; // Notifications never produce responses or invoke tools.
    let method=method.unwrap();
    let result=match method {
        "initialize"=>{*initialized=true;Ok(json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"Zapture","version":env!("CARGO_PKG_VERSION")},"instructions":"Local screen-capture tools only. Audio defaults off. Recording controls remain visible. Stop recording before disconnecting. This server does not provide mouse, keyboard, shell or arbitrary file access."}))},
        "ping"=>Ok(json!({})),
        _ if !*initialized=>Err((-32002,"Initialize first")),
        "tools/list"=>Ok(tools()),
        "tools/call"=>{
            let name=v.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
            let args=v.pointer("/params/arguments").cloned().unwrap_or(json!({}));
            match validate(name,&args).and_then(|_|call(name,args)) {
                Ok(data) if data.get("content").is_some()=>Ok(data),
                Ok(data)=>Ok(json!({"content":[{"type":"text","text":data.to_string()}],"isError":false})),
                Err(e)=>Ok(json!({"content":[{"type":"text","text":e}],"isError":true})),
            }
        },
        _=>Err((-32601,"Method not found")),
    };
    Some(match result {Ok(r)=>json!({"jsonrpc":"2.0","id":id,"result":r}),Err((code,msg))=>json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":msg}})})
}
pub fn start_if_requested(){
    if !std::env::args().any(|a|a=="--mcp"){return;}
    let (tx,rx)=mpsc::channel();let _=INBOX.set(Mutex::new(rx));
    std::thread::spawn(move||{
        let mut initialized=false;
        let mut input=std::io::stdin().lock();let mut output=std::io::stdout().lock();
        loop {
            // Bound incoming JSON lines, including malformed inputs.
            let mut line=Vec::new();
            let read=std::io::Read::by_ref(&mut input).take(1024*1024+1).read_until(b'\n',&mut line);
            match read {Ok(0)|Err(_)=>break,_=>{}}
            if line.len()>1024*1024 {break;}
            let reply=match serde_json::from_slice::<Value>(&line){
                Ok(v)=>route(v,&mut initialized,&mut |name,args|{
                    let (rtx,rrx)=mpsc::channel();
                    tx.send(Request{name:name.into(),args,reply:rtx,deadline:Instant::now()+Duration::from_secs(30)}).map_err(|_|"UI unavailable")?;
                    rrx.recv_timeout(Duration::from_secs(35)).map_err(|_|"Capture operation timed out; inspect status before retrying")?
                }),
                Err(_)=>Some(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}})),
            };
            if let Some(reply)=reply {if writeln!(output,"{reply}").and_then(|_|output.flush()).is_err(){break;}}
        }
        let (rtx,_)=mpsc::channel();let _=tx.send(Request{name:"__exit".into(),args:json!({}),reply:rtx,deadline:Instant::now()+Duration::from_secs(30)});
    });
}
pub fn drain(mut handle:impl FnMut(&str,&Value)->Result<Value,String>){
    let Some(inbox)=INBOX.get() else{return};
    loop {
        let request=inbox.lock().ok().and_then(|r|r.try_recv().ok());
        let Some(r)=request else{break};
        let result=if Instant::now()>r.deadline {Err("Request expired before execution".into())}else{handle(&r.name,&r.args)};
        let _=r.reply.send(result);
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn lifecycle_and_tools(){let mut init=false;let mut called=false;
        let mut handler=|_:&str,_:Value|{called=true;Ok(json!({"state":"idle"}))};
        assert!(route(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),&mut init,&mut handler).unwrap().get("error").is_some());
        let r=route(json!({"jsonrpc":"2.0","id":2,"method":"initialize"}),&mut init,&mut handler).unwrap();assert_eq!(r["result"]["protocolVersion"],"2025-06-18");
        assert!(route(json!({"jsonrpc":"2.0","method":"notifications/initialized"}),&mut init,&mut handler).is_none());
        let r=route(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"zapture_status"}}),&mut init,&mut handler).unwrap();assert_eq!(r["result"]["isError"],false);assert!(called);
    }
    #[test] fn rejects_shell_and_bad_args(){assert!(validate("exec",&json!({"command":"test"})).is_err());assert!(validate("zapture_screenshot",&json!({"path":"/tmp/x"})).is_err());assert!(validate("zapture_start_recording",&json!({"audio":true})).is_err());}
    #[test] fn errors_stay_in_tool_results(){let r=route(json!({"jsonrpc":"2.0","id":"x","method":"tools/call","params":{"name":"zapture_status"}}),&mut true,&mut |_,_|Err("Denied".into())).unwrap();assert_eq!(r["result"]["isError"],true);assert_eq!(r["id"],"x");}
}
