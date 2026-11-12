//! Experimental Linux/X11 and macOS adapter. Windows uses its native backend.
//! No shell command interpolation: every external argument is passed separately.
use eframe::egui;
use std::{io::Write, path::PathBuf, process::{Child, Command, Stdio}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use crate::config::AudioSource;

struct Session { child: Child, path: PathBuf, log: PathBuf, since: Instant, stopping: bool }
struct Desktop {
    audio: AudioSource, fps: u32, message: String, session: Option<Session>,
    screen: String, microphone: String, system: String, settings: bool, closing: bool,
    shot_rx: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
}
fn root() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
        .join(if cfg!(target_os="macos") { "Movies" } else { "Videos" }).join("Zapture")
}
fn output(kind: &str, ext: &str) -> Result<PathBuf, String> {
    let dir = root().join(kind);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
    Ok(dir.join(format!("Zapture_{now}.{ext}")))
}
fn query(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program).args(args).output().map_err(|e| format!("{program}: {e}"))?;
    if !out.status.success() { return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned()); }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}
fn wayland() -> bool { cfg!(target_os="linux") && std::env::var("XDG_SESSION_TYPE").is_ok_and(|s| s=="wayland") }
fn recording_args(audio: AudioSource, fps: u32, screen: &str, mic: &str, system: &str, display: &str) -> Result<Vec<String>, String> {
    let mut args: Vec<String> = ["-hide_banner", "-loglevel", "warning", "-n"].iter().map(|s|s.to_string()).collect();
    let mut add = |a: &[&str]| args.extend(a.iter().map(|s|s.to_string()));
    let fps = fps.to_string();
    let sources: Vec<&str> = match audio { AudioSource::Off=>vec![], AudioSource::System=>vec![system], AudioSource::Mic=>vec![mic], AudioSource::Both=>vec![system,mic] };
    if cfg!(target_os="linux") {
        if display.is_empty() { return Err("No X11 display. Native Wayland capture is not implemented yet.".into()); }
        add(&["-thread_queue_size","64","-f","x11grab","-framerate",&fps,"-i",display]);
        for source in &sources { add(&["-thread_queue_size","256","-f","pulse","-i",source]); }
    } else {
        if screen.trim().is_empty() { return Err("Set the screen device index in Settings. See the device list command there.".into()); }
        add(&["-thread_queue_size","64","-f","avfoundation","-capture_cursor","1","-framerate",&fps,"-i",&format!("{screen}:none")]);
        for source in &sources {
            if source.trim().is_empty() { return Err("Select an audio device in Settings. System sound requires a loopback device in this experimental adapter.".into()); }
            add(&["-thread_queue_size","256","-f","avfoundation","-i",&format!("none:{source}")]);
        }
    }
    add(&["-map","0:v:0"]);
    if sources.len()==2 {
        add(&["-filter_complex","[1:a][2:a]amix=inputs=2:duration=longest:normalize=1[a]","-map","[a]"]);
    } else if sources.len()==1 { add(&["-map","1:a:0"]); }
    add(&["-vf","pad=ceil(iw/2)*2:ceil(ih/2)*2","-pix_fmt","yuv420p"]);
    if cfg!(target_os="macos") { add(&["-c:v","h264_videotoolbox","-b:v","6000k"]); }
    else { add(&["-c:v","libx264","-preset","ultrafast","-crf","23"]); }
    if !sources.is_empty() { add(&["-c:a","aac","-b:a","160k"]); }
    add(&["-movflags","+faststart"]);
    Ok(args)
}
impl Default for Desktop {
    fn default() -> Self {
        Self { audio: AudioSource::Off, fps:30, message:"Ready • experimental desktop adapter".into(), session:None,
            screen:String::new(), microphone:if cfg!(target_os="linux") {"default".into()} else {String::new()}, system:String::new(),
            settings:false, closing:false, shot_rx:None }
    }
}
impl Desktop {
    fn start(&mut self) -> Result<(),String> {
        if wayland() { return Err("Wayland capture is not implemented in this preview. Use an X11 desktop session. Portal/PipeWire is planned.".into()); }
        let mut system=self.system.clone();
        if cfg!(target_os="linux") && system.is_empty() && matches!(self.audio,AudioSource::System|AudioSource::Both) {
            system=format!("{}.monitor",query("pactl", &["get-default-sink"])?);
        }
        let args=recording_args(self.audio,self.fps,&self.screen,&self.microphone,&system,&std::env::var("DISPLAY").unwrap_or_default())?;
        let path=output("Recordings","mp4")?;
        let log=path.with_extension("log");
        let logfile=std::fs::File::create(&log).map_err(|e|e.to_string())?;
        let child=Command::new("ffmpeg").args(args).arg(&path).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(logfile)
            .spawn().map_err(|e|format!("FFmpeg is required for this platform preview: {e}"))?;
        self.session=Some(Session {child,path,log,since:Instant::now(),stopping:false});
        self.message="Starting capture…".into(); Ok(())
    }
    fn stop(&mut self) {
        if let Some(s)=&mut self.session {
            if !s.stopping {
                match s.child.stdin.as_mut().ok_or_else(|| "Recorder input closed".to_string()).and_then(|i|i.write_all(b"q\n").map_err(|e|e.to_string())) {
                    Ok(())=>{s.stopping=true;self.message="Saving… Please wait.".into();},
                    Err(e)=>self.message=e,
                }
            }
        }
    }
    fn poll(&mut self) {
        let status=self.session.as_mut().and_then(|s|s.child.try_wait().transpose());
        if let Some(status)=status {
            let s=self.session.take().unwrap();
            self.message=match status {
                Ok(status) if status.success()=>format!("Saved: {}",s.path.display()),
                Ok(_)=>format!("Recording failed. See {}",s.log.display()),
                Err(e)=>format!("Recorder status error: {e}"),
            };
        }
        if let Some(rx)=&self.shot_rx {
            if let Ok(result)=rx.try_recv() {
                self.message=result.unwrap_or_else(|e|e);self.shot_rx=None;
            }
        }
    }
    fn screenshot(&mut self) {
        let (tx,rx)=std::sync::mpsc::channel();self.shot_rx=Some(rx);
        self.message="Taking screenshot…".into();
        std::thread::spawn(move||{ let _=tx.send(take_shot()); });
    }
}
fn take_shot()->Result<String,String> {
    let path=output("Screenshots","png")?;
    let status=if cfg!(target_os="macos") {
        Command::new("screencapture").args(["-x","-i"]).arg(&path).status()
    } else if wayland() {
        return Err("Wayland screenshot portal is not implemented in this preview.".into());
    } else {
        Command::new("ffmpeg").args(["-hide_banner","-loglevel","error","-n","-f","x11grab","-i"])
            .arg(std::env::var("DISPLAY").map_err(|_|"No X11 display")?)
            .args(["-frames:v","1"]).arg(&path).stdin(Stdio::null()).status()
    }.map_err(|e|e.to_string())?;
    if !status.success() || !path.exists() { return Err("Screenshot cancelled or capture failed.".into()); }
    let img=image::open(&path).map_err(|e|e.to_string())?.to_rgba8();
    let (w,h)=img.dimensions();
    // X11 clipboard data belongs to the owner process; retain the handle.
    static CLIPBOARD: std::sync::OnceLock<std::sync::Mutex<Option<arboard::Clipboard>>> = std::sync::OnceLock::new();
    let mut guard=CLIPBOARD.get_or_init(||std::sync::Mutex::new(None)).lock().map_err(|_|"Clipboard lock failed")?;
    if guard.is_none() { *guard=Some(arboard::Clipboard::new().map_err(|e|e.to_string())?); }
    let copied=guard.as_mut().unwrap().set_image(arboard::ImageData{width:w as usize,height:h as usize,bytes:std::borrow::Cow::Owned(img.into_raw())});
    match copied { Ok(())=>Ok(format!("Saved + copied: {}",path.display())),Err(e)=>Ok(format!("Saved: {}. Clipboard unavailable: {e}",path.display())) }
}
impl eframe::App for Desktop {
    fn update(&mut self, ctx:&egui::Context,_:&mut eframe::Frame) {
        self.poll();
        if ctx.input(|i|i.viewport().close_requested()) && (self.session.is_some() || self.shot_rx.is_some()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);self.closing=true;self.stop();
        }
        if self.closing && self.session.is_none() && self.shot_rx.is_none(){ctx.send_viewport_cmd(egui::ViewportCommand::Close);}
        egui::CentralPanel::default().show(ctx,|ui|{
            ui.horizontal(|ui|{ui.heading("Zapture");ui.label("●");});
            ui.horizontal(|ui|{
                if ui.add_enabled(self.session.is_none() && self.shot_rx.is_none(),egui::Button::new("● Record")).clicked(){if let Err(e)=self.start(){self.message=e;}}
                if ui.add_enabled(self.session.as_ref().is_some_and(|s|!s.stopping),egui::Button::new("■ Stop")).clicked(){self.stop();}
                if ui.add_enabled(self.shot_rx.is_none(),egui::Button::new("Screenshot")).clicked(){self.screenshot();}
            });
            ui.add_enabled_ui(self.session.is_none(),|ui|{
                egui::ComboBox::from_label("Sound").selected_text(match self.audio {AudioSource::Off=>"None",AudioSource::System=>"Computer",AudioSource::Mic=>"Microphone",AudioSource::Both=>"Computer + microphone"}).show_ui(ui,|ui|{
                    for (s,label) in [(AudioSource::Off,"None"),(AudioSource::System,"Computer"),(AudioSource::Mic,"Microphone"),(AudioSource::Both,"Computer + microphone")] {ui.selectable_value(&mut self.audio,s,label);}
                });
            });
            if let Some(s)=&self.session {ui.label(format!("{} • {}s",if s.stopping{"Saving"}else{"Recording"},s.since.elapsed().as_secs()));}
            ui.small(&self.message);
            ui.horizontal(|ui|{
                if ui.button("Files").clicked(){let _=std::fs::create_dir_all(root());let _=Command::new(if cfg!(target_os="macos"){"open"}else{"xdg-open"}).arg(root()).spawn();}
                if ui.button("Settings").clicked(){self.settings=!self.settings;}
            });
            if self.settings {
                ui.separator();ui.label("Desktop preview needs FFmpeg installed.");
                ui.add_enabled_ui(self.session.is_none(),|ui|{
                    ui.add(egui::Slider::new(&mut self.fps,10..=60).text("FPS"));
                    if cfg!(target_os="macos") {
                        ui.label("Screen device index");ui.text_edit_singleline(&mut self.screen);
                        ui.small("List indices: ffmpeg -f avfoundation -list_devices true -i empty");
                        ui.small("Allow Screen Recording and Microphone in macOS privacy settings. Computer sound needs a configured loopback device (e.g. BlackHole). Native ScreenCaptureKit is planned.");
                    }
                    ui.label("Microphone device (index on macOS)");ui.text_edit_singleline(&mut self.microphone);
                    ui.label("System audio device (blank = auto on Linux)");ui.text_edit_singleline(&mut self.system);
                });
            }
        });
        if self.session.is_some() || self.shot_rx.is_some(){ctx.request_repaint_after(Duration::from_millis(250));}
    }
}
pub fn run()->eframe::Result {
    eframe::run_native("Zapture",eframe::NativeOptions{
        viewport:egui::ViewportBuilder::default().with_inner_size([360.0,260.0]).with_min_inner_size([320.0,240.0]).with_always_on_top(),
        ..Default::default()
    },Box::new(|_|Ok(Box::new(Desktop::default()))))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn both_sources_are_mixed_not_separate_tracks(){
        let args=recording_args(AudioSource::Both,30,"1","0","2",":0").unwrap();
        assert!(args.iter().any(|a|a.contains("amix=inputs=2")));
        assert!(args.iter().any(|a|a=="[a]"));
    }
    #[test] fn silent_video_has_no_aac_encoder(){
        let args=recording_args(AudioSource::Off,30,"1","","",":0").unwrap();
        assert!(!args.iter().any(|a|a=="aac"));
    }
}
