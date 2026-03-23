use anyhow::Error;
use std::path::{Path, PathBuf};
use rusqlite::{Connection, params};
use serde::{Serialize, Deserialize};

use onetagger_tagger::audiofeatures_provider::SyncReport;
use onetagger_tagger::MatchingUtils;


/// SQLite-backed cache that stores mappings between local audio files and
/// provider track IDs. Persists across sessions so we only need to do the
/// expensive catalog-fetch + fuzzy-match step once.
pub struct AFSyncCache {
    conn: Connection,
}

/// A cached mapping entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub local_path: PathBuf,
    pub provider_id: String,
    pub provider_track_id: String,
    /// When this mapping was created
    pub synced_at: String,
    /// When features were last written to the file
    pub tagged_at: Option<String>,
    /// Provider version at time of tagging
    pub provider_version: Option<String>,
}

impl AFSyncCache {
    /// Open or create the cache database.
    /// Stored in OneTagger's settings folder alongside other config.
    pub fn open(db_path: &Path) -> Result<Self, Error> {
        let conn = Connection::open(db_path)?;

        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS sync_map (
                local_path      TEXT NOT NULL,
                provider_id     TEXT NOT NULL,
                provider_track_id TEXT NOT NULL,
                local_title     TEXT,
                local_artist    TEXT,
                provider_title  TEXT,
                provider_artist TEXT,
                match_confidence REAL,
                synced_at       TEXT NOT NULL DEFAULT (datetime('now')),
                tagged_at       TEXT,
                provider_version TEXT,
                PRIMARY KEY (local_path, provider_id)
            );

            CREATE INDEX IF NOT EXISTS idx_sync_provider
                ON sync_map (provider_id, provider_track_id);

            CREATE TABLE IF NOT EXISTS provider_catalog_cache (
                provider_id     TEXT NOT NULL,
                provider_track_id TEXT NOT NULL,
                title           TEXT,
                artist          TEXT,
                cached_at       TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (provider_id, provider_track_id)
            );

            CREATE TABLE IF NOT EXISTS sync_meta (
                provider_id     TEXT PRIMARY KEY,
                last_sync       TEXT,
                catalog_size    INTEGER,
                provider_version TEXT
            );
        ")?;

        Ok(Self { conn })
    }

