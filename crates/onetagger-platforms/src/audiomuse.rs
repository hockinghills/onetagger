use anyhow::Error;
use std::collections::HashMap;
use std::time::Duration;
use serde::{Serialize, Deserialize};
use serde_json::Value;
use onetagger_tagger::audiofeatures_provider::*;
use onetagger_tagger::ConfigCallbackResponse;


// ============================================================================
// AudioMuse Configuration
// ============================================================================

/// Provider-specific configuration for AudioMuse-AI.
/// This is what gets stored in AFConfig.provider_config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioMuseConfig {
    /// Base URL of the AudioMuse-AI instance, e.g. "http://localhost:8500"
    pub api_url: String,
    /// Optional Bearer token (if AudioMuse has auth enabled)
    pub api_token: Option<String>,
}

impl Default for AudioMuseConfig {
    fn default() -> Self {
        Self {
            api_url: "http://localhost:8000".to_string(),
            api_token: None,
        }
    }
}


// ============================================================================
// API Response Types
// ============================================================================

/// Score record from /external/get_score
#[derive(Debug, Clone, Deserialize)]
pub struct AudioMuseScore {
    pub item_id: String,
    pub title: Option<String>,
    pub author: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub tempo: Option<f64>,
    pub key: Option<String>,
    pub scale: Option<String>,
    pub energy: Option<f64>,
    pub mood_vector: Option<String>,
    pub other_features: Option<String>,
    pub file_path: Option<String>,
    pub year: Option<i32>,
    pub rating: Option<i32>,
}

/// Map item from /api/map (used for catalog sync)
#[derive(Debug, Clone, Deserialize)]
pub struct AudioMuseMapItem {
    pub item_id: String,
    pub title: String,
    pub artist: String,
    // embedding_2d and mood_vector also present but we don't need them for sync
}

/// Map response wrapper
#[derive(Debug, Clone, Deserialize)]
pub struct AudioMuseMapResponse {
    pub items: Vec<AudioMuseMapItem>,
    #[serde(default)]
    pub projection: Option<String>,
}

/// Similar track from /api/similar_tracks
#[derive(Debug, Clone, Deserialize)]
pub struct AudioMuseSimilarTrack {
    pub item_id: String,
    pub title: String,
    pub author: String,
    #[serde(default)]
    pub album: Option<String>,
    pub distance: f64,
}


// ============================================================================
// Parsing Helpers
// ============================================================================

/// Parse a comma-separated "key:value" string like
/// "danceable:0.57,aggressive:0.03,happy:0.15,party:0.03,relaxed:0.96,sad:0.77"
fn parse_kv_string(s: &str) -> Vec<(String, f64)> {
    s.split(',')
        .filter_map(|pair| {
            let parts: Vec<&str> = pair.splitn(2, ':').collect();
            if parts.len() == 2 {
                let key = parts[0].trim().to_string();
                let val = parts[1].trim().parse::<f64>().ok()?;
                Some((key, val))
            } else {
                None
            }
        })
        .collect()
}

/// Labels from mood_vector that should be treated as genres rather than moods.
/// Everything else from mood_vector is treated as a mood tag.
const GENRE_LABELS: &[&str] = &[
    "rock", "pop", "alternative", "indie", "electronic", "dance", "jazz", "metal",
    "classic rock", "soul", "indie rock", "electronica", "folk", "punk", "blues",
    "hard rock", "ambient", "acoustic", "experimental", "Hip-Hop", "country", "funk",
    "electro", "heavy metal", "Progressive rock", "rnb", "indie pop", "House",
    "alternative rock",
];

/// Labels from mood_vector that are decade tags (not genre or mood).
const DECADE_LABELS: &[&str] = &["00s", "80s", "90s", "70s", "60s"];

