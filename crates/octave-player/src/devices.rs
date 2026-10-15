//! Output enumeration is called on a worker, never the UI/audio callback.
use rodio::cpal::traits::{DeviceTrait, HostTrait};

#[derive(Clone)]
pub struct OutputDevice {
    pub(crate) device: rodio::cpal::Device,
    name: String,
}
impl OutputDevice {
    pub fn name(&self) -> &str {
        &self.name
    }
}
pub fn output_devices() -> Result<Vec<OutputDevice>, super::Failure> {
    let devices = rodio::cpal::default_host().output_devices().map_err(|_| {
        super::Failure::Audio("Audio outputs could not be listed. Try Refresh outputs.")
    })?;
    Ok(devices
        .take(64)
        .map(|device| {
            let name = device
                .description()
                .map(|description| description.name().to_owned())
                .unwrap_or_else(|_| "Unnamed output".into());
            OutputDevice { device, name }
        })
        .collect())
}
