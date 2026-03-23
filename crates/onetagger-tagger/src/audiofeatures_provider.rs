use anyhow::Error;
use std::any::Any;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use serde::{Serialize, Deserialize};
use serde_json::Value;

use crate::{AudioFileInfo, FrameName, AudioFileFormat, ConfigCallbackResponse};


// ============================================================================
// Provider Capabilities & Metadata
// ============================================================================

/// Describes what an audio features provider can do.
/// The UI reads this to dynamically render the appropriate controls.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AFProviderCapabilities {
    /// The audio features this provider can return
    pub features: Vec<AFFeatureDescriptor>,
    /// Can this provider find tracks similar to a given track?
    pub similarity_search: bool,
    /// Can this provider generate playlists?
    pub playlist_generation: bool,
    /// Can this provider create a "song path" between two tracks?
    pub song_paths: bool,
    /// Does this provider require authentication/login?
    pub requires_auth: bool,
    /// Does this provider depend on an external service being available?
    pub requires_external_service: bool,
    /// Can this provider also write BPM?
    pub provides_bpm: bool,
    /// Can this provider also write key/scale?
    pub provides_key: bool,
    /// Can this provider return genre/mood predictions?
    pub provides_genre: bool,
    /// Can this provider return mood predictions (separate from genre)?
    pub provides_mood: bool,
}

/// Describes a single audio feature a provider can return.
/// Used by the UI to dynamically build the properties panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AFFeatureDescriptor {
    /// Internal key, e.g. "danceability", "energy", "aggression"
    pub id: String,
    /// Display name for the UI, e.g. "Danceability", "Energy"
    pub name: String,
    /// Default tag frame name prefix (will have format-specific variants)
    pub default_tag: String,
    /// Min/max of the raw values from the provider (for normalization to 0-100)
    pub raw_value_min: f64,
    pub raw_value_max: f64,
    /// Default threshold range for prominent-tag classification (0-100 scale)
    pub default_threshold_min: i8,
    pub default_threshold_max: i8,
    /// Labels for the prominent tag: (below_min, between, above_max)
    pub label_low: String,
    pub label_mid: String,
    pub label_high: String,
}

/// Metadata about a provider, shown in the UI provider selector.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AFProviderInfo {
    /// Unique identifier, e.g. "spotify", "audiomuse"
    pub id: String,
    /// Display name, e.g. "Spotify", "AudioMuse-AI"
    pub name: String,
    /// Description shown in UI
    pub description: String,
    /// SemVer version string
    pub version: String,
    /// Icon bytes (PNG recommended, 1:1 aspect ratio) — skipped in serialization
    #[serde(skip)]
    pub icon: &'static [u8],
}


// ============================================================================
// Provider Results
// ============================================================================

/// The result of fetching audio features for a single track.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AFTrackResult {
    /// The provider's internal track ID (for subsequent API calls)
    pub provider_track_id: String,
    /// Feature values normalized to 0-100 (keyed by AFFeatureDescriptor.id)
    pub features: HashMap<String, i8>,
    /// Prominent tag labels (keyed by AFFeatureDescriptor.id, only for features in range)
    pub prominent_tags: HashMap<String, String>,
    /// Optional BPM value
    pub bpm: Option<f64>,
    /// Optional musical key, e.g. "Dm", "G", "F#m"
    pub key: Option<String>,
    /// Optional genre predictions (label, confidence 0.0-1.0), ordered by confidence
    pub genres: Vec<(String, f64)>,
    /// Optional mood predictions (label, confidence 0.0-1.0), ordered by confidence
    pub moods: Vec<(String, f64)>,
    /// Any extra provider-specific data (passed through but not interpreted)
    pub extra: HashMap<String, String>,
}

/// A track returned from similarity search.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimilarTrack {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Distance/dissimilarity score (lower = more similar, provider-specific scale)
    pub distance: f64,
    /// The provider's internal track ID
    pub provider_track_id: String,
}

/// Result of a sync operation — matching local files against a provider's catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    /// Successfully matched: (local path, provider track ID)
    pub matched: Vec<(PathBuf, String)>,
    /// Local files with no match in the provider
    pub unmatched_local: Vec<PathBuf>,
    /// Provider tracks with no matching local file (orphans in provider DB)
    pub unmatched_provider: Vec<(String, String, String)>, // (id, title, artist)
    /// Total tracks in the provider's catalog
    pub provider_total: usize,
    /// Total local files scanned
    pub local_total: usize,
}


