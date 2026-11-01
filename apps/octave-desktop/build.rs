use std::{env, error::Error};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-env-changed=OCTAVE_BUILD_VERSION");
    let version = env::var("OCTAVE_BUILD_VERSION")
        .unwrap_or_else(|_| env::var("CARGO_PKG_VERSION").expect("Cargo package version"));
    let parsed = semver::Version::parse(&version)?;
    let major = u16::try_from(parsed.major)?;
    let minor = u16::try_from(parsed.minor)?;
    let patch = u16::try_from(parsed.patch)?;
    let numeric = (u64::from(major) << 48) | (u64::from(minor) << 32) | (u64::from(patch) << 16);

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resources = winresource::WindowsResource::new();
        let mut flags = 0;
        if !parsed.pre.is_empty() {
            flags |= winresource::VersionInfo::VS_FF_PRERELEASE;
        }
        if env::var("PROFILE").as_deref() == Ok("debug") {
            flags |= winresource::VersionInfo::VS_FF_DEBUG;
        }
        resources
            .set_language(0x0409)
            .set("ProductName", "Octave")
            .set("FileDescription", "Octave desktop music client")
            .set("CompanyName", "Octave contributors")
            .set("LegalCopyright", "Copyright (c) 2026 Octave contributors")
            .set("OriginalFilename", "octave-desktop.exe")
            .set("InternalName", "octave-desktop")
            .set("FileVersion", &version)
            .set("ProductVersion", &version)
            .set_version_info(winresource::VersionInfo::FILEVERSION, numeric)
            .set_version_info(winresource::VersionInfo::PRODUCTVERSION, numeric)
            .set_version_info(winresource::VersionInfo::FILEFLAGS, flags);
        resources.compile()?;
    }
    Ok(())
}