    /// Look up the provider track ID for a local file.
    pub fn get_mapping(&self, local_path: &Path, provider_id: &str) -> Result<Option<String>, Error> {
        let path_str = local_path.to_string_lossy().to_string();
        let mut stmt = self.conn.prepare(
            "SELECT provider_track_id FROM sync_map WHERE local_path = ?1 AND provider_id = ?2"
        )?;
        let result = stmt.query_row(params![path_str, provider_id], |row| {
            row.get::<_, String>(0)
        });

        match result {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Store a mapping between a local file and a provider track.
    pub fn set_mapping(
        &self,
        local_path: &Path,
        provider_id: &str,
        provider_track_id: &str,
        local_title: Option<&str>,
        local_artist: Option<&str>,
        provider_title: Option<&str>,
        provider_artist: Option<&str>,
        confidence: f64,
    ) -> Result<(), Error> {
        let path_str = local_path.to_string_lossy().to_string();
        self.conn.execute(
            "INSERT OR REPLACE INTO sync_map
                (local_path, provider_id, provider_track_id,
                 local_title, local_artist, provider_title, provider_artist,
                 match_confidence, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now'))",
            params![
                path_str, provider_id, provider_track_id,
                local_title, local_artist, provider_title, provider_artist,
                confidence
            ],
        )?;
        Ok(())
    }

    /// Mark a file as tagged (after successfully writing tags).
    pub fn mark_tagged(
        &self,
        local_path: &Path,
        provider_id: &str,
        provider_version: &str,
    ) -> Result<(), Error> {
        let path_str = local_path.to_string_lossy().to_string();
        self.conn.execute(
            "UPDATE sync_map SET tagged_at = datetime('now'), provider_version = ?1
             WHERE local_path = ?2 AND provider_id = ?3",
            params![provider_version, path_str, provider_id],
        )?;
        Ok(())
    }

    /// Check if a file has been tagged by a specific provider.
    pub fn is_tagged(&self, local_path: &Path, provider_id: &str) -> Result<bool, Error> {
        let path_str = local_path.to_string_lossy().to_string();
        let mut stmt = self.conn.prepare(
            "SELECT tagged_at FROM sync_map WHERE local_path = ?1 AND provider_id = ?2"
        )?;
        let result = stmt.query_row(params![path_str, provider_id], |row| {
            row.get::<_, Option<String>>(0)
        });

        match result {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Cache the full provider catalog for fast local matching.
    pub fn cache_catalog(
        &self,
        provider_id: &str,
        catalog: &[(String, String, String)], // (id, title, artist)
    ) -> Result<(), Error> {
        let tx = self.conn.unchecked_transaction()?;

        // Clear old catalog cache for this provider
        tx.execute(
            "DELETE FROM provider_catalog_cache WHERE provider_id = ?1",
            params![provider_id],
        )?;

        // Insert new catalog
        {
            let mut stmt = tx.prepare(
                "INSERT INTO provider_catalog_cache (provider_id, provider_track_id, title, artist)
                 VALUES (?1, ?2, ?3, ?4)"
            )?;
            for (id, title, artist) in catalog {
                stmt.execute(params![provider_id, id, title, artist])?;
            }
        } // stmt dropped here

        // Update sync metadata
        tx.execute(
            "INSERT OR REPLACE INTO sync_meta (provider_id, last_sync, catalog_size)
             VALUES (?1, datetime('now'), ?2)",
            params![provider_id, catalog.len()],
        )?;

        tx.commit()?;
        Ok(())
    }

    /// Get the cached catalog for a provider.
    pub fn get_cached_catalog(&self, provider_id: &str) -> Result<Vec<(String, String, String)>, Error> {
        let mut stmt = self.conn.prepare(
            "SELECT provider_track_id, title, artist FROM provider_catalog_cache
             WHERE provider_id = ?1"
        )?;
        let rows = stmt.query_map(params![provider_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        let mut results = vec![];
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get when the last sync happened for a provider.
    pub fn last_sync(&self, provider_id: &str) -> Result<Option<String>, Error> {
        let mut stmt = self.conn.prepare(
            "SELECT last_sync FROM sync_meta WHERE provider_id = ?1"
        )?;
        let result = stmt.query_row(params![provider_id], |row| {
            row.get::<_, Option<String>>(0)
        });

        match result {
            Ok(v) => Ok(v),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// How many cached mappings exist for a provider.
    pub fn mapping_count(&self, provider_id: &str) -> Result<usize, Error> {
        let mut stmt = self.conn.prepare(
            "SELECT COUNT(*) FROM sync_map WHERE provider_id = ?1"
        )?;
        let count: i64 = stmt.query_row(params![provider_id], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// How many files have been tagged by a provider.
    pub fn tagged_count(&self, provider_id: &str) -> Result<usize, Error> {
        let mut stmt = self.conn.prepare(
            "SELECT COUNT(*) FROM sync_map WHERE provider_id = ?1 AND tagged_at IS NOT NULL"
        )?;
        let count: i64 = stmt.query_row(params![provider_id], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Clear all mappings for a provider (force re-sync).
    pub fn clear_provider(&self, provider_id: &str) -> Result<(), Error> {
        self.conn.execute(
            "DELETE FROM sync_map WHERE provider_id = ?1",
            params![provider_id],
        )?;
        self.conn.execute(
            "DELETE FROM provider_catalog_cache WHERE provider_id = ?1",
            params![provider_id],
        )?;
        self.conn.execute(
            "DELETE FROM sync_meta WHERE provider_id = ?1",
            params![provider_id],
        )?;
        Ok(())
    }
}


/// Perform the full sync: match local files against a provider's catalog.
/// This is the expensive step that we only do once (or on demand).
pub fn run_sync(
    cache: &AFSyncCache,
    provider_id: &str,
    catalog: &[(String, String, String)],  // (provider_track_id, title, artist)
    local_files: &[(PathBuf, String, String)],  // (path, title, artist)
    strictness: f64,
) -> SyncReport {
    let mut matched = vec![];
    let mut unmatched_local = vec![];

    // Track which provider entries got matched (for orphan detection)
    let mut matched_provider_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Pre-compute cleaned catalog entries (avoid redoing this per local file)
    info!("[AF Sync] Pre-processing {} catalog entries...", catalog.len());
    let cleaned_catalog: Vec<(String, String, String, String, String)> = catalog
        .iter()
        .map(|(id, title, artist)| {
            let clean_title = MatchingUtils::clean_title_matching(title);
            let artist_lower = artist.to_lowercase();
            (id.clone(), title.clone(), artist.clone(), clean_title, artist_lower)
        })
        .collect();

    // Build a lookup map: words -> list of catalog indices
    // Index by BOTH artist AND title words, because YouTube-sourced tracks in AudioMuse
    // often have the channel name as "artist" and the real artist embedded in the title
    // (e.g., title="Kevin Morby - Beautiful Strangers", artist="Dead Oceans")
    let mut word_index: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
    for (i, (_, _, _, ref clean_title, ref artist_lower)) in cleaned_catalog.iter().enumerate() {
        // Index by artist words
        for word in artist_lower.split_whitespace() {
            if word.len() >= 2 {
                word_index.entry(word.to_string()).or_default().push(i);
            }
        }
        word_index.entry(artist_lower.clone()).or_default().push(i);
        // Also index by title words (catches artist-in-title YouTube pattern)
        for word in clean_title.split_whitespace() {
            if word.len() >= 3 {
                word_index.entry(word.to_string()).or_default().push(i);
            }
        }
    }

    info!("[AF Sync] Matching {} local files...", local_files.len());
    let total = local_files.len();
    let mut last_log = 0;

    for (file_idx, (path, local_title, local_artist)) in local_files.iter().enumerate() {
        // Progress logging every 500 files
        if file_idx - last_log >= 500 {
            info!("[AF Sync] Progress: {}/{} files processed, {} matched so far",
                file_idx, total, matched.len());
            last_log = file_idx;
        }

        let clean_local_title = MatchingUtils::clean_title_matching(local_title);
        let local_artist_lower = local_artist.to_lowercase();

        // Skip files with empty title+artist (can't match on nothing)
        if clean_local_title.is_empty() && local_artist_lower.is_empty() {
            unmatched_local.push(path.clone());
            continue;
        }

        // Find candidates via word index (checks both artist and title words)
        let mut candidate_indices: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for word in local_artist_lower.split_whitespace() {
            if word.len() >= 2 {
                if let Some(indices) = word_index.get(word) {
                    candidate_indices.extend(indices);
                }
            }
        }
        for word in clean_local_title.split_whitespace() {
            if word.len() >= 3 {
                if let Some(indices) = word_index.get(word) {
                    candidate_indices.extend(indices);
                }
            }
        }

        let mut best_match: Option<(usize, f64)> = None;

        let candidates: Vec<usize> = if candidate_indices.is_empty() {
            (0..cleaned_catalog.len()).collect()
        } else {
            candidate_indices.into_iter().collect()
        };

        for idx in candidates {
            let (_, _, _, ref clean_prov_title, ref prov_artist_lower) = cleaned_catalog[idx];

            let title_sim = strsim::normalized_levenshtein(&clean_local_title, clean_prov_title);
            let artist_sim = strsim::normalized_levenshtein(&local_artist_lower, prov_artist_lower);

            // Also check if local artist appears inside the provider title
            // (YouTube pattern: title="Kevin Morby - Beautiful Strangers", artist="Dead Oceans")
            let artist_in_title = if local_artist_lower.len() >= 3 {
                clean_prov_title.contains(&MatchingUtils::clean_title_matching(&local_artist_lower))
            } else {
                false
            };

            // If artist is found in provider title, boost the score significantly
            let combined = if artist_in_title && artist_sim < 0.5 {
                // Artist is in the title — use title similarity but give credit for the artist match
                (title_sim * 0.5) + 0.4  // guaranteed 0.4 base from the artist-in-title find
            } else {
                (title_sim * 0.6) + (artist_sim * 0.4)
            };

            if combined >= strictness {
                if best_match.is_none() || combined > best_match.unwrap().1 {
                    best_match = Some((idx, combined));
                }
            }
        }

        if let Some((idx, confidence)) = best_match {
            let (ref prov_id, ref prov_title, ref prov_artist, _, _) = cleaned_catalog[idx];
            let _ = cache.set_mapping(
                path,
                provider_id,
                prov_id,
                Some(local_title),
                Some(local_artist),
                Some(prov_title),
                Some(prov_artist),
                confidence,
            );
            matched.push((path.clone(), prov_id.to_string()));
            matched_provider_ids.insert(prov_id.to_string());
        } else {
            unmatched_local.push(path.clone());
        }
    }

    info!("[AF Sync] Pass 1 complete: {} matched, {} unmatched",
        matched.len(), unmatched_local.len());

    // === Pass 2: Retry unmatched at lower threshold (0.55) ===
    // This catches files where YouTube cruft or naming differences
    // made the score drop just below the normal threshold.
    if !unmatched_local.is_empty() {
        let lower_threshold = 0.55;
        info!("[AF Sync] Pass 2: Retrying {} unmatched files at threshold {:.2}...",
            unmatched_local.len(), lower_threshold);

        let mut still_unmatched = vec![];
        let mut pass2_matched = 0;

        for path in &unmatched_local {
            // Re-read the local file info for this path from the original list
            let local_entry = local_files.iter().find(|(p, _, _)| p == path);
            if local_entry.is_none() {
                still_unmatched.push(path.clone());
                continue;
            }
            let (_, local_title, local_artist) = local_entry.unwrap();
            let clean_local_title = MatchingUtils::clean_title_matching(local_title);
            let local_artist_lower = local_artist.to_lowercase();

            // This time, search the ENTIRE catalog (no pre-filter)
            // since the pre-filter might have excluded the right match
            let mut best_match: Option<(usize, f64)> = None;
            for (idx, (_, _, _, ref clean_prov_title, ref prov_artist_lower)) in cleaned_catalog.iter().enumerate() {
                let title_sim = strsim::normalized_levenshtein(&clean_local_title, clean_prov_title);
                let artist_sim = strsim::normalized_levenshtein(&local_artist_lower, prov_artist_lower);

                let artist_in_title = if local_artist_lower.len() >= 3 {
                    clean_prov_title.contains(&MatchingUtils::clean_title_matching(&local_artist_lower))
                } else {
                    false
                };

                let combined = if artist_in_title && artist_sim < 0.5 {
                    (title_sim * 0.5) + 0.4
                } else {
                    (title_sim * 0.6) + (artist_sim * 0.4)
                };

                if combined >= lower_threshold {
                    if best_match.is_none() || combined > best_match.unwrap().1 {
                        best_match = Some((idx, combined));
                    }
                }
            }

            if let Some((idx, confidence)) = best_match {
                let (ref prov_id, ref prov_title, ref prov_artist, _, _) = cleaned_catalog[idx];
                let _ = cache.set_mapping(
                    path,
                    provider_id,
                    prov_id,
                    Some(local_title),
                    Some(local_artist),
                    Some(prov_title),
                    Some(prov_artist),
                    confidence,
                );
                matched.push((path.clone(), prov_id.to_string()));
                matched_provider_ids.insert(prov_id.to_string());
                pass2_matched += 1;
            } else {
                still_unmatched.push(path.clone());
            }
        }

        info!("[AF Sync] Pass 2 recovered {} additional matches", pass2_matched);
        unmatched_local = still_unmatched;
    }

    info!("[AF Sync] Complete: {} matched, {} unmatched local, {} orphaned provider",
        matched.len(), unmatched_local.len(),
        catalog.len() - matched_provider_ids.len());

    // Find orphaned provider entries
    let unmatched_provider: Vec<(String, String, String)> = catalog
        .iter()
        .filter(|(id, _, _)| !matched_provider_ids.contains(id))
        .map(|(id, title, artist)| (id.clone(), title.clone(), artist.clone()))
        .collect();

    SyncReport {
        provider_total: catalog.len(),
        local_total: local_files.len(),
        matched,
        unmatched_local,
        unmatched_provider,
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_cache_roundtrip() {
        let tmp = NamedTempFile::new().unwrap();
        let cache = AFSyncCache::open(tmp.path()).unwrap();

        let path = PathBuf::from("/music/test.mp3");

        // Initially no mapping
        assert!(cache.get_mapping(&path, "audiomuse").unwrap().is_none());
        assert!(!cache.is_tagged(&path, "audiomuse").unwrap());

        // Set a mapping
        cache.set_mapping(
            &path, "audiomuse", "abc123",
            Some("Test Song"), Some("Test Artist"),
            Some("Test Song"), Some("Test Artist"),
            0.95,
        ).unwrap();

        // Now it exists
        assert_eq!(
            cache.get_mapping(&path, "audiomuse").unwrap(),
            Some("abc123".to_string())
        );

        // Not yet tagged
        assert!(!cache.is_tagged(&path, "audiomuse").unwrap());

        // Mark as tagged
        cache.mark_tagged(&path, "audiomuse", "0.1.0").unwrap();
        assert!(cache.is_tagged(&path, "audiomuse").unwrap());

        // Counts
        assert_eq!(cache.mapping_count("audiomuse").unwrap(), 1);
        assert_eq!(cache.tagged_count("audiomuse").unwrap(), 1);
    }

    #[test]
    fn test_catalog_cache() {
        let tmp = NamedTempFile::new().unwrap();
        let cache = AFSyncCache::open(tmp.path()).unwrap();

        let catalog = vec![
            ("id1".to_string(), "Song A".to_string(), "Artist A".to_string()),
            ("id2".to_string(), "Song B".to_string(), "Artist B".to_string()),
        ];

        cache.cache_catalog("audiomuse", &catalog).unwrap();
        let retrieved = cache.get_cached_catalog("audiomuse").unwrap();
        assert_eq!(retrieved.len(), 2);
        assert!(cache.last_sync("audiomuse").unwrap().is_some());
    }
}
