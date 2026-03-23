use anyhow::Error;
use std::sync::{Arc, Mutex};
use std::io::Cursor;
use base64::Engine;
use image::{ImageFormat, ImageReader};
use serde::{Serialize, Deserialize};
use onetagger_tagger::audiofeatures_provider::*;


lazy_static::lazy_static! {
    /// Global registry of all audio features providers
    pub static ref AF_PROVIDERS: Arc<Mutex<AFProviderRegistry>> =
        Arc::new(Mutex::new(AFProviderRegistry::builtin()));
}


/// Registry of available audio features providers.
pub struct AFProviderRegistry {
    pub providers: Vec<AFProviderEntry>,
}

/// A registered provider with its serializable info and the builder instance.
pub struct AFProviderEntry {
    pub info: AFProviderInfoSerialized,
    pub builder: Box<dyn AFProviderBuilder + Send + Sync>,
}

/// Provider info prepared for sending to the UI (icon as base64 data URL).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AFProviderInfoSerialized {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    /// Base64-encoded PNG data URL for the icon
    pub icon: String,
    /// Capabilities — what this provider can do
    pub capabilities: AFProviderCapabilities,
}

impl AFProviderRegistry {
    /// Create the registry with all built-in providers.
    fn builtin() -> Self {
        let mut providers = vec![];

        // AudioMuse-AI — local, free, feature-rich
        Self::add_builtin::<onetagger_platforms::audiomuse::AudioMuseBuilder>(&mut providers);

        // Spotify — requires Premium, existing functionality preserved
        // TODO: Refactor existing Spotify AF code into SpotifyAFBuilder and add here
        // Self::add_builtin::<onetagger_platforms::spotify_af::SpotifyAFBuilder>(&mut providers);

        AFProviderRegistry { providers }
    }

    /// Reload the registry (e.g., after plugin changes).
    pub fn reload(&mut self) {
        *self = Self::builtin();
    }

    /// Look up a provider builder by ID.
    pub fn get_builder(&mut self, id: &str) -> Option<&mut Box<dyn AFProviderBuilder + Send + Sync>> {
        let entry = self.providers.iter_mut()
            .find(|p| p.info.id == id)?;
        Some(&mut entry.builder)
    }

    /// Get serializable info for all providers (for sending to the UI).
    pub fn provider_list(&self) -> Vec<AFProviderInfoSerialized> {
        self.providers.iter().map(|p| p.info.clone()).collect()
    }

    /// Register a built-in provider.
    fn add_builtin<P: AFProviderBuilder>(output: &mut Vec<AFProviderEntry>) {
        let builder = P::new();
        let info = builder.info();
        let capabilities = builder.capabilities();

        let icon_b64 = match Self::encode_icon(info.icon) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to encode icon for AF provider '{}': {}", info.id, e);
                String::new()
            }
        };

        output.push(AFProviderEntry {
            info: AFProviderInfoSerialized {
                id: info.id,
                name: info.name,
                description: info.description,
                version: info.version,
                icon: icon_b64,
                capabilities,
            },
            builder: Box::new(builder),
        });
    }

    /// Encode icon bytes as a base64 data URL (PNG).
    fn encode_icon(data: &[u8]) -> Result<String, Error> {
        let img = ImageReader::new(Cursor::new(data)).with_guessed_format()?.decode()?;
        let mut buf = vec![];
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;
        Ok(format!(
            "data:image/png;charset=utf-8;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(buf)
        ))
    }
}
