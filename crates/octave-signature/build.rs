#[path = "../../build-support/windows-resource.rs"]
mod windows_resource;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows_resource::compile("octave-signature", "Octave contained playback worker")
}