/// Classify mood_vector entries into genres, moods, and decades.
fn classify_mood_vector(entries: &[(String, f64)]) -> (Vec<(String, f64)>, Vec<(String, f64)>) {
    let mut genres = vec![];
    let mut moods = vec![];

    for (label, score) in entries {
        let label_lower = label.to_lowercase();
        if DECADE_LABELS.iter().any(|d| d.to_lowercase() == label_lower) {
            // Skip decade labels — they don't belong in genre or mood
            continue;
        }
        if GENRE_LABELS.iter().any(|g| g.to_lowercase() == label_lower) {
            genres.push((label.clone(), *score));
        } else {
            moods.push((label.clone(), *score));
        }
    }

    genres.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    moods.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    (genres, moods)
}

/// Normalize a 0.0-1.0 float to a 0-100 i8 value.
fn normalize_to_100(val: f64, min: f64, max: f64) -> i8 {
    let clamped = ((val - min) / (max - min)).clamp(0.0, 1.0);
    (clamped * 100.0) as i8
}

/// Format key + scale into a standard notation like "Dm", "G", "F#m".
fn format_key(key: &str, scale: &str) -> String {
    match scale.to_lowercase().as_str() {
        "minor" => format!("{}m", key),
        _ => key.to_string(),
    }
}


// ============================================================================
// HTTP Client
// ============================================================================

/// Simple HTTP client wrapper for AudioMuse-AI REST API.
struct AudioMuseClient {
    base_url: String,
    token: Option<String>,
    client: reqwest::blocking::Client,
}

impl AudioMuseClient {
    fn new(config: &AudioMuseConfig) -> Result<Self, Error> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        // Strip trailing slash from URL
        let base_url = config.api_url.trim_end_matches('/').to_string();

        Ok(Self {
            base_url,
            token: config.api_token.clone(),
            client,
        })
    }

    /// Make a GET request with optional auth header.
    fn get(&self, path: &str) -> Result<reqwest::blocking::Response, Error> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.get(&url);

        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let resp = req.send()?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "AudioMuse API error: {} {} returned {}",
                "GET", path, resp.status()
            ));
        }
        Ok(resp)
    }

    /// Fetch the full track catalog via the map endpoint.
    fn fetch_catalog(&self) -> Result<Vec<AudioMuseMapItem>, Error> {
        let resp = self.get("/api/map?percent=100")?;
        let map_resp: AudioMuseMapResponse = resp.json()?;
        Ok(map_resp.items)
    }

    /// Fetch the score (analysis data) for a single track by ID.
    fn get_score(&self, item_id: &str) -> Result<AudioMuseScore, Error> {
        let resp = self.get(&format!("/external/get_score?id={}", item_id))?;
        let score: AudioMuseScore = resp.json()?;
        Ok(score)
    }

    /// Find similar tracks via the similarity API.
    fn similar_tracks(&self, item_id: &str, count: usize) -> Result<Vec<AudioMuseSimilarTrack>, Error> {
        let resp = self.get(&format!(
            "/api/similar_tracks?item_id={}&n={}",
            item_id, count
        ))?;
        let tracks: Vec<AudioMuseSimilarTrack> = resp.json()?;
        Ok(tracks)
    }

    /// Test connectivity — try to reach the API and return version info.
    fn test_connection(&self) -> Result<String, Error> {
        // The map cache status endpoint is lightweight and confirms the API is up
        let resp = self.get("/api/map_cache_status")?;
        let body: Value = resp.json()?;
        Ok(format!("AudioMuse-AI connected (cache status: {:?})",
            body.get("status").and_then(|v| v.as_str()).unwrap_or("unknown")
        ))
    }
}


// ============================================================================
// AudioMuse Provider Implementation
// ============================================================================

/// The active AudioMuse provider instance.
pub struct AudioMuseProvider {
    client: AudioMuseClient,
}

