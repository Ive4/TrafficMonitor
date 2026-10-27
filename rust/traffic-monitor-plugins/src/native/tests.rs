use super::*;
use windows_sys::Win32::Graphics::Gdi::*;

#[repr(C)]
struct Fake {
    table: *const *const c_void,
    refresh: u32,
    monitor: MonitorInfo,
    mouse: [i32; 5],
    keys: [i32; 6],
    option: i32,
    extended: Vec<(i32, String)>,
    commands: u32,
}
callback! {
 fn api(_p:*mut c_void)->i32{8}
 fn no_item(_p:*mut c_void,_i:i32)->*mut c_void{ptr::null_mut()}
 fn data(p:*mut c_void)->(){(*(p as *mut Fake)).refresh+=1;}
 fn options(p:*mut c_void,_parent:*mut c_void)->i32{(*(p as *mut Fake)).option}
 fn info(_p:*mut c_void,_i:i32)->*const u16{static S:[u16;1]=[0];S.as_ptr()}
 fn monitor(p:*mut c_void,m:*const MonitorInfo)->(){(*(p as *mut Fake)).monitor = *m;}
 fn empty(_p:*mut c_void)->*const u16{static S:[u16;1]=[0];S.as_ptr()}
 fn extended(p:*mut c_void,i:i32,s:*const u16)->(){(*(p as *mut Fake)).extended.push((i,string(s).unwrap()));}
 fn icon(_p:*mut c_void)->*mut c_void{ptr::null_mut()}
 fn count(_p:*mut c_void)->i32{1}
 fn command_name(_p:*mut c_void,_i:i32)->*const u16{static S:[u16;2]=[65,0];S.as_ptr()}
 fn command_icon(_p:*mut c_void,_i:i32)->*mut c_void{ptr::null_mut()}
 fn command(p:*mut c_void,i:i32,_w:*mut c_void,_para:*mut c_void)->(){assert_eq!(i,0);(*(p as *mut Fake)).commands+=1;}
 fn checked(p:*mut c_void,_i:i32)->i32{i32::from((*(p as *mut Fake)).commands>0)}
 fn initialize(_p:*mut c_void,_host:*mut c_void)->(){}
 fn name(_p:*mut c_void)->*const u16{static S:[u16;5]=[116,101,115,116,0];S.as_ptr()}
 fn custom(p:*mut c_void)->bool{(*(p as *mut Fake)).option==77}
 fn width(_p:*mut c_void)->i32{40}
 fn draw(p:*mut c_void,_hdc:*mut c_void,_x:i32,_y:i32,_w:i32,_h:i32,_dark:bool)->(){(*(p as *mut Fake)).commands+=1;}
 fn width_ex(_p:*mut c_void,_dc:*mut c_void)->i32{81}
 fn mouse(p:*mut c_void,kind:i32,x:i32,y:i32,w:*mut c_void,flag:i32)->i32{(*(p as *mut Fake)).mouse=[kind,x,y,w as usize as i32,flag];1}
 fn key(p:*mut c_void,key:i32,ctrl:bool,shift:bool,alt:bool,w:*mut c_void,flag:i32)->i32{(*(p as *mut Fake)).keys=[key,ctrl as i32,shift as i32,alt as i32,w as usize as i32,flag];1}
 fn reserved(_p:*mut c_void,_kind:i32,_a:*mut c_void,_b:*mut c_void)->*mut c_void{ptr::null_mut()}
 fn graph(_p:*mut c_void)->i32{1}
 fn graph_value(_p:*mut c_void)->f32{0.75}
 fn draw_ex(p:*mut c_void,_drawer:*mut c_void,_x:i32,_y:i32,_w:i32,_h:i32,_dark:bool)->bool{let p=&mut *(p as *mut Fake);assert_eq!(p.commands,1);p.refresh+=1;false}
 fn double(_p:*mut c_void)->i32{1}
}
fn fake(table: &[*const c_void]) -> Fake {
    Fake {
        table: table.as_ptr(),
        refresh: 0,
        monitor: MonitorInfo::default(),
        mouse: [0; 5],
        keys: [0; 6],
        option: 2,
        extended: Vec::new(),
        commands: 0,
    }
}
fn manager(plugin: &mut Fake, item: &mut Fake, version: i32) -> PluginManager {
    let mut m = PluginManager::new(HostInfo::default());
    let mut info = blank_info(PathBuf::from("test.dll"), "test.dll".into(), true);
    info.api_version = version;
    if version >= 5 {
        info.commands.push(PluginCommand {
            index: 0,
            name: "A".into(),
            icon: 0,
            checked: false,
        });
    }
    m.entries.push(Entry {
        info,
        loaded: Some(Loaded {
            _library: Library(ptr::null_mut()),
            object: plugin as *mut _ as _,
            items: vec![item as *mut _ as _],
        }),
    });
    m
}
fn plugin_table() -> [*const c_void; 15] {
    [
        api as _,
        no_item as _,
        data as _,
        options as _,
        info as _,
        monitor as _,
        empty as _,
        extended as _,
        icon as _,
        count as _,
        command_name as _,
        command_icon as _,
        command as _,
        checked as _,
        initialize as _,
    ]
}
fn item_table() -> [*const c_void; 16] {
    [
        name as _,
        name as _,
        name as _,
        name as _,
        name as _,
        custom as _,
        width as _,
        draw as _,
        width_ex as _,
        mouse as _,
        key as _,
        reserved as _,
        graph as _,
        graph_value as _,
        draw_ex as _,
        double as _,
    ]
}
#[test]
fn draw_colors_target_item_owner_and_gate_api_without_format_broadcast() {
    let pt = plugin_table();
    let it = item_table();
    for version in 1..=8 {
        let mut p = fake(&pt[..if version == 1 { 6 } else { 15 }]);
        let mut i = fake(&it);
        let mut m = manager(&mut p, &mut i, version);
        m.set_draw_colors("test", 0x123456, 0xabcdef).unwrap();
        if version == 1 {
            assert!(p.extended.is_empty());
        } else {
            assert_eq!(
                p.extended,
                vec![(0, "1193046".into()), (1, "11259375".into())]
            );
        }
        assert!(m.set_draw_colors("missing", 0, 0).is_err());
        m.shutdown().unwrap();
        assert!(m.set_draw_colors("test", 0, 0).is_err());
    }
}
#[test]
fn api1_truncated_vtables_never_read_newer_slots() {
    let pt = plugin_table();
    let it = item_table();
    let mut p = fake(&pt[..6]);
    let mut item = fake(&it[..8]);
    let mut m = manager(&mut p, &mut item, 1);
    m.refresh(MonitorInfo {
        up_speed: 123,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(p.refresh, 1);
    assert_eq!(p.monitor.up_speed, 123);
    assert_eq!(m.tooltip("test.dll").unwrap(), "");
    assert_eq!(m.plugin_icon("test.dll").unwrap(), 0);
    assert!(
        !m.mouse(
            "test",
            MouseEvent {
                kind: MouseEventKind::Left,
                x: 1,
                y: 2,
                window: 3,
                taskbar: true
            }
        )
        .unwrap()
    );
    assert!(
        !m.keyboard(
            "test",
            KeyboardEvent {
                key: 65,
                ctrl: true,
                shift: false,
                alt: false,
                window: 3,
                taskbar: false
            }
        )
        .unwrap()
    );
    assert!(m.command("test.dll", 0, 0).is_err());
    let items = m.items().unwrap();
    assert_eq!(items[0].graph, None);
    assert!(!items[0].double_line);
    m.update_host(HostInfo::default()).unwrap();
    assert!(p.extended.is_empty());
}
#[test]
fn modern_slots_preserve_parameters_and_state() {
    let pt = plugin_table();
    let it = item_table();
    let mut p = fake(&pt);
    let mut item = fake(&it);
    let mut m = manager(&mut p, &mut item, 8);
    assert!(
        m.mouse(
            "test",
            MouseEvent {
                kind: MouseEventKind::WheelDown,
                x: -3,
                y: 9,
                window: 123,
                taskbar: true
            }
        )
        .unwrap()
    );
    assert_eq!(item.mouse, [4, -3, 9, 123, 1]);
    assert!(
        m.keyboard(
            "test",
            KeyboardEvent {
                key: 65,
                ctrl: true,
                shift: false,
                alt: true,
                window: 456,
                taskbar: false
            }
        )
        .unwrap()
    );
    assert_eq!(item.keys, [65, 1, 0, 1, 456, 0]);
    assert_eq!(
        m.configure("test.dll", 0).unwrap(),
        OptionResult::NotProvided
    );
    p.option = 0;
    assert_eq!(m.configure("test.dll", 0).unwrap(), OptionResult::Changed);
    p.option = 1;
    assert_eq!(m.configure("test.dll", 0).unwrap(), OptionResult::Unchanged);
    p.option = 88;
    assert!(m.configure("test.dll", 0).is_err());
    m.command("test.dll", 0, 0).unwrap();
    assert_eq!(p.commands, 1);
    assert_eq!(m.plugin_icon("test.dll").unwrap(), 0);
    m.refresh(MonitorInfo::default()).unwrap();
    assert!(m.plugins()[0].commands[0].checked);
    let items = m.items().unwrap();
    assert_eq!(items[0].graph, Some(0.75));
    assert!(items[0].double_line);
    m.set_enabled("TEST.DLL", false).unwrap();
    assert!(m.items().unwrap().is_empty());
    assert!(m.refresh(MonitorInfo::default()).is_ok());
    assert_eq!(p.refresh, 1);
    m.shutdown().unwrap();
    m.shutdown().unwrap();
    assert!(m.items().is_err());
    assert!(m.configure("test.dll", 0).is_err());
}
#[test]
fn unchanged_host_update_does_not_reload_plugin_config() {
    let pt = plugin_table();
    let it = item_table();
    let mut p = fake(&pt);
    let mut item = fake(&it);
    let mut m = manager(&mut p, &mut item, 8);
    m.update_host(HostInfo::default()).unwrap();
    assert!(p.extended.is_empty());
    let mut host = HostInfo::default();
    host.extended.insert(3, "1".into());
    m.update_host(host.clone()).unwrap();
    m.update_host(host).unwrap();
    assert_eq!(p.extended, vec![(3, "1".into())]);
}
#[test]
fn host_vtable_exposes_all_current_values_and_copies_notifications() -> Result<(), String> {
    let mut info = HostInfo {
        version: "9.1".into(),
        language_id: 0x804,
        config_dir: PathBuf::from("C:\\config"),
        main_dpi: 144,
        taskbar_dpi: 192,
        theme_color: 0x123456,
        main_window: 101,
        taskbar_window: 202,
        ..Default::default()
    };
    info.monitor_values[2] = 12.5;
    info.main_value_strings[2] = "main".into();
    info.taskbar_value_strings[2] = "taskbar".into();
    info.strings
        .insert(("KEY".into(), "text".into()), "翻译".into());
    let mut h = Host::new(info);
    let p = &mut h as *mut _ as *mut c_void;
    assert_eq!(call!(p, 0, (), (), i32), 1);
    assert_eq!(text(p, 1)?, "9.1");
    assert_eq!(call!(p, 2, (i32), (2), f64), 12.5);
    let value = call!(p, 3, (i32, i32), (2, 1), *const u16);
    assert_eq!(unsafe { string(value) }?, "main");
    let value = call!(p, 3, (i32, i32), (2, 0), *const u16);
    assert_eq!(unsafe { string(value) }?, "taskbar");
    assert_eq!(call!(p, 5, (), (), u16), 0x804);
    assert_eq!(text(p, 6)?, "C:\\config\\");
    assert_eq!(call!(p, 7, (i32), (1), i32), 192);
    assert_eq!(call!(p, 8, (), (), u32), 0x123456);
    let key = wide("KEY");
    let section = wide("text");
    let value = call!(
        p,
        9,
        (*const u16, *const u16),
        (key.as_ptr(), section.as_ptr()),
        *const u16
    );
    assert_eq!(unsafe { string(value) }?, "翻译");
    assert_eq!(call!(p, 10, (), (), *mut c_void) as usize, 101);
    assert_eq!(call!(p, 11, (), (), *mut c_void) as usize, 202);
    let msg = wide("notification");
    call!(p, 4, (*const u16), (msg.as_ptr()), ());
    assert_eq!(h.take_notifications(), vec!["notification"]);
    h.close();
    call!(p, 4, (*const u16), (msg.as_ptr()), ());
    assert!(h.take_notifications().is_empty());
    assert_eq!(call!(p, 10, (), (), *mut c_void), ptr::null_mut());
    Ok(())
}
#[test]
fn drawer_alpha_and_measurement_are_real_gdi_operations() -> Result<(), String> {
    unsafe {
        let screen = GetDC(ptr::null_mut());
        let dc = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, 100, 100);
        ReleaseDC(ptr::null_mut(), screen);
        let old = SelectObject(dc, bmp);
        PatBlt(dc, 0, 0, 100, 100, BLACKNESS);
        let mut d = drawer::Drawer::new(dc as usize);
        let p = &mut d as *mut _ as *mut c_void;
        call!(
            p,
            3,
            (i32, i32, i32, i32, u32, u8),
            (0, 0, 50, 50, 255, 128),
            ()
        );
        let pixel = GetPixel(dc, 10, 10);
        assert!((126..=129).contains(&(pixel & 255)), "pixel {pixel:x}");
        let text = wide("TrafficMonitor");
        let mut w = 0;
        let mut h = 0;
        call!(
            p,
            7,
            (*const u16, *mut i32, *mut i32),
            (text.as_ptr(), &mut w, &mut h),
            ()
        );
        assert!(w > 0 && h > 0);
        assert_eq!(call!(p, 9, (), (), *mut c_void), dc);
        d.finish()?;
        SelectObject(dc, old);
        DeleteObject(bmp);
        DeleteDC(dc);
        Ok(())
    }
}
#[test]
fn invalid_slots_and_missing_files_report_errors() {
    assert!(slot(ptr::null_mut(), 0).is_err());
    let table = [ptr::null(); 1];
    let mut p = fake(&table);
    assert!(slot(&mut p as *mut _ as _, 0).is_err());
    let mut m = PluginManager::new(HostInfo::default());
    assert!(
        m.load(Path::new("missing-trafficmonitor-fixture.dll"))
            .is_err()
    );
}

#[test]
fn legacy_and_extended_draw_callbacks_each_run_once_in_original_order() {
    let pt = plugin_table();
    let it = item_table();
    let mut p = fake(&pt);
    let mut item = fake(&it);
    item.option = 77;
    let mut manager = manager(&mut p, &mut item, 8);
    unsafe {
        let dc = CreateCompatibleDC(ptr::null_mut());
        assert!(!dc.is_null());
        manager
            .draw("test", dc as usize, [0, 0, 40, 20], false, false)
            .unwrap();
        DeleteDC(dc);
    }
    assert_eq!(
        item.commands, 1,
        "legacy DrawItem must be called exactly once"
    );
    assert_eq!(item.refresh, 1);
}

#[test]
fn drawer_set_draw_rect_replaces_previous_clip() -> Result<(), String> {
    unsafe {
        let screen = GetDC(ptr::null_mut());
        let dc = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, 100, 100);
        ReleaseDC(ptr::null_mut(), screen);
        let old = SelectObject(dc, bmp);
        PatBlt(dc, 0, 0, 100, 100, BLACKNESS);
        let mut d = drawer::Drawer::new(dc as usize);
        let p = &mut d as *mut _ as *mut c_void;
        call!(p, 2, (i32, i32, i32, i32), (0, 0, 20, 20), ());
        call!(p, 2, (i32, i32, i32, i32), (40, 40, 20, 20), ());
        call!(
            p,
            3,
            (i32, i32, i32, i32, u32, u8),
            (40, 40, 20, 20, 0xff, 255),
            ()
        );
        let pixel = GetPixel(dc, 45, 45);
        d.finish()?;
        SelectObject(dc, old);
        DeleteObject(bmp);
        DeleteDC(dc);
        assert_eq!(pixel, 0xff);
        Ok(())
    }
}
#[test]
fn discovery_retains_disabled_and_failed_entries_without_executing_invalid_dlls() {
    let temp = std::env::temp_dir().join(format!("tm-plugin-discovery-{}", std::process::id()));
    std::fs::create_dir_all(&temp).unwrap();
    std::fs::write(temp.join("disabled.DLL"), b"not a PE").unwrap();
    std::fs::write(temp.join("invalid.dll"), b"not a PE").unwrap();
    let mut m = PluginManager::new(HostInfo::default());
    m.discover(&temp, &["DISABLED.dll".into()]).unwrap();
    let infos = m.plugins();
    assert_eq!(infos.len(), 2);
    assert!(infos.iter().any(|i| !i.enabled && i.error.is_none()));
    assert!(infos.iter().any(|i| {
        i.enabled
            && i.error
                .as_ref()
                .is_some_and(|e| e.contains("LoadLibraryExW"))
    }));
    assert!(m.items().unwrap().is_empty());
    assert!(m.set_enabled("disabled.dll", true).is_err());
    assert!(m.plugins().iter().all(|i| i.error.is_some()));
    m.shutdown().unwrap();
    std::fs::remove_dir_all(temp).unwrap();
}
#[test]
fn host_rejects_wrong_thread_callback_before_touching_mutable_state() {
    let mut h = Host::new(HostInfo {
        main_window: 123,
        ..Default::default()
    });
    let address = &mut h as *mut _ as usize;
    let result = std::thread::spawn(move || -> Result<usize, String> {
        let p = address as *mut c_void;
        Ok(call!(p, 10, (), (), *mut c_void) as usize)
    })
    .join()
    .unwrap()
    .unwrap();
    assert_eq!(result, 0);
}
