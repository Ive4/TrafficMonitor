use traffic_monitor_windows::plugin_updates::*;

#[test]
fn version_components_compare_numerically_and_ignore_only_trailing_zeros() {
    assert!(PluginVersion::parse("1.10").unwrap() > PluginVersion::parse("1.9").unwrap());
    assert_eq!(
        PluginVersion::parse("v1.03.0").unwrap(),
        PluginVersion::parse("1.3").unwrap()
    );
    assert!(PluginVersion::parse("1.0.1").unwrap() > PluginVersion::parse("1").unwrap());
    for invalid in [
        "",
        "V",
        "1..2",
        "1.-2",
        "1.2beta3",
        "1/2",
        "1.4294967296",
        "1.2.3 trailing",
    ] {
        assert!(PluginVersion::parse(invalid).is_err(), "{invalid}");
    }
}
#[test]
fn official_shape_supports_unicode_and_case_insensitive_file_lookup() {
    let manifest=UpdateManifest::parse("<?xml version=\"1.0\" encoding=\"utf-8\"?><!-- manifest --><plugins><plugin file_name=\"Battery.dll\" version=\"1.03\"/><plugin file_name=\"时钟.dll\" version=\"1.2.3.4\"></plugin></plugins>").unwrap();
    assert_eq!(manifest.entries.len(), 2);
    assert_eq!(manifest.latest("BATTERY.DLL").unwrap().text, "1.03");
    assert!(manifest.latest("missing.dll").is_none());
}
#[test]
fn malformed_manifests_never_produce_partial_success() {
    for invalid in [
        "",
        "<wrong/>",
        "<plugins><plugin file_name=\"A.dll\" version=\"1\"/>",
        "<plugins><plugin file_name=\"A.dll\"/></plugins>",
        "<plugins><plugin file_name=\"../A.dll\" version=\"1\"/></plugins>",
        "<plugins><plugin file_name=\"C:\\A.dll\" version=\"1\"/></plugins>",
        "<plugins><plugin file_name=\"CON.dll\" version=\"1\"/></plugins>",
        "<plugins><plugin file_name=\"A.dll\" version=\"1\"/><plugin file_name=\"a.DLL\" version=\"2\"/></plugins>",
        "<plugins><plugin file_name=\"A.dll\" version=\"1\" download=\"https://example.com/a.exe\"/></plugins>",
        "<!DOCTYPE plugins [<!ENTITY x SYSTEM 'file:///x'>]><plugins/>",
        "<plugins>unexpected text</plugins>",
        "<plugins><plugin file_name=\"A.dll\" version=\"bad\"/></plugins>",
        "<plugins/><plugins/>",
    ] {
        assert!(
            UpdateManifest::parse(invalid).is_err(),
            "accepted {invalid}"
        );
    }
}
#[test]
fn checks_only_newer_installed_plugins_and_never_invents_missing_versions() {
    let manifest=UpdateManifest::parse("<plugins><plugin file_name=\"A.dll\" version=\"1.10\"/><plugin file_name=\"B.dll\" version=\"2.0\"/></plugins>").unwrap();
    let updates = manifest
        .updates(&[
            ("a.DLL".into(), "1.9".into()),
            ("B.dll".into(), "2.0.0".into()),
            ("NotListed.dll".into(), "3.0".into()),
        ])
        .unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].file_name, "a.DLL");
    assert_eq!(updates[0].latest, "1.10");
    assert!(
        manifest
            .updates(&[("A.dll".into(), "not a version".into())])
            .is_err()
    );
}
#[test]
fn manifest_size_and_version_component_limits_are_enforced() {
    assert!(UpdateManifest::parse(&" ".repeat(1024 * 1024 + 1)).is_err());
    assert!(PluginVersion::parse(&vec!["1"; 33].join(".")).is_err());
    assert!(
        UpdateManifest::parse("<plugins/>")
            .unwrap()
            .entries
            .is_empty()
    );
}
#[test]
fn update_sources_are_fixed_official_https_locations() {
    assert_eq!(
        manifest_url(0).unwrap(),
        "https://raw.githubusercontent.com/zhongyang219/TrafficMonitorPlugins/main/plugins_version.xml"
    );
    assert!(
        manifest_url(1)
            .unwrap()
            .starts_with("https://gitee.com/zhongyang219/")
    );
    assert!(manifest_url(9).is_err());
}
#[cfg(windows)]
#[test]
#[ignore = "Explicit live HTTPS verification; not a deterministic CI test"]
fn live_official_manifest_check_is_read_only() {
    let manifest = fetch_manifest(0).unwrap();
    assert!(manifest.latest("Battery.dll").is_some());
}
