// Embeds icon, version info and manifest (asInvoker, per-monitor DPI, UTF-8 ACP, common controls v6) on Windows.
fn main() {
    println!("cargo:rerun-if-changed=assets/nextmon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    // rust-embed needs the folder to exist at compile time (the UI is built by someone else and may be absent).
    let ui = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join("../../ui");
    let _ = std::fs::create_dir_all(&ui);
    println!("cargo:rerun-if-changed={}", ui.display());
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let ver = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".into());
    let mut parts: Vec<u64> = ver.split('.').filter_map(|p| p.parse().ok()).collect();
    parts.resize(4, 0);
    let packed = (parts[0] << 48) | (parts[1] << 32) | (parts[2] << 16) | parts[3];
    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/nextmon.ico");
    res.set("ProductName", "NextMON Server");
    res.set("FileDescription", "NextMON Server");
    res.set("CompanyName", "NextMON");
    res.set("LegalCopyright", "NextMON");
    res.set("OriginalFilename", "NextMON Server.exe");
    res.set("InternalName", "nextmon-server");
    res.set_version_info(winres::VersionInfo::FILEVERSION, packed);
    res.set_version_info(winres::VersionInfo::PRODUCTVERSION, packed);
    res.set("FileVersion", &ver);
    res.set("ProductVersion", &ver);
    res.set_manifest(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security>
  </trustInfo>
  <dependency><dependentAssembly>
    <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
  </dependentAssembly></dependency>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1"><application>
    <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
  </application></compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3"><windowsSettings>
    <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
    <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2,PerMonitor</dpiAwareness>
    <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
  </windowsSettings></application>
</assembly>"#,
    );
    if let Err(e) = res.compile() {
        println!("cargo:warning=resource compile failed: {e}");
    }
}
