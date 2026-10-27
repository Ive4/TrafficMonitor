//! Standalone Rust implementation of the published MSVC plugin ABI.
//! Compile with rustc --crate-type cdylib --edition 2024; no host crate/C++ dependency.
#![allow(unsafe_op_in_unsafe_fn)]
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicI32,Ordering};
macro_rules! callback {
 ($(fn $name:ident($($a:ident:$t:ty),*)->$r:ty $body:block)*)=>{$(
 #[cfg(target_arch="x86")] unsafe extern "thiscall" fn $name($($a:$t),*)->$r $body
 #[cfg(not(target_arch="x86"))] unsafe extern "system" fn $name($($a:$t),*)->$r $body
 )*};
}
macro_rules! invoke {($p:expr,$i:expr,($($t:ty),*),($($a:expr),*),$r:ty)=>{{
 #[cfg(target_arch="x86")]type F=unsafe extern "thiscall" fn(*mut c_void,$($t),*)->$r;
 #[cfg(not(target_arch="x86"))]type F=unsafe extern "system" fn(*mut c_void,$($t),*)->$r;
 let p=$p;let table=*(p as *const *const *const c_void);let f:F=std::mem::transmute(*table.add($i));f(p,$($a),*)
}};}
static API:AtomicI32=AtomicI32::new(8);
static MODE:AtomicI32=AtomicI32::new(0);
static LABEL_COLOR:AtomicI32=AtomicI32::new(-1);
static VALUE_COLOR:AtomicI32=AtomicI32::new(-1);
#[repr(C)]struct Obj{vtable:*const *const c_void,host:*mut c_void,refresh:u32,up:u64,commands:i32,value:Option<Vec<u16>>,custom:bool}
#[repr(C)]struct Monitor{up:u64,down:u64,values:[i32;9]}
struct Table<const N:usize>([*const c_void;N]);unsafe impl<const N:usize> Sync for Table<N>{}
static mut PLUGIN:Obj=Obj{vtable:PLUGIN_TABLE.0.as_ptr(),host:ptr::null_mut(),refresh:0,up:0,commands:0,value:None,custom:false};
static mut ITEM:Obj=Obj{vtable:ITEM_TABLE.0.as_ptr(),host:ptr::null_mut(),refresh:0,up:0,commands:0,value:None,custom:false};
static mut DRAW:Obj=Obj{vtable:ITEM_TABLE.0.as_ptr(),host:ptr::null_mut(),refresh:0,up:0,commands:0,value:None,custom:true};
static EMPTY:[u16;1]=[0];
fn wide(s:&str)->Vec<u16>{s.encode_utf16().chain(Some(0)).collect()}
unsafe fn str_value(p:*const u16)->String{if p.is_null(){return String::new();}let mut n=0;while *p.add(n)!=0&&n<65536{n+=1;}String::from_utf16_lossy(std::slice::from_raw_parts(p,n))}
fn gate(v:i32){assert!(API.load(Ordering::Relaxed)>=v,"host invoked unavailable API slot");}
callback!{
 fn api(_p:*mut c_void)->i32{API.load(Ordering::Relaxed)}
 fn item(_p:*mut c_void,i:i32)->*mut c_void{if MODE.load(Ordering::Relaxed)==2{return ptr::null_mut();}match i{0=>(&raw mut ITEM).cast(),1=>if MODE.load(Ordering::Relaxed)==5{(&raw mut ITEM).cast()}else{(&raw mut DRAW).cast()},_=>ptr::null_mut()}}
 fn data(p:*mut c_void)->(){let p=&mut *(p as *mut Obj);p.refresh+=1;let s=format!("refresh={},up={},commands={}",p.refresh,p.up,p.commands);(*(&raw mut ITEM)).value=Some(wide(&s));}
 fn options(_p:*mut c_void,_window:*mut c_void)->i32{MODE.load(Ordering::Relaxed)}
 fn info(_p:*mut c_void,i:i32)->*const u16{static NAME:[u16;5]=[82,117,115,116,0];if i==0{NAME.as_ptr()}else{EMPTY.as_ptr()}}
 fn monitor(p:*mut c_void,m:*const Monitor)->(){(*(p as *mut Obj)).up=(*m).up;}
 fn tooltip(_p:*mut c_void)->*const u16{gate(2);static S:[u16;5]=[104,105,110,116,0];S.as_ptr()}
 fn extended(_p:*mut c_void,i:i32,s:*const u16)->(){gate(2);match i{0=>LABEL_COLOR.store(str_value(s).parse().unwrap(),Ordering::Relaxed),1=>VALUE_COLOR.store(str_value(s).parse().unwrap(),Ordering::Relaxed),17=>assert!(str_value(s).ends_with('\\')),_=>()}}
 fn icon(_p:*mut c_void)->*mut c_void{gate(2);ptr::null_mut()}
 fn count(_p:*mut c_void)->i32{gate(5);if MODE.load(Ordering::Relaxed)==4{2048}else{1}}
 fn command_name(_p:*mut c_void,_i:i32)->*const u16{gate(5);static S:[u16;7]=[84,111,103,103,108,101,0];S.as_ptr()}
 fn command_icon(_p:*mut c_void,_i:i32)->*mut c_void{gate(5);ptr::null_mut()}
 fn command(p:*mut c_void,i:i32,_w:*mut c_void,_para:*mut c_void)->(){gate(5);assert_eq!(i,0);let p=&mut *(p as *mut Obj);p.commands+=1;if !p.host.is_null(){let s=wide("Rust command");invoke!(p.host,4,(*const u16),(s.as_ptr()),());}}
 fn checked(p:*mut c_void,_i:i32)->i32{gate(5);(*(p as *mut Obj)).commands&1}
 fn initialize(p:*mut c_void,host:*mut c_void)->(){
  gate(7);(*(p as *mut Obj)).host=host;
  assert_eq!(invoke!(host,0,(),(),i32),1);assert!(!str_value(invoke!(host,1,(),(),*const u16)).is_empty());
  assert_eq!(invoke!(host,2,(i32),(2),f64),42.5);
  assert_eq!(str_value(invoke!(host,3,(i32,i32),(2,1),*const u16)),"main CPU");
  assert_eq!(str_value(invoke!(host,3,(i32,i32),(2,0),*const u16)),"taskbar CPU");
  assert_eq!(invoke!(host,5,(),(),u16),0x804);assert!(!str_value(invoke!(host,6,(),(),*const u16)).is_empty());
  assert_eq!(invoke!(host,7,(i32),(0),i32),144);assert_eq!(invoke!(host,7,(i32),(1),i32),192);assert_eq!(invoke!(host,8,(),(),u32),0x123456);
  let key=wide("key");let section=wide("text");assert_eq!(str_value(invoke!(host,9,(*const u16,*const u16),(key.as_ptr(),section.as_ptr()),*const u16)),"resource");
  assert_eq!(invoke!(host,10,(),(),*mut c_void) as usize,123);assert_eq!(invoke!(host,11,(),(),*mut c_void) as usize,456);
  let s=wide("Rust initialized");invoke!(host,4,(*const u16),(s.as_ptr()),());
 }
 fn name(_p:*mut c_void)->*const u16{static S:[u16;5]=[82,117,115,116,0];S.as_ptr()}
 fn id(p:*mut c_void)->*const u16{static A:[u16;5]=[116,101,120,116,0];static B:[u16;5]=[100,114,97,119,0];if (*(p as *mut Obj)).custom{B.as_ptr()}else{A.as_ptr()}}
 fn label(_p:*mut c_void)->*const u16{static S:[u16;3]=[82,58,0];S.as_ptr()}
 fn value(p:*mut c_void)->*const u16{(*(p as *mut Obj)).value.as_ref().map_or(EMPTY.as_ptr(),|s|s.as_ptr())}
 fn sample(_p:*mut c_void)->*const u16{static S:[u16;4]=[49,48,48,0];S.as_ptr()}
 fn custom(p:*mut c_void)->bool{(*(p as *mut Obj)).custom}
 fn width(_p:*mut c_void)->i32{64}
 fn draw(_p:*mut c_void,dc:*mut c_void,x:i32,y:i32,_w:i32,_h:i32,_dark:bool)->(){if API.load(Ordering::Relaxed)>=2{assert_eq!(LABEL_COLOR.load(Ordering::Relaxed),0x123456);assert_eq!(VALUE_COLOR.load(Ordering::Relaxed),0xabcdef);}SetPixel(dc,x,y,0xff);}
 fn width_ex(_p:*mut c_void,_dc:*mut c_void)->i32{gate(3);101}
 fn mouse(_p:*mut c_void,t:i32,x:i32,y:i32,w:*mut c_void,f:i32)->i32{gate(3);assert_eq!((t,x,y,w as usize,f),(2,11,22,333,1));1}
 fn key(_p:*mut c_void,k:i32,c:bool,s:bool,a:bool,w:*mut c_void,f:i32)->i32{gate(4);assert_eq!((k,c,s,a,w as usize,f),(65,true,false,true,333,1));1}
 fn reserved(_p:*mut c_void,_i:i32,_a:*mut c_void,_b:*mut c_void)->*mut c_void{gate(4);ptr::null_mut()}
 fn graph(_p:*mut c_void)->i32{gate(6);1}
 fn graph_value(_p:*mut c_void)->f32{gate(6);0.625}
 fn draw_ex(_p:*mut c_void,d:*mut c_void,x:i32,y:i32,w:i32,h:i32,_dark:bool)->bool{
  gate(8);assert_eq!(invoke!(d,0,(),(),i32),0);let s=wide("Rust draw");
  invoke!(d,2,(i32,i32,i32,i32),(x,y,w,h),());
  invoke!(d,3,(i32,i32,i32,i32,u32,u8),(x,y,w,h,0x00ff00,128),());
  invoke!(d,1,(i32,i32,i32,i32,*const u16,u32,i32,bool,u8),(x,y,w,h,s.as_ptr(),0xffffff,2,false,255),());
  invoke!(d,4,(i32,i32,i32,i32,u32,i32,bool,u8,i32),(x,y,w,h,0xff0000,1,false,255,3),());
  invoke!(d,8,(i32,i32,i32,i32,u32,i32,bool,u8),(x,y,x+w,y+h,0xffffff,1,false,128),());
  let mut tw=0;let mut th=0;invoke!(d,7,(*const u16,*mut i32,*mut i32),(s.as_ptr(),&mut tw,&mut th),());assert!(tw>0&&th>0);
  let dc=invoke!(d,9,(),(),*mut c_void);assert!(!dc.is_null());
  let b=CreateCompatibleBitmap(dc,4,4);assert!(!b.is_null());
  for mode in 0..3{invoke!(d,5,(*mut c_void,i32,i32,i32,i32,i32,u8),(b,x+4,y+4,8,8,mode,255),());}DeleteObject(b);
  let icon=LoadIconW(ptr::null_mut(),32512usize as *const u16);assert!(!icon.is_null());invoke!(d,6,(*mut c_void,i32,i32,i32,i32),(icon,x+12,y+4,16,16),());true
 }
 fn double(_p:*mut c_void)->i32{gate(8);1}
}
static PLUGIN_TABLE:Table<15>=Table([api as _,item as _,data as _,options as _,info as _,monitor as _,tooltip as _,extended as _,icon as _,count as _,command_name as _,command_icon as _,command as _,checked as _,initialize as _]);
static ITEM_TABLE:Table<16>=Table([name as _,id as _,label as _,value as _,sample as _,custom as _,width as _,draw as _,width_ex as _,mouse as _,key as _,reserved as _,graph as _,graph_value as _,draw_ex as _,double as _]);
#[unsafe(no_mangle)]pub extern "C" fn FixtureSetApi(version:i32,mode:i32){API.store(version,Ordering::Relaxed);MODE.store(mode,Ordering::Relaxed);}
#[unsafe(no_mangle)]pub unsafe extern "C" fn TMPluginGetInstance()->*mut c_void{
 if MODE.load(Ordering::Relaxed)==3{return ptr::null_mut();}
 let p=&mut *(&raw mut PLUGIN);p.host=ptr::null_mut();p.refresh=0;p.up=0;p.commands=0;
 (&raw mut PLUGIN).cast()
}
#[link(name="gdi32")]unsafe extern "system"{fn SetPixel(dc:*mut c_void,x:i32,y:i32,c:u32)->u32;fn CreateCompatibleBitmap(dc:*mut c_void,w:i32,h:i32)->*mut c_void;fn DeleteObject(p:*mut c_void)->i32;}
#[link(name="user32")]unsafe extern "system"{fn LoadIconW(instance:*mut c_void,name:*const u16)->*mut c_void;}