impl AFProvider for AudioMuseProvider {
    fn fetch_catalog(&mut self) -> Result<Vec<(String, String, String)>, Error> {
        info!("[AudioMuse] Fetching full catalog from map endpoint...");
        let items = self.client.fetch_catalog()?;
        info!("[AudioMuse] Received {} tracks from catalog", items.len());

        Ok(items
            .into_iter()
            .map(|item| (item.item_id, item.title, item.artist))
            .collect())
    }

    fn get_features(&mut self, provider_track_id: &str) -> Result<AFTrackResult, Error> {
        let score = self.client.get_score(provider_track_id)?;

        let mut features = HashMap::new();
        let prominent_tags = HashMap::new();
        let mut extra = HashMap::new();

        // Parse other_features → numeric feature values
        if let Some(ref of_str) = score.other_features {
            let of_entries = parse_kv_string(of_str);
            for (key, val) in &of_entries {
                let (feature_id, normalized) = match key.as_str() {
                    "danceable" => ("danceability", normalize_to_100(*val, 0.0, 1.0)),
                    "aggressive" => ("aggression", normalize_to_100(*val, 0.0, 1.0)),
                    "happy" => ("happiness", normalize_to_100(*val, 0.0, 1.0)),
                    "party" => ("party", normalize_to_100(*val, 0.0, 1.0)),
                    "relaxed" => ("relaxation", normalize_to_100(*val, 0.0, 1.0)),
                    "sad" => ("sadness", normalize_to_100(*val, 0.0, 1.0)),
                    _ => continue,
                };
                features.insert(feature_id.to_string(), normalized);
            }
        }

        // Energy from score.energy
        if let Some(energy) = score.energy {
            // AudioMuse energy is RMS-based, typically 0.0-0.5 range
            // We normalize to 0-100 with a practical max of ~0.5
            let normalized = normalize_to_100(energy, 0.0, 0.5);
            features.insert("energy".to_string(), normalized);
        }

        // BPM
        let bpm = score.tempo;

        // Key
        let key = match (score.key.as_ref(), score.scale.as_ref()) {
            (Some(k), Some(s)) => Some(format_key(k, s)),
            (Some(k), None) => Some(k.clone()),
            _ => None,
        };

        // Parse mood_vector → genres and moods
        let (genres, moods) = if let Some(ref mv_str) = score.mood_vector {
            let mv_entries = parse_kv_string(mv_str);
            classify_mood_vector(&mv_entries)
        } else {
            (vec![], vec![])
        };

        // Store raw mood_vector and other_features in extra for reference
        if let Some(mv) = &score.mood_vector {
            extra.insert("raw_mood_vector".to_string(), mv.clone());
        }
        if let Some(of) = &score.other_features {
            extra.insert("raw_other_features".to_string(), of.clone());
        }

        Ok(AFTrackResult {
            provider_track_id: score.item_id,
            features,
            prominent_tags,  // Filled in by the tagging engine based on thresholds
            bpm,
            key,
            genres,
            moods,
            extra,
        })
    }

    fn find_similar(&mut self, provider_track_id: &str, count: usize) -> Result<Vec<SimilarTrack>, Error> {
        let tracks = self.client.similar_tracks(provider_track_id, count)?;
        Ok(tracks
            .into_iter()
            .map(|t| SimilarTrack {
                title: t.title,
                artist: t.author,
                album: t.album.unwrap_or_default(),
                distance: t.distance,
                provider_track_id: t.item_id,
            })
            .collect())
    }

    fn test_connection(&mut self) -> Result<String, Error> {
        self.client.test_connection()
    }
}


// ============================================================================
// AudioMuse Provider Builder
// ============================================================================

/// Builder that creates AudioMuseProvider instances.
/// Registered in the provider registry alongside SpotifyAFBuilder.
pub struct AudioMuseBuilder;

impl AFProviderBuilder for AudioMuseBuilder {
    fn new() -> Self {
        AudioMuseBuilder
    }

