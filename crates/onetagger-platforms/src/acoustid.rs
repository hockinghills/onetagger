use anyhow::Error;
use std::process::Command;
use std::time::Duration;
use serde::Deserialize;
use reqwest::blocking::Client;
use onetagger_tagger::*;


/// AcoustID API response structures
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
struct AcoustIDResponse {
    status: String,
    results: Option<Vec<AcoustIDResult>>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
struct AcoustIDResult {
    id: String,
    score: f64,
    recordings: Option<Vec<AcoustIDRecording>>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
struct AcoustIDRecording {
    id: String,
    title: Option<String>,
    artists: Option<Vec<AcoustIDArtist>>,
    duration: Option<u64>,
    releasegroups: Option<Vec<AcoustIDReleaseGroup>>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
struct AcoustIDArtist {
    id: String,
    name: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
struct AcoustIDReleaseGroup {
    id: String,
    title: Option<String>,
    #[serde(rename = "type")]
    release_type: Option<String>,
}


/// fpcalc JSON output
#[derive(Debug, Clone, Deserialize)]
struct FpcalcOutput {
    duration: f64,
    fingerprint: String,
}


/// AcoustID platform configuration
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcoustIDConfig {
    /// AcoustID API key (get one at https://acoustid.org/new-application)
    pub api_key: String,
}

impl Default for AcoustIDConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
        }
    }
}


/// The AcoustID autotagger source
pub struct AcoustID {
    config: AcoustIDConfig,
    client: Client,
}

impl AcoustID {
    /// Run fpcalc to generate a Chromaprint fingerprint for an audio file
    fn fingerprint(&self, path: &std::path::Path) -> Result<FpcalcOutput, Error> {
        let output = Command::new("fpcalc")
            .arg("-json")
            .arg(path)
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    anyhow!(
                        "fpcalc not found! Install it with: sudo apt install libchromaprint-tools"
                    )
                } else {
                    anyhow!("Failed to run fpcalc: {}", e)
                }
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("fpcalc failed: {}", stderr));
        }

        let result: FpcalcOutput = serde_json::from_slice(&output.stdout)?;
        Ok(result)
    }

    /// Look up a fingerprint on AcoustID
    fn lookup(&self, fingerprint: &str, duration: f64) -> Result<Vec<AcoustIDResult>, Error> {
        let url = format!(
            "https://api.acoustid.org/v2/lookup?client={}&meta=recordings+releasegroups&duration={}&fingerprint={}",
            self.config.api_key,
            duration as u64,
            fingerprint
        );

        let resp = self.client.get(&url).send()?;
        
        // Rate limit: AcoustID allows 3 requests per second
        std::thread::sleep(Duration::from_millis(350));

        let response: AcoustIDResponse = resp.json()?;
        if response.status != "ok" {
            return Err(anyhow!("AcoustID API error: status={}", response.status));
        }

        Ok(response.results.unwrap_or_default())
    }

    /// Convert AcoustID results to OneTagger Track matches
    fn results_to_tracks(&self, results: &[AcoustIDResult]) -> Vec<Track> {
        let mut tracks = vec![];

        for result in results {
            if result.score < 0.5 {
                continue; // Skip low-confidence matches
            }

            if let Some(recordings) = &result.recordings {
                for recording in recordings {
                    let title = match &recording.title {
                        Some(t) => t.clone(),
                        None => continue,
                    };

                    let artists: Vec<String> = recording.artists
                        .as_ref()
                        .map(|a| a.iter().map(|a| a.name.clone()).collect())
                        .unwrap_or_default();

                    if artists.is_empty() {
                        continue;
                    }

                    let album = recording.releasegroups
                        .as_ref()
                        .and_then(|rgs| rgs.first())
                        .and_then(|rg| rg.title.clone());

                    let duration = recording.duration
                        .map(|d| Duration::from_secs(d))
                        .unwrap_or_default();

                    let track = Track {
                        platform: "acoustid".to_string(),
                        title,
                        artists,
                        album,
                        duration: duration.into(),
                        track_id: Some(recording.id.clone()),
                        release_id: recording.releasegroups
                            .as_ref()
                            .and_then(|rgs| rgs.first())
                            .map(|rg| rg.id.clone()),
                        url: format!("https://musicbrainz.org/recording/{}", recording.id),
                        ..Default::default()
                    };

                    tracks.push(track);
                }
            }
        }

        tracks
    }
}


