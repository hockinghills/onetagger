# OneTagger × AudioMuse-AI Integration
## Architecture & Design Specification

**Version:** 0.1-draft
**Date:** 2026-03-22
**Status:** Design Phase

---

## 1. Executive Summary

This document describes the plan to refactor OneTagger's Audio Features system from a
Spotify-hardwired implementation into a modular provider architecture, and to build an
AudioMuse-AI provider that delivers richer functionality than Spotify ever offered —
entirely locally, entirely free.

**The Spotify audio features endpoint is already dead in the current codebase** (line
136-138 of `spotify.rs` returns `Err(anyhow!("Spotify deprecated AudioFeatures"))`).
This work replaces a broken feature with something substantially better.

### What users get

- **Spotify Premium users**: Their existing workflow continues, now as a module
- **Everyone else**: AudioMuse-AI provides BPM, key, energy, danceability, mood
  predictions, genre analysis, and AI-powered similarity/playlist features — all
  running locally with zero API costs
- **Future providers**: The modular architecture makes it trivial to add new audio
  analysis backends

---

## 2. Current Architecture (What Exists Today)

### 2.1 OneTagger Audio Features — Hardwired to Spotify

**File:** `crates/onetagger-autotag/src/audiofeatures.rs` (303 lines)

The entire audio features pipeline is a monolithic block that directly imports and
depends on `rspotify` types:

```
AudioFeaturesConfig
  └─ AFProperties (8 hardcoded Spotify features)
       ├─ acousticness
       ├─ danceability
       ├─ energy
       ├─ instrumentalness
       ├─ liveness
       ├─ speechiness
       ├─ valence
       └─ popularity

AudioFeatures::start_tagging(config, spotify: Spotify, files)
  └─ AudioFeatures::find_features(spotify, track)  // searches Spotify by ISRC/title
       └─ spotify.audio_features(track_id)          // DEAD — returns error
  └─ AudioFeatures::write_to_path(...)              // writes tags to file
```

**Key problems:**
- `AFProperties::merge_with_values()` takes `rspotify::model::audio::AudioFeatures`
  directly — no abstraction
- `AudioFeatures::start_tagging()` takes a `Spotify` object directly — no trait
- The 8 properties are hardcoded structs, not dynamic
- The UI (`AudioFeatures.vue`) is hardwired to Spotify login flow

### 2.2 OneTagger Auto Tag — Already Modular (The Pattern to Follow)

**File:** `crates/onetagger-tagger/src/lib.rs`

```rust
trait AutotaggerSourceBuilder: Any + Send + Sync {
    fn new() -> Self;
    fn get_source(&mut self, config: &TaggerConfig) -> Result<Box<dyn AutotaggerSource>>;
    fn info(&self) -> PlatformInfo;
    fn config_callback(&mut self, name: &str, config: Value) -> ConfigCallbackResponse;
}

trait AutotaggerSource: Any + Send + Sync {
    fn match_track(&mut self, info: &AudioFileInfo, config: &TaggerConfig) -> Result<Vec<TrackMatch>>;
    fn extend_track(&mut self, track: &mut Track, config: &TaggerConfig) -> Result<()>;
}
```

**Registration:** `crates/onetagger-autotag/src/platforms.rs` — each platform is
registered via `add_builtin::<BuilderType>()`, with support for dynamically loaded
custom platforms via shared libraries.

This is the proven pattern we replicate for audio features.

---

## 3. AudioMuse-AI Data Model

### 3.1 Score Table (per-track analysis data)

From a real record (`The Tams — Take Away`):

```json
{
  "item_id":        "0776337a1b765b23b6f865ff2c637673",
  "title":          "Take Away",
  "author":         "The Tams",
  "album":          "Hey Girl Don't Bother Me",
  "album_artist":   null,
  "tempo":          133.93,
  "key":            "D",
  "scale":          "minor",
  "energy":         0.142,
  "mood_vector":    "jazz:0.563,female vocalists:0.561,electronic:0.529,pop:0.526,chillout:0.525",
  "other_features": "danceable:0.57,aggressive:0.03,happy:0.15,party:0.03,relaxed:0.96,sad:0.77",
  "file_path":      null,
  "year":           null,
  "rating":         null
}
```

**Field details:**

