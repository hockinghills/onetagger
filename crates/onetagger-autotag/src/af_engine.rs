use anyhow::Error;
use std::path::{PathBuf, Path};
use std::sync::atomic::Ordering;
use std::thread;
use chrono::Local;
use crossbeam_channel::{unbounded, Receiver};
use onetagger_tagger::{AudioFileInfo, MatchingUtils};
use onetagger_tagger::audiofeatures_provider::*;
use onetagger_tag::{Tag, FrameName};
use onetagger_shared::Settings;

use crate::{TaggingState, TaggingStatus, TaggingStatusWrap, AudioFileInfoImpl, STOP_TAGGING};
use crate::af_providers::AF_PROVIDERS;
use crate::af_sync_cache::AFSyncCache;


/// Start the provider-based audio features tagging process.
/// Returns a channel receiver for progress updates (same pattern as the old system).
pub fn start_provider_tagging(
    config: AFConfig,
    files: Vec<PathBuf>,
) -> Receiver<TaggingStatusWrap> {
    STOP_TAGGING.store(false, Ordering::SeqCst);
    let file_count = files.len();
    let (tx, rx) = unbounded();

    thread::spawn(move || {
        match run_provider_tagging(&config, &files, file_count, &tx) {
            Ok(_) => info!("[AF] Provider tagging complete."),
            Err(e) => error!("[AF] Provider tagging failed: {}", e),
        }
    });

    rx
}


/// The main tagging logic — runs in a background thread.
fn run_provider_tagging(
    config: &AFConfig,
    files: &[PathBuf],
    file_count: usize,
    tx: &crossbeam_channel::Sender<TaggingStatusWrap>,
) -> Result<(), Error> {
    let provider_id = &config.provider_id;

    // Get provider from registry
    let mut provider = {
        let mut registry = AF_PROVIDERS.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let builder = registry.get_builder(provider_id)
            .ok_or(anyhow!("Unknown audio features provider: {}", provider_id))?;
        builder.get_provider(&config.provider_config)?
    };

    // Test connection first
    match provider.test_connection() {
        Ok(msg) => info!("[AF] Provider connected: {}", msg),
        Err(e) => return Err(anyhow!("Failed to connect to {}: {}", provider_id, e)),
    }

    // Open the sync cache
    let cache_path = Settings::get_folder()?.join("af_sync_cache.db");
    let cache = AFSyncCache::open(&cache_path)?;

    // Check if we have cached mappings; if not, do a sync first
    let mapping_count = cache.mapping_count(provider_id)?;
    if mapping_count == 0 {
        info!("[AF] No cached mappings found. Running initial sync...");
        run_initial_sync(&mut *provider, &cache, provider_id, files)?;
    }

    // Get provider capabilities for knowing what we can write
    let capabilities = {
        let registry = AF_PROVIDERS.lock().map_err(|e| anyhow!("Lock error: {}", e))?;
        let entry = registry.providers.iter()
            .find(|p| p.info.id == *provider_id)
            .ok_or(anyhow!("Provider not found: {}", provider_id))?;
        entry.info.capabilities.clone()
    };

    // Process each file
    for (i, file) in files.iter().enumerate() {
        if STOP_TAGGING.load(Ordering::SeqCst) {
            info!("[AF] Tagging stopped by user.");
            break;
        }

        let mut status = TaggingStatus {
            status: TaggingState::Error,
            path: file.to_owned(),
            message: None,
            accuracy: None,
            used_shazam: false,
            release_id: None,
            reason: None,
        };

        // Check skip-tagged
        if config.skip_tagged {
            if let Ok(true) = cache.is_tagged(file, provider_id) {
                status.status = TaggingState::Skipped;
                status.message = Some("Already tagged by this provider".to_string());
                tx.send(TaggingStatusWrap::wrap(
                    provider_id, &status, 0, 1, i as i64, file_count,
                )).ok();
                continue;
            }
        }

        // Look up the cached mapping
        match cache.get_mapping(file, provider_id) {
            Ok(Some(provider_track_id)) => {
                // Fetch features from provider
                match provider.get_features(&provider_track_id) {
                    Ok(result) => {
                        // Write tags to file
                        match write_features_to_file(file, &result, config, &capabilities) {
                            Ok(_) => {
                                // Mark as tagged in cache
                                let _ = cache.mark_tagged(file, provider_id, "0.1.0");
                                status.status = TaggingState::Ok;
                            }
                            Err(e) => {
                                error!("[AF] Failed writing tags to {:?}: {}", file, e);
                                status.message = Some(format!("Write error: {}", e));
                            }
                        }
                    }
                    Err(e) => {
                        warn!("[AF] Failed fetching features for {}: {}", provider_track_id, e);
                        status.message = Some(format!("Feature fetch error: {}", e));
                    }
                }
            }
            Ok(None) => {
                // No mapping — try to match on the fly
                if let Ok(info) = AudioFileInfo::load_file(file, None, None) {
                    let title = info.title().unwrap_or_default().to_string();
                    let artist = info.artist().unwrap_or_default().to_string();
                    
                    // Try matching against cached catalog
                    match try_match_from_catalog(&cache, provider_id, &title, &artist) {
                        Some(provider_track_id) => {
                            // Cache the mapping for next time
                            let _ = cache.set_mapping(
                                file, provider_id, &provider_track_id,
                                Some(&title), Some(&artist), None, None, 0.0,
                            );
                            // Now fetch features
                            match provider.get_features(&provider_track_id) {
                                Ok(result) => {
                                    match write_features_to_file(file, &result, config, &capabilities) {
                                        Ok(_) => {
                                            let _ = cache.mark_tagged(file, provider_id, "0.1.0");
                                            status.status = TaggingState::Ok;
                                        }
                                        Err(e) => {
                                            status.message = Some(format!("Write error: {}", e));
                                        }
                                    }
                                }
                                Err(e) => {
                                    status.message = Some(format!("Feature fetch error: {}", e));
                                }
                            }
                        }
                        None => {
                            status.message = Some(format!(
                                "No match found in provider for: {} - {}", artist, title
                            ));
                        }
                    }
                } else {
                    status.message = Some("Failed to read file metadata".to_string());
                }
            }
            Err(e) => {
                status.message = Some(format!("Cache lookup error: {}", e));
            }
        }

        tx.send(TaggingStatusWrap::wrap(
            provider_id, &status, 0, 1, i as i64, file_count,
        )).ok();
    }

    Ok(())
}