impl AutotaggerSource for AcoustID {
    fn match_track(&mut self, info: &AudioFileInfo, config: &TaggerConfig) -> Result<Vec<TrackMatch>, Error> {
        // Generate fingerprint
        info!("[AcoustID] Fingerprinting: {:?}", info.path);
        let fp = self.fingerprint(&info.path)?;

        // Look up on AcoustID
        let results = self.lookup(&fp.fingerprint, fp.duration)?;
        if results.is_empty() {
            info!("[AcoustID] No results for: {:?}", info.path);
            return Ok(vec![]);
        }

        // Convert to tracks
        let tracks = self.results_to_tracks(&results);
        if tracks.is_empty() {
            info!("[AcoustID] No usable recordings for: {:?}", info.path);
            return Ok(vec![]);
        }

        info!("[AcoustID] Found {} candidate(s) for: {:?}", tracks.len(), info.path);

        // Use OneTagger's standard matching if we have existing metadata to compare against
        if info.title.is_some() || !info.artists.is_empty() {
            Ok(MatchingUtils::match_track(info, &tracks, config, true))
        } else {
            // No existing metadata — take the best AcoustID result directly
            // (this is the whole point for untagged files)
            if let Some(best) = tracks.into_iter().next() {
                Ok(vec![TrackMatch {
                    accuracy: 1.0,
                    track: best,
                    reason: MatchReason::Fingerprint,
                }])
            } else {
                Ok(vec![])
            }
        }
    }

    fn extend_track(&mut self, _track: &mut Track, _config: &TaggerConfig) -> Result<(), Error> {
        // AcoustID returns everything in the initial lookup
        Ok(())
    }
}


/// Builder for AcoustID platform
pub struct AcoustIDBuilder;

impl AutotaggerSourceBuilder for AcoustIDBuilder {
    fn new() -> Self {
        AcoustIDBuilder
    }

    fn get_source(&mut self, config: &TaggerConfig) -> Result<Box<dyn AutotaggerSource>, Error> {
        let acoustid_config: AcoustIDConfig = config.get_custom("acoustid")?;
        
        if acoustid_config.api_key.is_empty() {
            return Err(anyhow!(
                "AcoustID API key is required! Get one free at https://acoustid.org/new-application"
            ));
        }

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Box::new(AcoustID {
            config: acoustid_config,
            client,
        }))
    }

    fn info(&self) -> PlatformInfo {
        PlatformInfo {
            id: "acoustid".to_string(),
            name: "AcoustID".to_string(),
            description: "Audio fingerprint identification via AcoustID + MusicBrainz. \
                Identifies tracks by their audio content, not metadata. \
                Requires fpcalc (sudo apt install libchromaprint-tools) \
                and a free API key from acoustid.org.".to_string(),
            icon: include_bytes!("../assets/musicbrainz.png"), // Reuse MusicBrainz icon for now
            max_threads: 1, // AcoustID rate limit: 3 req/sec
            version: "1.0.0".to_string(),
            custom_options: PlatformCustomOptions::new()
                .add_tooltip(
                    "api_key",
                    "AcoustID API Key",
                    "Get a free key at https://acoustid.org/new-application",
                    PlatformCustomOptionValue::String { value: String::new(), hidden: None },
                ),
            requires_auth: false,
            supported_tags: supported_tags!(Title, Artist, Album, TrackId, ReleaseId, Duration, URL),
        }
    }
}