| Field | Type | Range | Notes |
|-------|------|-------|-------|
| `tempo` | float | BPM | Detected via librosa beat tracking |
| `key` | string | A-G# | Musical key |
| `scale` | string | "major"/"minor" | |
| `energy` | float | 0.0–1.0 | RMS energy via librosa |
| `mood_vector` | string | "label:score,..." | Top N from 50 MusiCNN mood labels |
| `other_features` | string | "label:score,..." | 6 CLAP-derived features (see below) |
| `file_path` | string? | nullable | Media server file path (not always populated) |

**`other_features` labels** (all 0.0–1.0):
- `danceable` — maps to Spotify's danceability
- `aggressive` — no Spotify equivalent (unique to AudioMuse)
- `happy` — maps to Spotify's valence (positive end)
- `party` — social/party energy
- `relaxed` — chill/ambient quality
- `sad` — maps to inverse of Spotify's valence

**`mood_vector` labels** (50 total from MusiCNN, top N stored):
rock, pop, alternative, indie, electronic, female vocalists, dance, 00s,
alternative rock, jazz, beautiful, metal, chillout, male vocalists, classic rock,
soul, indie rock, Mellow, electronica, 80s, folk, 90s, chill, instrumental, punk,
oldies, blues, hard rock, ambient, acoustic, experimental, female vocalist, guitar,
Hip-Hop, 70s, party, country, easy listening, sexy, catchy, funk, electro, heavy
metal, Progressive rock, 60s, rnb, indie pop, sad, House, happy

### 3.2 Embedding Table

- 200-dimensional float32 vector per track
- Generated by MusiCNN neural network
- Powers similarity search, playlist paths, alchemy, clustering

### 3.3 Available API Endpoints

**Core data access:**
- `GET /external/get_score?id={item_id}` — full analysis data for a track
- `GET /external/get_embedding?id={item_id}` — 200-dim embedding vector
- `GET /external/search?search_query={q}` — find tracks by title/artist/album
- `GET /api/search_tracks?search_query={q}` — same with pagination

**Similarity & discovery:**
- `GET /api/similar_tracks?item_id={id}&n={count}` — find N similar tracks
- `GET /api/similar_artists?artist={name}&n={count}` — find similar artists
- `GET /api/artist_tracks?artist={name}` — all tracks by an artist

**Advanced features:**
- `GET /api/sonic_fingerprint/generate` — personalized recommendations
- `GET /api/waveform?item_id={id}` — waveform peak data
- `GET /api/map` — 2D projection of library for visualization
- `POST /api/create_playlist` — push playlists to media server

**AI-powered:**
- `POST /chat/api/chatPlaylist` — natural language playlist generation

---

## 4. Proposed Architecture

### 4.1 New Trait: AudioFeaturesProvider

**New file:** `crates/onetagger-tagger/src/audiofeatures_provider.rs`

```rust
/// Capabilities an audio features provider can advertise
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AFProviderCapabilities {
    /// Basic audio features this provider supports
    pub features: Vec<AFFeatureDescriptor>,
    /// Whether this provider can find similar tracks
    pub similarity_search: bool,
    /// Whether this provider can generate playlists
    pub playlist_generation: bool,
    /// Whether this provider can do song-path generation
    pub song_paths: bool,
    /// Whether this provider requires authentication
    pub requires_auth: bool,
    /// Whether this provider requires an external service
    pub requires_external_service: bool,
}

/// Describes a single audio feature this provider can return
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AFFeatureDescriptor {
    /// Internal key (e.g., "danceability", "energy", "aggressive")
    pub id: String,
    /// Display name for UI
    pub name: String,
    /// Default tag frame name
    pub default_tag: FrameName,
    /// Range of raw values (for normalization)
    pub value_range: (f64, f64),
    /// Default threshold range for prominent tag classification
    pub default_threshold: (i8, i8),
    /// Low/mid/high labels for prominent tag
    pub labels: (String, String, String),
}

/// A matched track with its audio features from a provider
#[derive(Debug, Clone)]
pub struct AFTrackResult {
    /// The features as key-value pairs (feature_id -> 0-100 normalized value)
    pub features: HashMap<String, i8>,
    /// The prominent tag values (feature_id -> label string)
    pub prominent_tags: HashMap<String, String>,
    /// Optional: raw/extra data the provider wants to pass through
    pub extra: HashMap<String, String>,
}

/// Builder for creating provider instances
pub trait AudioFeaturesProviderBuilder: Any + Send + Sync {
    fn new() -> Self where Self: Sized;
    fn info(&self) -> AFProviderInfo;
    fn capabilities(&self) -> AFProviderCapabilities;
    fn get_provider(&mut self, config: &Value) -> Result<Box<dyn AudioFeaturesProvider>>;
    fn config_callback(&mut self, name: &str, config: Value) -> ConfigCallbackResponse {
        ConfigCallbackResponse::Empty
    }
}

/// The provider itself — does the actual work
pub trait AudioFeaturesProvider: Any + Send + Sync {
    /// Match a local file to this provider's database and return features
    fn get_features(&mut self, info: &AudioFileInfo) -> Result<Option<AFTrackResult>>;

    /// Find tracks similar to the given track (optional capability)
    fn find_similar(&mut self, info: &AudioFileInfo, count: usize)
        -> Result<Vec<SimilarTrack>> {
        Err(anyhow!("Similarity search not supported by this provider"))
    }

    /// Generate a playlist path between two tracks (optional capability)
    fn find_path(&mut self, from: &AudioFileInfo, to: &AudioFileInfo, steps: usize)
        -> Result<Vec<SimilarTrack>> {
        Err(anyhow!("Song paths not supported by this provider"))
    }
}

/// Provider metadata for UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AFProviderInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub icon: &'static [u8],
}

/// A track returned from similarity search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarTrack {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub distance: f64,
    pub provider_track_id: String,
}
```