// ============================================================================
// Provider Configuration
// ============================================================================

/// User-configurable settings for the audio features tagging run.
/// Provider-specific config is stored in `provider_config` as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AFConfig {
    /// Which provider to use
    pub provider_id: String,
    /// Path to the folder of audio files to tag
    pub path: Option<PathBuf>,
    /// Tag frame for the combined prominent-tag output
    pub main_tag: FrameName,
    /// Per-feature configuration (keyed by feature ID)
    pub feature_config: HashMap<String, AFFeatureConfig>,
    /// Whether to write a meta tag with timestamp
    pub meta_tag: bool,
    /// Skip files that have already been tagged by this provider
    pub skip_tagged: bool,
    /// Include subfolders when scanning
    pub include_subfolders: bool,
    /// Whether to write BPM tag (if provider supports it)
    pub write_bpm: bool,
    /// Whether to write key tag (if provider supports it)
    pub write_key: bool,
    /// Whether to write genre tag from provider's genre predictions
    pub write_genre: bool,
    /// How many genre labels to write (e.g., top 3)
    pub genre_count: usize,
    /// Whether to write mood tag from provider's mood predictions
    pub write_mood: bool,
    /// How many mood labels to write
    pub mood_count: usize,
    /// Tag frame for genre output
    pub genre_tag: FrameName,
    /// Tag frame for mood output
    pub mood_tag: FrameName,
    /// Provider-specific configuration blob
    pub provider_config: Value,
    /// Separators config (carried over from existing system)
    pub separators: crate::TagSeparators,
}

/// Per-feature config: whether it's enabled, which tag to write to, threshold range.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AFFeatureConfig {
    /// Include this feature in tagging
    pub enabled: bool,
    /// Tag frame to write the numeric value to
    pub tag: FrameName,
    /// Threshold range for prominent tag (min, max on 0-100 scale)
    pub threshold_min: i8,
    pub threshold_max: i8,
}


// ============================================================================
// Provider Traits
// ============================================================================

/// Builder for creating provider instances. Each provider module implements this.
/// Mirrors the pattern of `AutotaggerSourceBuilder`.
pub trait AFProviderBuilder: Any + Send + Sync {
    /// Constructor
    fn new() -> Self where Self: Sized;

    /// Get metadata about this provider
    fn info(&self) -> AFProviderInfo;

    /// Get this provider's capabilities (features, similarity, etc.)
    fn capabilities(&self) -> AFProviderCapabilities;

    /// Create an active provider instance from config.
    /// The config blob is provider-specific (connection URL, credentials, etc.)
    fn get_provider(&mut self, config: &Value) -> Result<Box<dyn AFProvider>, Error>;

    /// Handle a callback from the UI (e.g., test connection, OAuth flow)
    fn config_callback(&mut self, _name: &str, _config: Value) -> ConfigCallbackResponse {
        ConfigCallbackResponse::Empty
    }
}

/// The active provider — does the actual work of fetching features, finding
/// similar tracks, etc. Created by `AFProviderBuilder::get_provider()`.
pub trait AFProvider: Any + Send + Sync {
    /// Fetch the full catalog listing from this provider.
    /// Returns (provider_track_id, title, artist) tuples.
    /// Used during the sync step to build the local-to-provider mapping.
    fn fetch_catalog(&mut self) -> Result<Vec<(String, String, String)>, Error>;

    /// Get audio features for a track identified by its provider track ID.
    /// The ID comes from a previous sync/match step cached in SQLite.
    fn get_features(&mut self, provider_track_id: &str) -> Result<AFTrackResult, Error>;

    /// Find tracks similar to the given track. Optional capability.
    /// Returns an error if the provider doesn't support similarity search.
    fn find_similar(&mut self, provider_track_id: &str, count: usize)
        -> Result<Vec<SimilarTrack>, Error>
    {
        Err(anyhow!("Similarity search not supported by this provider"))
    }

    /// Generate a "song path" — a playlist that transitions from one track to another.
    /// Optional capability.
    fn find_path(
        &mut self,
        from_provider_id: &str,
        to_provider_id: &str,
        steps: usize,
    ) -> Result<Vec<SimilarTrack>, Error> {
        Err(anyhow!("Song paths not supported by this provider"))
    }

    /// Check whether the provider's external service is reachable and working.
    /// Returns Ok(version_string) on success, Err on failure.
    fn test_connection(&mut self) -> Result<String, Error>;
}