/// Run the initial sync: fetch the provider's full catalog, match against local files.
fn run_initial_sync(
    provider: &mut dyn AFProvider,
    cache: &AFSyncCache,
    provider_id: &str,
    files: &[PathBuf],
) -> Result<(), Error> {
    info!("[AF] Fetching provider catalog...");
    let catalog = provider.fetch_catalog()?;
    info!("[AF] Got {} tracks from provider", catalog.len());

    // Cache the catalog
    cache.cache_catalog(provider_id, &catalog)?;

    // Build local file info list
    info!("[AF] Reading metadata from {} local files...", files.len());
    let mut local_files = vec![];
    for file in files {
        if let Ok(info) = AudioFileInfo::load_file(file, None, None) {
            let title = info.title().unwrap_or_default().to_string();
            let artist = info.artist().unwrap_or_default().to_string();
            local_files.push((file.clone(), title, artist));
        }
    }

    // Run the matching
    info!("[AF] Matching {} local files against {} provider tracks...",
        local_files.len(), catalog.len());
    let report = crate::af_sync_cache::run_sync(
        cache,
        provider_id,
        &catalog,
        &local_files,
        0.75, // default strictness
    );

    info!("[AF] Sync complete:");
    info!("[AF]   Matched:              {} of {} local files", report.matched.len(), report.local_total);
    info!("[AF]   Unmatched local:      {}", report.unmatched_local.len());
    info!("[AF]   Orphaned in provider: {} of {} provider tracks",
        report.unmatched_provider.len(), report.provider_total);

    if !report.unmatched_local.is_empty() {
        info!("[AF] First 10 unmatched local files:");
        for path in report.unmatched_local.iter().take(10) {
            info!("[AF]   {:?}", path);
        }
    }

    Ok(())
}


