use super::*;
use windows_sys::Win32::Foundation::{RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{DI_NORMAL, DrawIconEx};

#[repr(C)]
pub(super) struct Drawer {
    vtable: *const *const c_void,
    hdc: HDC,
    error: Option<String>,
}
impl Drawer {
    pub(super) fn new(hdc: usize) -> Self {
        Self {
            vtable: TABLE.0.as_ptr(),
            hdc: hdc as _,
            error: None,
        }
    }
    pub(super) fn finish(self) -> Result<(), String> {
        self.error.map_or(Ok(()), Err)
    }
    fn fail(&mut self, message: &str) {
        if self.error.is_none() {
            self.error = Some(message.into());
        }
    }
    // Composite on an isolated DC with a copied background. This supports alpha for
    // every GDI primitive, including text, without altering the caller's DC state.
    unsafe fn paint(&mut self, rect: RECT, alpha: u8, operation: impl FnOnce(HDC) -> bool) {
        let w = rect.right.saturating_sub(rect.left);
        let h = rect.bottom.saturating_sub(rect.top);
        if w <= 0 || h <= 0 || alpha == 0 {
            return;
        }
        if w > 32768 || h > 32768 {
            self.fail("plugin draw dimensions exceed GDI limits");
            return;
        }
        if alpha == 255 {
            if !operation(self.hdc) {
                self.fail("plugin GDI drawing operation failed");
            }
            return;
        }
        let dc = CreateCompatibleDC(self.hdc);
        if dc.is_null() {
            self.fail("CreateCompatibleDC failed");
            return;
        }
        let bitmap = CreateCompatibleBitmap(self.hdc, w, h);
        if bitmap.is_null() {
            DeleteDC(dc);
            self.fail("CreateCompatibleBitmap failed");
            return;
        }
        let previous = SelectObject(dc, bitmap);
        let copied = BitBlt(dc, 0, 0, w, h, self.hdc, rect.left, rect.top, SRCCOPY);
        let font = GetCurrentObject(self.hdc, OBJ_FONT as u32);
        if !font.is_null() {
            SelectObject(dc, font);
        }
        SetViewportOrgEx(dc, -rect.left, -rect.top, ptr::null_mut());
        let painted = operation(dc);
        SetViewportOrgEx(dc, 0, 0, ptr::null_mut());
        let blended = GdiAlphaBlend(
            self.hdc,
            rect.left,
            rect.top,
            w,
            h,
            dc,
            0,
            0,
            w,
            h,
            BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: alpha,
                AlphaFormat: 0,
            },
        );
        SelectObject(dc, previous);
        DeleteObject(bitmap);
        DeleteDC(dc);
        if copied == 0 || !painted || blended == 0 {
            self.fail("plugin alpha composition failed");
        }
    }
}
fn rect(x: i32, y: i32, w: i32, h: i32) -> RECT {
    RECT {
        left: x,
        top: y,
        right: x.saturating_add(w),
        bottom: y.saturating_add(h),
    }
}
unsafe fn drawer<'a>(p: *mut c_void) -> &'a mut Drawer {
    &mut *(p as *mut Drawer)
}
callback! {
    fn api(_p:*mut c_void)->i32 {0}
    fn text_draw(p:*mut c_void,x:i32,y:i32,w:i32,h:i32,s:*const u16,color:u32,align:i32,multi:bool,alpha:u8)->() {
        let d=drawer(p);let Ok(text)=string(s) else {d.fail("invalid plugin text");return;};let text=wide(&text);
        d.paint(rect(x,y,w,h),alpha,|dc|{
            let old_color=SetTextColor(dc,color);let old_mode=SetBkMode(dc,TRANSPARENT as _);
            let mut bounds=rect(x,y,w,h);
            let alignment=match align{1=>DT_RIGHT,2=>DT_CENTER,_=>DT_LEFT};
            let flags=alignment|DT_NOPREFIX|if multi{DT_WORDBREAK}else{DT_SINGLELINE|DT_VCENTER};
            let result=DrawTextW(dc,text.as_ptr(),(text.len()-1) as i32,&mut bounds,flags);
            SetTextColor(dc,old_color);SetBkMode(dc,old_mode);
            result!=0 || text.len()==1
        });
    }
    fn draw_rect(p:*mut c_void,x:i32,y:i32,w:i32,h:i32)->() {
        let d=drawer(p);let region=CreateRectRgn(x,y,x.saturating_add(w),y.saturating_add(h));
        if region.is_null(){d.fail("CreateRectRgn failed");return;}
        let result=SelectClipRgn(d.hdc,region);DeleteObject(region);if result==ERROR {d.fail("SelectClipRgn failed");}
    }
    fn fill(p:*mut c_void,x:i32,y:i32,w:i32,h:i32,color:u32,alpha:u8)->() {
        drawer(p).paint(rect(x,y,w,h),alpha,|dc|{let brush=CreateSolidBrush(color);if brush.is_null(){return false;}let ok=FillRect(dc,&rect(x,y,w,h),brush)!=0;DeleteObject(brush);ok});
    }
    fn outline(p:*mut c_void,x:i32,y:i32,w:i32,h:i32,color:u32,width:i32,dot:bool,alpha:u8,radius:i32)->() {
        let width=width.clamp(1,32768);
        drawer(p).paint(rect(x.saturating_sub(width),y.saturating_sub(width),w.saturating_add(width*2),h.saturating_add(width*2)),alpha,|dc|{
            let pen=CreatePen(if dot{PS_DOT}else{PS_SOLID},width,color);if pen.is_null(){return false;}
            let old_pen=SelectObject(dc,pen);let old_brush=SelectObject(dc,GetStockObject(NULL_BRUSH));
            let ok=if radius>0{RoundRect(dc,x,y,x.saturating_add(w),y.saturating_add(h),radius.saturating_mul(2),radius.saturating_mul(2))}else{Rectangle(dc,x,y,x.saturating_add(w),y.saturating_add(h))};
            SelectObject(dc,old_brush);SelectObject(dc,old_pen);DeleteObject(pen);ok!=0
        });
    }
    fn bitmap(p:*mut c_void,bitmap:*mut c_void,x:i32,y:i32,w:i32,h:i32,mode:i32,alpha:u8)->() {
        let d=drawer(p);if bitmap.is_null(){d.fail("DrawBitmap received a null HBITMAP");return;}
        let mut info:BITMAP=std::mem::zeroed();
        if GetObjectW(bitmap,std::mem::size_of::<BITMAP>() as i32,&mut info as *mut _ as _)==0 || info.bmWidth<=0 || info.bmHeight==0 {d.fail("DrawBitmap received an invalid HBITMAP");return;}
        let sh=info.bmHeight.saturating_abs();let sw=info.bmWidth;
        d.paint(rect(x,y,w,h),alpha,|dc|{
            let source=CreateCompatibleDC(dc);if source.is_null(){return false;}
            let old=SelectObject(source,bitmap);let mut dx=x;let mut dy=y;let mut dw=w;let mut dh=h;let mut sx=0;let mut sy=0;let mut cw=sw;let mut ch=sh;
            if mode==1 {if i64::from(sw)*i64::from(h)>i64::from(sh)*i64::from(w){cw=(i64::from(sh)*i64::from(w)/i64::from(h)) as i32;sx=(sw-cw)/2;}else{ch=(i64::from(sw)*i64::from(h)/i64::from(w)) as i32;sy=(sh-ch)/2;}}
            else if mode==2 {if i64::from(sw)*i64::from(h)>i64::from(sh)*i64::from(w){dh=(i64::from(sh)*i64::from(w)/i64::from(sw)) as i32;dy+=(h-dh)/2;}else{dw=(i64::from(sw)*i64::from(h)/i64::from(sh)) as i32;dx+=(w-dw)/2;}}
            let old_mode=SetStretchBltMode(dc,HALFTONE);let ok=StretchBlt(dc,dx,dy,dw,dh,source,sx,sy,cw,ch,SRCCOPY)!=0;SetStretchBltMode(dc,old_mode);
            SelectObject(source,old);DeleteDC(source);ok
        });
    }
    fn icon(p:*mut c_void,icon:*mut c_void,x:i32,y:i32,w:i32,h:i32)->() {
        let d=drawer(p);if DrawIconEx(d.hdc,x,y,icon,w,h,0,ptr::null_mut(),DI_NORMAL)==0{d.fail("DrawIconEx failed");}
    }
    fn extent(p:*mut c_void,s:*const u16,w:*mut i32,h:*mut i32)->() {
        let d=drawer(p);if w.is_null() || h.is_null(){d.fail("GetTextExtent null output pointer");return;}
        *w=0;*h=0;
        let Ok(text)=string(s) else {d.fail("GetTextExtent invalid text");return;};let text=wide(&text);let mut size:SIZE=std::mem::zeroed();
        if GetTextExtentPoint32W(d.hdc,text.as_ptr(),(text.len()-1) as i32,&mut size)==0{d.fail("GetTextExtentPoint32W failed");return;}*w=size.cx;*h=size.cy;
    }
    fn line(p:*mut c_void,x1:i32,y1:i32,x2:i32,y2:i32,color:u32,width:i32,dot:bool,alpha:u8)->() {
        let width=width.clamp(1,32768);
        let bounds=RECT{left:x1.min(x2).saturating_sub(width),top:y1.min(y2).saturating_sub(width),right:x1.max(x2).saturating_add(width),bottom:y1.max(y2).saturating_add(width)};
        drawer(p).paint(bounds,alpha,|dc|{let pen=CreatePen(if dot{PS_DOT}else{PS_SOLID},width,color);if pen.is_null(){return false;}let old=SelectObject(dc,pen);let moved=MoveToEx(dc,x1,y1,ptr::null_mut());let ok=LineTo(dc,x2,y2);SelectObject(dc,old);DeleteObject(pen);moved!=0&&ok!=0});
    }
    fn hdc(p:*mut c_void)->*mut c_void {drawer(p).hdc}
}
struct VTable([*const c_void; 10]);
unsafe impl Sync for VTable {}
static TABLE: VTable = VTable([
    api as _,
    text_draw as _,
    draw_rect as _,
    fill as _,
    outline as _,
    bitmap as _,
    icon as _,
    extent as _,
    line as _,
    hdc as _,
]);