    fn info(&self) -> AFProviderInfo {
        AFProviderInfo {
            id: "audiomuse".to_string(),
            name: "AudioMuse-AI".to_string(),
            description: "Local AI-powered audio analysis via AudioMuse-AI. \
                Provides energy, danceability, mood, genre, BPM, key detection, \
                and similarity search — all running locally with no API costs."
                .to_string(),
            version: "0.1.0".to_string(),
            icon: include_bytes!("../assets/audiomuse.png"),
        }
    }

    fn capabilities(&self) -> AFProviderCapabilities {
        AFProviderCapabilities {
            features: vec![
                AFFeatureDescriptor {
                    id: "energy".to_string(),
                    name: "Energy".to_string(),
                    default_tag: "AM_ENERGY".to_string(),
                    raw_value_min: 0.0,
                    raw_value_max: 0.5,
                    default_threshold_min: 20,
                    default_threshold_max: 80,
                    label_low: "#energy-low".to_string(),
                    label_mid: "#energy-med".to_string(),
                    label_high: "#energy-high".to_string(),
                },
                AFFeatureDescriptor {
                    id: "danceability".to_string(),
                    name: "Danceability".to_string(),
                    default_tag: "AM_DANCEABILITY".to_string(),
                    raw_value_min: 0.0,
                    raw_value_max: 1.0,
                    default_threshold_min: 20,
                    default_threshold_max: 80,
                    label_low: "#dance-low".to_string(),
                    label_mid: "#dance-med".to_string(),
                    label_high: "#dance-high".to_string(),
                },
                AFFeatureDescriptor {
                    id: "happiness".to_string(),
                    name: "Happiness".to_string(),
                    default_tag: "AM_HAPPINESS".to_string(),
                    raw_value_min: 0.0,
                    raw_value_max: 1.0,
                    default_threshold_min: 20,
                    default_threshold_max: 80,
                    label_low: "#somber".to_string(),
                    label_mid: "#balanced".to_string(),
                    label_high: "#happy".to_string(),
                },
                AFFeatureDescriptor {
                    id: "sadness".to_string(),
                    name: "Sadness".to_string(),
                    default_tag: "AM_SADNESS".to_string(),
                    raw_value_min: 0.0,
                    raw_value_max: 1.0,
                    default_threshold_min: 20,
                    default_threshold_max: 80,
                    label_low: "#upbeat".to_string(),
                    label_mid: "".to_string(),
                    label_high: "#melancholy".to_string(),
                },
                AFFeatureDescriptor {
                    id: "aggression".to_string(),
                    name: "Aggression".to_string(),
                    default_tag: "AM_AGGRESSION".to_string(),
                    raw_value_min: 0.0,
                    raw_value_max: 1.0,
                    default_threshold_min: 20,
                    default_threshold_max: 80,
                    label_low: "#gentle".to_string(),
                    label_mid: "".to_string(),
                    label_high: "#aggressive".to_string(),
                },
                AFFeatureDescriptor {
                    id: "relaxation".to_string(),
                    name: "Relaxation".to_string(),
                    default_tag: "AM_RELAXATION".to_string(),
                    raw_value_min: 0.0,
                    raw_value_max: 1.0,
                    default_threshold_min: 20,
                    default_threshold_max: 80,
                    label_low: "#tense".to_string(),
                    label_mid: "".to_string(),
                    label_high: "#relaxed".to_string(),
                },
                AFFeatureDescriptor {
                    id: "party".to_string(),
                    name: "Party".to_string(),
                    default_tag: "AM_PARTY".to_string(),
                    raw_value_min: 0.0,
                    raw_value_max: 1.0,
                    default_threshold_min: 20,
                    default_threshold_max: 80,
                    label_low: "#intimate".to_string(),
                    label_mid: "".to_string(),
                    label_high: "#party".to_string(),
                },
            ],
            similarity_search: true,
            playlist_generation: true,
            song_paths: true,
            requires_auth: false,
            requires_external_service: true,
            provides_bpm: true,
            provides_key: true,
            provides_genre: true,
            provides_mood: true,
        }
    }