/// Try to match a track from the cached catalog.
fn try_match_from_catalog(
    cache: &AFSyncCache,
    provider_id: &str,
    title: &str,
    artist: &str,
) -> Option<String> {
    let catalog = cache.get_cached_catalog(provider_id).ok()?;
    if catalog.is_empty() {
        return None;
    }

    let clean_title = MatchingUtils::clean_title_matching(title);
    let artist_lower = artist.to_lowercase();

    let mut best: Option<(String, f64)> = None;

    for (prov_id, prov_title, prov_artist) in &catalog {
        let clean_prov = MatchingUtils::clean_title_matching(prov_title);
        let prov_artist_lower = prov_artist.to_lowercase();

        let title_sim = strsim::normalized_levenshtein(&clean_title, &clean_prov);
        let artist_sim = strsim::normalized_levenshtein(&artist_lower, &prov_artist_lower);
        let combined = (title_sim * 0.6) + (artist_sim * 0.4);

        if combined >= 0.75 {
            if best.is_none() || combined > best.as_ref().unwrap().1 {
                best = Some((prov_id.clone(), combined));
            }
        }
    }

    best.map(|(id, _)| id)
}


/// Write audio features to an audio file's tags.
fn write_features_to_file(
    path: &Path,
    result: &AFTrackResult,
    config: &AFConfig,
    capabilities: &AFProviderCapabilities,
) -> Result<(), Error> {
    let mut tag_wrap = Tag::load_file(path, false)?;
    tag_wrap.set_separators(&config.separators);
    let format = tag_wrap.format();
    let tag = tag_wrap.tag_mut();

    let mut main_tag_values = vec![];

    // Write individual feature values + build prominent tag
    for descriptor in &capabilities.features {
        let feature_config = match config.feature_config.get(&descriptor.id) {
            Some(fc) if fc.enabled => fc,
            _ => continue,
        };

        if let Some(&value) = result.features.get(&descriptor.id) {
            // Write numeric value to the feature-specific tag
            let tag_name = feature_config.tag.by_format(&format);
            if !tag_name.is_empty() {
                tag.set_raw(&tag_name, vec![value.to_string()], true);
            }

            // Determine prominent tag label based on threshold
            let label = if value < feature_config.threshold_min {
                &descriptor.label_low
            } else if value >= feature_config.threshold_max {
                &descriptor.label_high
            } else {
                &descriptor.label_mid
            };

            if !label.is_empty() {
                main_tag_values.push(label.clone());
            }
        }
    }

    // Write combined prominent tag
    if !main_tag_values.is_empty() {
        tag.set_raw(
            &config.main_tag.by_format(&format),
            main_tag_values,
            true,
        );
    }

    // Write BPM
    if config.write_bpm && capabilities.provides_bpm {
        if let Some(bpm) = result.bpm {
            tag.set_raw(
                &FrameName::same("BPM").by_format(&format),
                vec![format!("{:.0}", bpm)],
                true,
            );
        }
    }

    // Write Key
    if config.write_key && capabilities.provides_key {
        if let Some(ref key) = result.key {
            tag.set_raw(
                &FrameName::same("KEY").by_format(&format),
                vec![key.clone()],
                true,
            );
        }
    }

    // Write Genre (from provider's genre predictions)
    if config.write_genre && capabilities.provides_genre && !result.genres.is_empty() {
        let genre_tags: Vec<String> = result.genres.iter()
            .take(config.genre_count)
            .map(|(label, _)| label.clone())
            .collect();
        if !genre_tags.is_empty() {
            tag.set_raw(
                &config.genre_tag.by_format(&format),
                genre_tags,
                true,
            );
        }
    }

    // Write Mood (from provider's mood predictions)
    if config.write_mood && capabilities.provides_mood && !result.moods.is_empty() {
        let mood_tags: Vec<String> = result.moods.iter()
            .take(config.mood_count)
            .map(|(label, _)| label.clone())
            .collect();
        if !mood_tags.is_empty() {
            tag.set_raw(
                &config.mood_tag.by_format(&format),
                mood_tags,
                true,
            );
        }
    }

    // Meta tag with timestamp and provider info
    if config.meta_tag {
        let time = Local::now();
        tag.set_raw(
            "1T_TAGGEDDATE",
            vec![format!("{}_{}", time.format("%Y-%m-%d %H:%M:%S"), config.provider_id.to_uppercase())],
            true,
        );
    }

    tag.save_file(path)?;
    Ok(())
}
