use std::path::PathBuf;
pub fn data_directory() -> Option<PathBuf> {
    default_for(
        std::env::consts::OS,
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
    )
}
fn default_for(
    os: &str,
    local: Option<PathBuf>,
    home: Option<PathBuf>,
    xdg: Option<PathBuf>,
) -> Option<PathBuf> {
    match os {
        "windows" => local.map(|p| p.join("Octave")),
        "macos" => home.map(|p| p.join("Library/Application Support/Octave")),
        _ => xdg
            .filter(|p| p.is_absolute())
            .or_else(|| home.map(|p| p.join(".local/share")))
            .map(|p| p.join("octave")),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn platform_paths_and_relative_xdg_fallback() {
        let base = std::env::temp_dir();
        assert_eq!(
            default_for("windows", Some(base.clone()), None, None),
            Some(base.join("Octave"))
        );
        assert_eq!(
            default_for("macos", None, Some(base.clone()), None),
            Some(base.join("Library/Application Support/Octave"))
        );
        assert_eq!(
            default_for("linux", None, None, Some(base.clone())),
            Some(base.join("octave"))
        );
        assert_eq!(
            default_for("linux", None, Some(base.clone()), Some("relative".into())),
            Some(base.join(".local/share/octave"))
        );
        assert_eq!(default_for("windows", None, Some(base), None), None);
    }
}
