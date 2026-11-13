//! Small native settings window. Changes are applied together after validation.
use std::cell::RefCell;
use windows::core::{w,PCWSTR};
use windows::Win32::Foundation::{HWND,HINSTANCE,WPARAM,LPARAM,LRESULT,RECT};
use windows::Win32::Graphics::Gdi::{CreateFontW,DeleteObject,HFONT,DEFAULT_CHARSET,FW_NORMAL,FF_DONTCARE};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Com::{CoCreateInstance,CoTaskMemFree,CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{FileOpenDialog,IFileOpenDialog,FOS_PICKFOLDERS,FOS_PATHMUSTEXIST,SIGDN_FILESYSPATH};
use windows::Win32::UI::WindowsAndMessaging::*;
use crate::{app,config::parse_hotkey};
use crate::win::{ui,util};
const SAVE:usize=1; const CANCEL:usize=2; const BROWSE:usize=3;
struct Form { hwnd:HWND,font:HFONT,folder:HWND,quality:HWND,fps:HWND,size:HWND,dot:HWND,shot:HWND,record:HWND,keys:[HWND;4] }
thread_local! { static FORM:RefCell<Option<Form>>=const { RefCell::new(None) }; }
pub fn window()->Option<HWND>{FORM.with(|f|f.borrow().as_ref().map(|f|f.hwnd))}
unsafe fn text(h:HWND)->String {
    let mut buf=vec![0u16;GetWindowTextLengthW(h).max(0) as usize+1];
    let n=GetWindowTextW(h,&mut buf);String::from_utf16_lossy(&buf[..n.max(0) as usize])
}
unsafe fn child(h:HWND,font:HFONT,k:f32,class:PCWSTR,label:&str,style:u32,id:usize,x:i32,y:i32,w:i32,ht:i32)->HWND {
    let label=util::wide(label);
    let c=CreateWindowExW(Default::default(),class,PCWSTR(label.as_ptr()),WS_CHILD|WS_VISIBLE|WINDOW_STYLE(style),
        ui::px(x,k),ui::px(y,k),ui::px(w,k),ui::px(ht,k),h,HMENU(id as *mut _),HINSTANCE(GetModuleHandleW(None).unwrap().0),None).unwrap_or_default();
    SendMessageW(c,WM_SETFONT,WPARAM(font.0 as usize),LPARAM(1));c
}
unsafe fn combo(h:HWND,font:HFONT,k:f32,y:i32,items:&[&str],selected:usize)->HWND {
    let c=child(h,font,k,w!("COMBOBOX"),"",WS_TABSTOP.0|CBS_DROPDOWNLIST as u32|WS_VSCROLL.0,0,220,y,330,240);
    for label in items {let w=util::wide(label);SendMessageW(c,CB_ADDSTRING,WPARAM(0),LPARAM(w.as_ptr() as isize));}
    SendMessageW(c,CB_SETCURSEL,WPARAM(selected),LPARAM(0));c
}
unsafe fn checked(h:HWND)->bool {SendMessageW(h,BM_GETCHECK,WPARAM(0),LPARAM(0)).0==1}
unsafe fn selection(h:HWND)->usize {SendMessageW(h,CB_GETCURSEL,WPARAM(0),LPARAM(0)).0.max(0) as usize}
pub fn open(){
    if let Some(h)=window(){unsafe{let _=SetForegroundWindow(h);}return;}
    if app::cached_state()!=app::State::Idle {util::info("Stop recording and wait for saving before changing settings.");return;}
    let cfg=app::with(|a|a.cfg.clone()).unwrap_or_default();
    unsafe{
        let instance=HINSTANCE(GetModuleHandleW(None).unwrap().0);
        let class=WNDCLASSW{lpfnWndProc:Some(proc_),hInstance:instance,lpszClassName:w!("ZaptureSettings"),hCursor:LoadCursorW(None,IDC_ARROW).unwrap_or_default(),hbrBackground:windows::Win32::Graphics::Gdi::HBRUSH(16usize as *mut _),..Default::default()};
        RegisterClassW(&class);
        let h=match CreateWindowExW(WS_EX_TOPMOST,w!("ZaptureSettings"),w!("Zapture Settings"),WS_OVERLAPPED|WS_CAPTION|WS_SYSMENU,0,0,600,610,None,None,instance,None){Ok(h)=>h,Err(e)=>{util::alert(&e.to_string());return;}};
        let mut work=RECT::default();let _=SystemParametersInfoW(SPI_GETWORKAREA,0,Some(&mut work as *mut _ as *mut _),SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        let k=ui::scale_of(h).min(((work.bottom-work.top-30) as f32/610.0).min((work.right-work.left-30) as f32/590.0).max(0.5));
        let width=ui::px(590,k);let height=ui::px(605,k);
        let _=SetWindowPos(h,HWND_TOPMOST,work.left+((work.right-work.left-width)/2).max(0),work.top+((work.bottom-work.top-height)/2).max(0),width,height,SWP_NOACTIVATE);
        let font=CreateFontW(-ui::px(15,k),0,0,0,FW_NORMAL.0 as i32,0,0,0,DEFAULT_CHARSET.0 as u32,0,0,0,FF_DONTCARE.0 as u32,w!("Segoe UI"));
        let label=|s:&str,y:i32|child(h,font,k,w!("STATIC"),s,0,0,22,y,195,24);
        label("Save captures to",20);
        let current=if cfg.output_dir.is_empty(){util::captures_dir().to_string_lossy().into_owned()}else{cfg.output_dir.clone()};
        let folder=child(h,font,k,w!("EDIT"),&current,WS_BORDER.0|WS_TABSTOP.0|ES_AUTOHSCROLL as u32,0,22,48,435,28);
        child(h,font,k,w!("BUTTON"),"Browse...",WS_TABSTOP.0,BROWSE,465,48,85,28);
        child(h,font,k,w!("STATIC"),"Recordings and Screenshots are saved in separate subfolders.",0,0,22,82,530,24);
        label("Video quality",118);
        let qi=match cfg.bitrate_kbps{0=>0,2500=>1,6000=>2,12000=>3,20000=>4,_=>5};
        let quality=combo(h,font,k,114,&["Automatic (recommended)","Small file - 2.5 Mbps","Balanced - 6 Mbps","High - 12 Mbps","Very high - 20 Mbps","Custom - keep current bitrate"],qi);
        label("Frames per second",154);
        let fi=match cfg.fps{15=>0,24=>1,30=>2,60=>3,_=>4};
        let fps=combo(h,font,k,150,&["15 - low CPU","24","30 - recommended","60 - smoother, more CPU","Custom - keep current FPS"],fi);
        label("Floating control size",190);
        let size=combo(h,font,k,186,&["Small (75%)","Normal (100%)","Large (125%)","Extra large (150%)"],match cfg.bar_scale{75=>0,125=>2,150=>3,_=>1});
        let checkbox=|s:&str,y:i32,value:bool|{let c=child(h,font,k,w!("BUTTON"),s,WS_TABSTOP.0|BS_AUTOCHECKBOX as u32,0,22,y,530,25);SendMessageW(c,BM_SETCHECK,WPARAM(value as usize),LPARAM(0));c};
        let dot=checkbox("Use a floating red dot while recording (click for controls)",225,cfg.recording_dot);
        let shot=checkbox("Open folder and select the file after a screenshot",257,cfg.open_shot_folder);
        let record=checkbox("Open folder and select the file after recording finishes",289,cfg.open_record_folder);
        let shortcuts=[cfg.hk_record,cfg.hk_stop,cfg.hk_shot,cfg.hk_snip];
        let labels=["Record / pause / resume","Stop and save","Screenshot: save + copy","Select screenshot area"];
        let mut keys=[HWND::default();4];
        for i in 0..4 {let y=330+i as i32*35;label(labels[i],y);keys[i]=child(h,font,k,w!("EDIT"),&shortcuts[i].to_string(),WS_BORDER.0|WS_TABSTOP.0|ES_AUTOHSCROLL as u32,0,220,y-3,330,27);}
        child(h,font,k,w!("STATIC"),"Examples: F9, Ctrl+Shift+R, Alt+PrintScreen. PNG is always lossless.",0,0,22,472,530,32);
        child(h,font,k,w!("BUTTON"),"Save",WS_TABSTOP.0|BS_DEFPUSHBUTTON as u32,SAVE,350,520,95,30);
        child(h,font,k,w!("BUTTON"),"Cancel",WS_TABSTOP.0,CANCEL,455,520,95,30);
        FORM.with(|f|*f.borrow_mut()=Some(Form{hwnd:h,font,folder,quality,fps,size,dot,shot,record,keys}));
        let _=ui::wda_exclude(h);let _=ShowWindow(h,SW_SHOW);let _=SetForegroundWindow(h);
    }
}
fn apply()->Result<(),String>{
    if app::cached_state()!=app::State::Idle{return Err("Stop recording before saving settings.".into());}
    let (old,bar)=app::with(|a|(a.cfg.clone(),a.bar)).ok_or("Application unavailable")?;
    let mut next=old.clone();
    FORM.with(|f|->Result<(),String>{let f=f.borrow();let f=f.as_ref().ok_or("Settings unavailable")?;unsafe{
        next.output_dir=text(f.folder).trim().to_string();
        if next.output_dir.contains(['\n','\r','"']) || !std::path::Path::new(&next.output_dir).is_absolute(){return Err("Choose an absolute folder path, such as C:\\Users\\You\\Videos\\Zapture.".into());}
        next.bitrate_kbps=match selection(f.quality){0=>0,1=>2500,2=>6000,3=>12000,4=>20000,_=>old.bitrate_kbps};
        next.fps=match selection(f.fps){0=>15,1=>24,2=>30,3=>60,_=>old.fps};
        next.bar_scale=[75,100,125,150][selection(f.size).min(3)];
        next.recording_dot=checked(f.dot);next.open_shot_folder=checked(f.shot);next.open_record_folder=checked(f.record);
        let mut keys=Vec::new();for edit in f.keys {let value=text(edit);keys.push(parse_hotkey(&value).ok_or_else(||format!("Invalid shortcut: {value}"))?);}
        next.hk_record=keys[0];next.hk_stop=keys[1];next.hk_shot=keys[2];next.hk_snip=keys[3];
    }Ok(())})?;
    let dir=std::path::Path::new(&next.output_dir);std::fs::create_dir_all(dir).map_err(|e|format!("Cannot create folder: {e}"))?;
    let probe=dir.join(format!(".zapture-write-test-{}",util::timestamp()));
    let f=std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).map_err(|e|format!("Cannot write to folder: {e}"))?;drop(f);std::fs::remove_file(&probe).map_err(|e|e.to_string())?;
    crate::win::rebind_hotkeys(bar,&next,&old)?;
    if let Err(e)=next.save(&util::config_path()){let _=crate::win::rebind_hotkeys(bar,&old,&next);return Err(format!("Could not save settings: {e}"));}
    util::set_output_dir(&next.output_dir);ui::bar::set_preferences(next.bar_scale,next.recording_dot);
    app::with(|a|a.cfg=next);ui::refresh_all();Ok(())
}
unsafe fn browse(){
    let Some(h)=window()else{return};
    let result=(||->Option<String>{let dialog:IFileOpenDialog=CoCreateInstance(&FileOpenDialog,None,CLSCTX_INPROC_SERVER).ok()?;dialog.SetOptions(FOS_PICKFOLDERS|FOS_PATHMUSTEXIST).ok()?;dialog.Show(h).ok()?;let item=dialog.GetResult().ok()?;let path=item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;let value=path.to_string().ok();CoTaskMemFree(Some(path.0 as *const _));value})();
    if let Some(path)=result {FORM.with(|f|if let Some(f)=f.borrow().as_ref(){let w=util::wide(&path);let _=SetWindowTextW(f.folder,PCWSTR(w.as_ptr()));});}
}
unsafe extern "system" fn proc_(h:HWND,msg:u32,wp:WPARAM,lp:LPARAM)->LRESULT{
    match msg {
        WM_COMMAND=>{match wp.0&0xffff {SAVE=>match apply(){Ok(())=>{let _=DestroyWindow(h);},Err(e)=>util::alert(&e)},CANCEL=>{let _=DestroyWindow(h);},BROWSE=>browse(),_=>{}}LRESULT(0)},
        WM_CLOSE=>{let _=DestroyWindow(h);LRESULT(0)},
        WM_DESTROY=>{FORM.with(|f|if let Some(f)=f.borrow_mut().take(){let _=DeleteObject(f.font);});LRESULT(0)},
        _=>DefWindowProcW(h,msg,wp,lp)
    }
}