    fn get_provider(&mut self, config: &Value) -> Result<Box<dyn AFProvider>, Error> {
        let am_config: AudioMuseConfig = serde_json::from_value(config.clone())
            .unwrap_or_default();
        let client = AudioMuseClient::new(&am_config)?;
        Ok(Box::new(AudioMuseProvider { client }))
    }

    fn config_callback(&mut self, name: &str, config: Value) -> ConfigCallbackResponse {
        match name {
            "testConnection" => {
                let am_config: AudioMuseConfig = match serde_json::from_value(config) {
                    Ok(c) => c,
                    Err(e) => return ConfigCallbackResponse::Error {
                        error: format!("Invalid config: {}", e)
                    },
                };
                match AudioMuseClient::new(&am_config) {
                    Ok(client) => {
                        match client.test_connection() {
                            Ok(msg) => ConfigCallbackResponse::UpdateConfig {
                                config: serde_json::json!({"status": "connected", "message": msg})
                            },
                            Err(e) => ConfigCallbackResponse::Error {
                                error: format!("Connection failed: {}", e)
                            },
                        }
                    },
                    Err(e) => ConfigCallbackResponse::Error {
                        error: format!("Client creation failed: {}", e)
                    },
                }
            },
            _ => ConfigCallbackResponse::Empty,
        }
    }
}


// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_kv_string() {
        let input = "danceable:0.57,aggressive:0.03,happy:0.15,party:0.03,relaxed:0.96,sad:0.77";
        let parsed = parse_kv_string(input);
        assert_eq!(parsed.len(), 6);
        assert_eq!(parsed[0], ("danceable".to_string(), 0.57));
        assert_eq!(parsed[5], ("sad".to_string(), 0.77));
    }

    #[test]
    fn test_parse_mood_vector() {
        let input = "jazz:0.563,female vocalists:0.561,electronic:0.529,pop:0.526,chillout:0.525";
        let parsed = parse_kv_string(input);
        assert_eq!(parsed.len(), 5);
        assert_eq!(parsed[0], ("jazz".to_string(), 0.563));
    }

    #[test]
    fn test_classify_mood_vector() {
        let entries = vec![
            ("jazz".to_string(), 0.563),
            ("female vocalists".to_string(), 0.561),
            ("electronic".to_string(), 0.529),
            ("pop".to_string(), 0.526),
            ("chillout".to_string(), 0.525),
            ("80s".to_string(), 0.400),
        ];
        let (genres, moods) = classify_mood_vector(&entries);
        // jazz, electronic, pop should be genres
        assert!(genres.iter().any(|(l, _)| l == "jazz"));
        assert!(genres.iter().any(|(l, _)| l == "electronic"));
        assert!(genres.iter().any(|(l, _)| l == "pop"));
        // female vocalists, chillout should be moods
        assert!(moods.iter().any(|(l, _)| l == "female vocalists"));
        assert!(moods.iter().any(|(l, _)| l == "chillout"));
        // 80s should be excluded (decade label)
        assert!(!genres.iter().any(|(l, _)| l == "80s"));
        assert!(!moods.iter().any(|(l, _)| l == "80s"));
    }

    #[test]
    fn test_normalize_to_100() {
        assert_eq!(normalize_to_100(0.0, 0.0, 1.0), 0);
        assert_eq!(normalize_to_100(1.0, 0.0, 1.0), 100);
        assert_eq!(normalize_to_100(0.5, 0.0, 1.0), 50);
        // Energy normalization (0.0-0.5 range)
        assert_eq!(normalize_to_100(0.142, 0.0, 0.5), 28);
        assert_eq!(normalize_to_100(0.25, 0.0, 0.5), 50);
    }

    #[test]
    fn test_format_key() {
        assert_eq!(format_key("D", "minor"), "Dm");
        assert_eq!(format_key("G", "major"), "G");
        assert_eq!(format_key("F#", "minor"), "F#m");
    }
}