### 4.2 Spotify Provider Module

**New file:** `crates/onetagger-platforms/src/spotify_af.rs`

Wraps the existing Spotify audio features code into the new trait. Maps:

| Spotify Feature | AFFeatureDescriptor.id | Labels (low/mid/high) |
|----------------|----------------------|----------------------|
| acousticness | `acousticness` | #electronic / "" / #acoustic |
| danceability | `danceability` | #dance-low / #dance-med / #dance-high |
| energy | `energy` | #energy-low / #energy-med / #energy-high |
| instrumentalness | `instrumentalness` | #vocal-high / #vocal-med / #vocal-low |
| liveness | `liveness` | #recording / "" / #live |
| speechiness | `speechiness` | #music / "" / #speech |
| valence | `valence` | #negative / #balanced / #positive |
| popularity | `popularity` | #unpopular / "" / #popular |

Requires Spotify Premium + OAuth. Advertises `similarity_search: false`,
`playlist_generation: false`.

### 4.3 AudioMuse-AI Provider Module

**New file:** `crates/onetagger-platforms/src/audiomuse.rs`

#### Configuration

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioMuseConfig {
    /// AudioMuse-AI API base URL (e.g., "http://localhost:8500")
    pub api_url: String,
    /// Optional API token (if auth is enabled)
    pub api_token: Option<String>,
    /// How to match local files to AudioMuse tracks
    pub match_strategy: AudioMuseMatchStrategy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AudioMuseMatchStrategy {
    /// Match by title + artist (default, always works)
    Metadata,
    /// Match by file path (requires file_path populated in AudioMuse DB)
    FilePath,
}
```

#### Feature Mapping

| AudioMuse Field | AFFeatureDescriptor.id | Source | Labels (low/mid/high) |
|----------------|----------------------|--------|----------------------|
| `energy` | `energy` | score.energy | #energy-low / #energy-med / #energy-high |
| `other_features.danceable` | `danceability` | score.other_features | #dance-low / #dance-med / #dance-high |
| `other_features.happy` | `happiness` | score.other_features | #somber / #balanced / #happy |
| `other_features.sad` | `sadness` | score.other_features | #upbeat / "" / #melancholy |
| `other_features.aggressive` | `aggression` | score.other_features | #gentle / "" / #aggressive |
| `other_features.relaxed` | `relaxation` | score.other_features | #tense / "" / #relaxed |
| `other_features.party` | `party` | score.other_features | #intimate / "" / #party |
| `mood_vector` (top mood) | `genre` | score.mood_vector | Written as genre tag |

**Additional non-feature data written:**
- `tempo` → BPM tag
- `key` + `scale` → Key tag (e.g., "Dm", "G")

#### Advertised Capabilities

```rust
AFProviderCapabilities {
    features: [/* 7 features above */],
    similarity_search: true,     // via /api/similar_tracks
    playlist_generation: true,   // via /api/create_playlist + clustering
    song_paths: true,            // via song path API
    requires_auth: false,        // unless auth enabled on AudioMuse
    requires_external_service: true,  // needs AudioMuse running
}
```

#### Track Matching Flow

```
Local file (path, title, artist, ISRC)
  │
  ├─ Strategy: Metadata (default)
  │    └─ GET /api/search_tracks?search_query={artist}+{title}
  │         └─ Fuzzy match results using OneTagger's existing MatchingUtils
  │
  ├─ Strategy: FilePath
  │    └─ GET /external/search?search_query={filename}
  │         └─ Match by path comparison
  │
  └─ On match → GET /external/get_score?id={item_id}
       └─ Parse score fields → AFTrackResult
```

#### Similarity Search Flow (new capability)

```
User selects track in OneTagger → "Find Similar"
  │
  └─ GET /api/similar_tracks?item_id={matched_id}&n=20
       └─ Returns list of similar tracks with distances
            └─ Display in new UI panel
```

### 4.4 Provider Registration

**Modified file:** `crates/onetagger-autotag/src/audiofeatures.rs`

The current monolithic `AudioFeatures` struct gets replaced with a provider registry
mirroring how `AutotaggerPlatforms` works:

```rust
pub struct AudioFeaturesProviders {
    pub providers: Vec<AudioFeaturesProviderEntry>,
}

impl AudioFeaturesProviders {
    fn builtin() -> Self {
        let mut output = vec![];
        Self::add_builtin::<SpotifyAFBuilder>(&mut output);
        Self::add_builtin::<AudioMuseBuilder>(&mut output);
        AudioFeaturesProviders { providers: output }
    }
}
```

### 4.5 UI Changes

**Modified file:** `client/src/views/AudioFeatures.vue`

Current flow:
```
SpotifyLogin → Path selection → 8 hardcoded properties → Start
```

New flow:
```
Provider selector (Spotify | AudioMuse | future...)
  │
  ├─ Spotify selected:
  │    └─ SpotifyLogin → Path → 8 properties → Start
  │
  └─ AudioMuse selected:
       └─ Connection config (URL, optional token)
            → Test connection button
            → Path selection
            → Dynamic properties (7 features from capabilities)
            → Options:
                 ☑ Write genre from mood analysis
                 ☑ Write BPM
                 ☑ Write key
            → Similarity tools panel:
                 [Find Similar] [Song Path] [Create Playlist]
            → Start
```

The properties section dynamically renders based on `AFProviderCapabilities.features`
rather than being hardcoded. Each provider declares what it can do, and the UI adapts.

---

## 5. Implementation Plan

### Phase 1: Trait Definition & Refactor (Foundation)

**Effort:** Medium
**Files changed:** 4-5 new, 3-4 modified

1. Define `AudioFeaturesProvider` trait + associated types in `onetagger-tagger`
2. Create `AudioFeaturesProviders` registry in `onetagger-autotag`
3. Refactor existing Spotify code into `SpotifyAFBuilder` / `SpotifyAFProvider`
4. Verify existing Spotify flow still works identically
5. Update `AudioFeaturesConfig` to include provider selection

**Key principle:** After Phase 1, the app works exactly as before for Spotify users.
Nothing breaks. We just moved the code behind an abstraction.

### Phase 2: AudioMuse Provider (The Good Stuff)

**Effort:** Medium
**Files changed:** 2-3 new, 1-2 modified

1. Implement `AudioMuseBuilder` / `AudioMuseProvider`
2. HTTP client for AudioMuse REST API (reqwest, already in deps)
3. Track matching logic (metadata-based and file-path-based)
4. Score parsing (mood_vector string → features, other_features string → features)
5. Feature normalization and mapping to AFTrackResult
6. Connection test endpoint

### Phase 3: UI Modernization

**Effort:** Medium-High
**Files changed:** 3-5 Vue components

1. Provider selector component
2. Dynamic properties renderer (reads from capabilities)
3. AudioMuse connection config component
4. Similarity search panel (new feature)
5. Update settings persistence for multi-provider config

### Phase 4: Advanced AudioMuse Features (The Cool Shit)

**Effort:** Medium
**Files changed:** 2-3 Rust, 2-3 Vue

1. Similar tracks panel in UI
2. Song path generation (A→B playlist)
3. Playlist creation (push to media server via AudioMuse)
4. Library visualization (2D map from AudioMuse's UMAP projection)

### Phase 5: Community Release

1. Documentation
2. README updates for both projects
3. PR to OneTagger repo (or fork release)
4. PR to AudioMuse-AI repo (if API changes needed)

---

## 6. Matching Strategy Deep Dive

The hardest part of this integration is **matching local audio files to AudioMuse
tracks**. OneTagger works with files on disk. AudioMuse indexes tracks via a media
server (Jellyfin/Navidrome). Their IDs are different.

### Approach 1: Metadata Match (Default)

Use OneTagger's existing `MatchingUtils` to fuzzy-match by artist + title against
AudioMuse's search API. This is the same approach OneTagger uses for every other
platform.

**Pros:** Works everywhere, no special setup
**Cons:** Depends on search working correctly (note: search_u trigram index may need
investigation — search was returning empty results during testing)

### Approach 2: File Path Match

If the user's AudioMuse instance has `file_path` populated in the score table (depends
on media server config), we can match by comparing file paths.

**Pros:** Exact match, no fuzzy logic needed
**Cons:** Requires file_path to be populated; paths may differ between systems

### Approach 3: Direct Database Access (Power User Option)

For users running AudioMuse on the same machine, we could optionally connect directly
to PostgreSQL instead of going through the REST API. This would bypass any search
issues and give us full SQL query capability.

**Pros:** Most reliable, fastest, full access to all data
**Cons:** Requires PostgreSQL connection string, tighter coupling

### Recommendation

Support all three, with metadata match as default. Let the user choose in config.
The direct DB option is a nice power-user escape hatch.

---

## 7. Data Flow Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                      OneTagger                               │
│                                                              │
│  ┌──────────────┐    ┌──────────────────────────────────┐   │
│  │ Audio Files   │    │ Audio Features Engine (refactored)│   │
│  │ on disk       │───▶│                                  │   │
│  │ (.mp3/.flac)  │    │  ┌────────────────────────┐      │   │
│  └──────────────┘    │  │ Provider Registry       │      │   │
│                      │  │  ├─ SpotifyAFProvider    │      │   │
│                      │  │  └─ AudioMuseAFProvider ─┼──────┼─┐ │
│                      │  └────────────────────────┘      │ │ │
│                      │                                  │ │ │
│                      │  AudioFileInfo ──▶ Provider      │ │ │
│                      │                   ──▶ AFResult   │ │ │
│                      │                   ──▶ Write Tags │ │ │
│                      └──────────────────────────────────┘ │ │
│                                                            │ │
│  ┌──────────────────────────────────────────────────────┐  │ │
│  │ New: Similarity Panel                                 │  │ │
│  │  [Find Similar] [Song Path] [Create Playlist]        │  │ │
│  └──────────────────────────────────────────────────────┘  │ │
└────────────────────────────────────────────────────────────┘ │
                                                               │
                         HTTP/REST API                         │
                                                               │
┌──────────────────────────────────────────────────────────────┘
│
▼
┌─────────────────────────────────────────────────────────────┐
│                    AudioMuse-AI                               │
│                                                              │
│  Flask API (:8500)                                           │
│  ├─ /external/get_score     ← audio features per track      │
│  ├─ /external/get_embedding ← 200-dim vector                │
│  ├─ /external/search        ← find tracks                   │
│  ├─ /api/similar_tracks     ← similarity search             │
│  ├─ /api/similar_artists    ← artist discovery              │
│  └─ /api/create_playlist    ← push to media server          │
│                                                              │
│  PostgreSQL                                                  │
│  ├─ score (analysis data)                                    │
│  ├─ embedding (200-dim vectors)                              │
│  └─ clap_embedding (512-dim CLAP vectors)                   │
│                                                              │
│  Analysis Engine (ONNX + Librosa)                            │
│  ├─ MusiCNN → embeddings + mood predictions                 │
│  ├─ CLAP → text-audio matching + other_features             │
│  └─ Librosa → tempo, energy, key detection                  │
└─────────────────────────────────────────────────────────────┘
```

---

## 8. Comparison: Spotify vs AudioMuse Provider

| Capability | Spotify (old) | Spotify (module) | AudioMuse (module) |
|-----------|--------------|-----------------|-------------------|
| Energy | ✅ (0-1) | ✅ (0-1) | ✅ (0-1, local) |
| Danceability | ✅ (0-1) | ✅ (0-1) | ✅ (0-1, CLAP-derived) |
| Valence/Happiness | ✅ (0-1) | ✅ (0-1) | ✅ happy + sad (richer) |
| Acousticness | ✅ (0-1) | ✅ (0-1) | ❌ (not in model) |
| Instrumentalness | ✅ (0-1) | ✅ (0-1) | ❌ (not in model*) |
| Liveness | ✅ (0-1) | ✅ (0-1) | ❌ (not in model) |
| Speechiness | ✅ (0-1) | ✅ (0-1) | ❌ (not in model) |
| Popularity | ✅ (0-100) | ✅ (0-100) | ❌ (local library) |
| Aggression | ❌ | ❌ | ✅ (0-1, unique) |
| Relaxation | ❌ | ❌ | ✅ (0-1, unique) |
| Party vibe | ❌ | ❌ | ✅ (0-1, unique) |
| Sadness | ❌ | ❌ | ✅ (0-1, unique) |
| Genre/mood prediction | ❌ | ❌ | ✅ (50 labels, scored) |
| BPM detection | ❌ (separate) | ❌ | ✅ (librosa) |
| Key detection | ✅ (via features) | ✅ | ✅ (librosa) |
| Similar tracks | ❌ | ❌ | ✅ (200-dim embedding) |
| Playlist generation | ❌ | ❌ | ✅ (clustering + AI) |
| Song paths (A→B) | ❌ | ❌ | ✅ (vector interpolation) |
| Library visualization | ❌ | ❌ | ✅ (UMAP 2D map) |
| Requires internet | ✅ | ✅ | ❌ (fully local) |
| Requires payment | ✅ (Premium) | ✅ (Premium) | ❌ (free/open source) |

*AudioMuse's mood_vector includes "instrumental" as a mood label, which could
potentially be mapped as a proxy for instrumentalness.

---

## 9. Open Questions

1. **Search reliability:** The AudioMuse search API returned empty results during
   testing despite data being present. The `search_u` trigram index may need
   investigation. Should we prioritize the direct DB access approach?

2. **file_path population:** The test record had `file_path: null`. What percentage
   of your library has this populated? This affects the file-path matching strategy.

3. **Mood vector as genre tag:** Should the top mood(s) from mood_vector be written
   as genre tags? If so, how many? The top 1? Top 3? User configurable?

4. **Tag naming convention:** Should AudioMuse tags use the same `1T_` prefix as
   Spotify features (e.g., `1T_DANCEABILITY`) for compatibility, or use a different
   prefix (e.g., `AM_DANCEABILITY`) to distinguish the source?

5. **Embedding storage:** Should we optionally write the 200-dim embedding vector
   into a custom tag field? This would let other tools access the similarity data
   without needing AudioMuse running, but it's a lot of data per file.

6. **AudioMuse version pinning:** Which version of AudioMuse-AI should we target?
   The API appears stable but the project is actively developed.

---

## 10. File Inventory

### New files to create:

```
crates/onetagger-tagger/src/audiofeatures_provider.rs   # Trait definitions
crates/onetagger-platforms/src/spotify_af.rs             # Spotify AF module
crates/onetagger-platforms/src/audiomuse.rs              # AudioMuse module
crates/onetagger-platforms/assets/audiomuse.png          # Icon
crates/onetagger-autotag/src/af_providers.rs             # Provider registry
client/src/components/AFProviderSelector.vue             # Provider picker
client/src/components/AudioMuseConfig.vue                # Connection config
client/src/components/SimilarityPanel.vue                # Similarity features
```

### Files to modify:

```
crates/onetagger-autotag/src/audiofeatures.rs    # Refactor to use providers
crates/onetagger-autotag/src/lib.rs              # Wire in provider registry
crates/onetagger-autotag/Cargo.toml              # Add reqwest dependency
crates/onetagger-platforms/src/lib.rs            # Export new modules
crates/onetagger-platforms/Cargo.toml            # Dependencies
client/src/views/AudioFeatures.vue               # Provider-aware UI
client/src/scripts/settings.ts                   # Multi-provider config
client/src/scripts/onetagger.ts                  # New message handlers
```

### Files to keep unchanged:

```
crates/onetagger-platforms/src/spotify.rs        # AutoTagger Spotify (untouched)
All other platform modules                       # Not affected
```
