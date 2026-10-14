//! Maintainer verification helper. No automatic feed or installer execution.
fn main() {
    if let Err(error) = run() {
        eprintln!("update_check_failed: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 && args.len() != 5 {
        return Err("Usage: octave-update PUBLIC_KEY_HEX_FILE METADATA_JSON SIGNATURE_HEX_FILE INSTALLED_SEQUENCE [DOWNLOAD_DIRECTORY]".into());
    }
    let read = |path: &str| -> Result<Vec<u8>, &'static str> {
        use std::io::Read;
        let file =
            std::fs::File::open(path).map_err(|_| "Unable to read update verification input")?;
        let mut bytes = Vec::new();
        file.take(16385)
            .read_to_end(&mut bytes)
            .map_err(|_| "Unable to read update verification input")?;
        Ok(bytes)
    };
    let key = read(&args[0])?;
    let signature = read(&args[2])?;
    let key = octave_update::decode_hex::<32>(std::str::from_utf8(&key)?.trim())?;
    let signature = octave_update::decode_hex::<64>(std::str::from_utf8(&signature)?.trim())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let release = octave_update::verify(&read(&args[1])?, &signature, &key, args[3].parse()?, now)?;
    println!(
        "verified_version={} sequence={}",
        release.version(),
        release.sequence()
    );
    if let Some(directory) = args.get(4) {
        release.download(std::path::Path::new(directory))?;
        println!("payload_verified=true");
    }
    Ok(())
}
