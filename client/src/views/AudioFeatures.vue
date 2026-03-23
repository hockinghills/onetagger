<template>
<div class='text-center af-wrapper'>
<div class='af-content'>

    <!-- Mode Selector -->
    <div class='text-subtitle2 text-bold text-primary q-mt-lg'>AUDIO FEATURES ENGINE</div>
    <div class='text-subtitle2 q-mb-md text-grey-6'>Choose your audio analysis provider</div>
    <div class='row justify-center q-mb-lg'>
        <q-btn-toggle
            v-model='mode'
            toggle-color='primary'
            :options="[
                { label: 'AI Provider (New)', value: 'provider' },
                { label: 'Spotify (Legacy)', value: 'legacy' },
            ]"
            class='q-mb-sm'
        />
    </div>

    <!-- ========== PROVIDER MODE ========== -->
    <div v-if='mode === "provider"'>

        <!-- Provider Selector -->
        <div v-if='$1t.afProviders.value.length > 0'>
            <div class='text-subtitle2 text-bold text-primary q-mt-md'>PROVIDER</div>
            <div class='row justify-center q-my-sm'>
                <q-select
                    v-model='providerConfig.providerId'
                    :options='providerOptions'
                    emit-value map-options filled
                    style='width: 400px'
                    @update:model-value='onProviderChange'
                >
                    <template v-slot:option='scope'>
                        <q-item v-bind='scope.itemProps'>
                            <q-item-section avatar v-if='scope.opt.icon'>
                                <q-avatar size='32px'><img :src='scope.opt.icon'></q-avatar>
                            </q-item-section>
                            <q-item-section>
                                <q-item-label>{{ scope.opt.label }}</q-item-label>
                                <q-item-label caption>{{ scope.opt.description }}</q-item-label>
                            </q-item-section>
                        </q-item>
                    </template>
                </q-select>
            </div>
        </div>
        <div v-else class='text-grey-6 q-my-md'>Loading providers...</div>

        <!-- Connection Config -->
        <div v-if='selectedProvider'>
            <q-separator class='q-mx-auto' style='max-width: 513px; margin-top: 16px; margin-bottom: 25px' inset color="dark"/>
            <div class='text-subtitle2 text-bold text-primary'>CONNECTION</div>
            <div class='text-subtitle2 q-mb-md text-grey-6'>
                Configure connection to your {{ selectedProvider.name }} instance
            </div>
            <div class='row justify-center q-gutter-sm' style='max-width: 600px; margin: auto;'>
                <q-input filled label='API URL' v-model='providerConfig.providerConfig.apiUrl'
                    style='flex: 1; min-width: 300px;' placeholder='http://localhost:8000' />
                <q-input filled label='API Token (optional)' v-model='providerConfig.providerConfig.apiToken'
                    style='flex: 1; min-width: 200px;' type='password' />
            </div>
            <div class='row justify-center q-mt-sm'>
                <q-btn outline color='primary' label='Test Connection' icon='mdi-connection'
                    :loading='testing' @click='testConnection' />
            </div>
            <div v-if='$1t.afConnectionStatus.value' class='q-mt-sm'>
                <q-badge :color='$1t.afConnectionStatus.value.success ? "positive" : "negative"' class='q-pa-sm'>
                    {{ $1t.afConnectionStatus.value.message }}
                </q-badge>
            </div>
        </div>

        <!-- Path Selection -->
        <div v-if='selectedProvider'>
            <q-separator class='q-mx-auto' style='max-width: 513px; margin-top: 20px; margin-bottom: 25px' inset color="dark"/>
            <div class='text-subtitle2 text-bold text-primary'>SELECT INPUT</div>
            <div class='text-subtitle2 q-mb-md text-grey-6'>
                Drag & drop folder, copy/paste path or <span class='click-highlight text-caption text-bold'>CLICK</span>
                the <q-icon name='mdi-open-in-app'></q-icon> icon
            </div>
            <div class='row justify-center' style='max-width: 725px; margin: auto;'>
                <div class='col-1'></div>
                <q-input filled class='col-10' label='Path' v-model='providerConfig.path'>
                    <template v-slot:append>
                        <q-btn round dense flat icon='mdi-open-in-app' class='text-grey-4' @click='browse'></q-btn>
                    </template>
                </q-input>
                <div class='col-1'></div>
            </div>
            <div class='row justify-center' style='max-width: 725px; margin: auto;'>
                <div class='col-1'></div>
                <PlaylistDropZone v-model='playlist' class='q-my-sm q-pt-md q-pb-md col-10'></PlaylistDropZone>
                <div class='col-1'></div>
            </div>
        </div>

        <!-- Sync Library (below path, requires connection + path) -->
        <div v-if='selectedProvider'>
            <q-separator class='q-mx-auto' style='max-width: 513px; margin-top: 16px; margin-bottom: 25px' inset color="dark"/>
            <div class='text-subtitle2 text-bold text-primary'>SYNC LIBRARY</div>
            <div class='text-subtitle2 q-mb-md text-grey-6'>
                Match your local files against the provider's catalog
            </div>
            <div class='row justify-center q-gutter-sm'>
                <q-btn
                    outline color='secondary' label='Sync Library' icon='mdi-sync'
                    :loading='syncing' @click='syncLibrary'
                    :disable='!providerConfig.path || !isConnected'
                />
                <q-btn
                    outline color='grey-7' label='Reset Cache' icon='mdi-delete-outline'
                    @click='resetCache'
                    :disable='!selectedProvider'
                />
            </div>
            <div v-if='!providerConfig.path && isConnected' class='text-caption text-grey-6 q-mt-xs'>
                Enter a path above first
            </div>
            <div v-if='!isConnected' class='text-caption text-grey-6 q-mt-xs'>
                Test your connection first
            </div>
            <!-- Sync Results -->
            <div v-if='$1t.afSyncResult.value' class='q-mt-md'>
                <div v-if='$1t.afSyncResult.value.success' class='sync-results'>
                    <div class='text-subtitle2 text-weight-bold q-mb-sm' style='color: #00D2BF;'>Sync Complete</div>
                    <div class='row justify-center q-gutter-md'>
                        <div class='sync-stat'>
                            <div class='sync-stat-number'>{{ $1t.afSyncResult.value.catalogSize }}</div>
                            <div class='sync-stat-label'>Provider Tracks</div>
                        </div>
                        <div class='sync-stat'>
                            <div class='sync-stat-number'>{{ $1t.afSyncResult.value.localTotal }}</div>
                            <div class='sync-stat-label'>Local Files</div>
                        </div>
                        <div class='sync-stat'>
                            <div class='sync-stat-number text-positive'>{{ $1t.afSyncResult.value.matched }}</div>
                            <div class='sync-stat-label'>Matched</div>
                        </div>
                        <div class='sync-stat'>
                            <div class='sync-stat-number' :class='$1t.afSyncResult.value.unmatchedLocal > 0 ? "text-warning" : ""'>{{ $1t.afSyncResult.value.unmatchedLocal }}</div>
                            <div class='sync-stat-label'>Unmatched Local</div>
                        </div>
                        <div class='sync-stat'>
                            <div class='sync-stat-number' :class='$1t.afSyncResult.value.unmatchedProvider > 0 ? "text-orange" : ""'>{{ $1t.afSyncResult.value.unmatchedProvider }}</div>
                            <div class='sync-stat-label'>Orphaned in Provider</div>
                        </div>
                    </div>
                </div>
                <div v-else>
                    <q-badge color='negative' class='q-pa-sm'>
                        Sync failed: {{ $1t.afSyncResult.value.message }}
                    </q-badge>
                </div>
            </div>
        </div>

        <!-- Prominent Tag -->
        <div v-if='selectedProvider'>
            <q-separator class='q-mx-auto' style='max-width: 513px; margin-top: 20px; margin-bottom: 25px' inset color="dark"/>
            <div class='text-subtitle2 text-bold text-primary'>PROMINENT TAG</div>
            <div class='text-subtitle2 text-grey-6'>Converts feature values to labels based on threshold</div>
            <div class='text-subtitle2 q-mt-xs q-mb-md text-grey-4'>e.g. #dance-high, #energy-med, #gentle, #relaxed</div>
            <TagFields style='max-width: 550px; margin: auto;' v-model='providerConfig.mainTag'></TagFields>
        </div>

        <!-- Dynamic Properties -->
        <div v-if='selectedProvider && selectedProvider.capabilities.features.length > 0'>
            <q-separator class='q-mx-auto' style='max-width: 513px; margin-top: 20px; margin-bottom: 25px' inset color="dark"/>
            <div class='text-subtitle2 text-bold text-primary'>PROPERTIES</div>
            <div class='q-px-xl'>
                <div class='row text-subtitle3 text-weight-medium q-mb-md text-grey-6'>
                    <div class='col-1'>Include</div>
                    <div class='col-2'>Feature</div>
                    <div class='col-6'>Tag frame</div>
                    <div class='col-3'>Threshold</div>
                </div>
                <div v-for='feature in selectedProvider.capabilities.features' :key='feature.id'>
                    <div class='row' v-if='providerConfig.featureConfig[feature.id]'>
                        <div class='col-1'>
                            <q-checkbox v-model='providerConfig.featureConfig[feature.id].enabled' class='checkbox'></q-checkbox>
                        </div>
                        <div class='col-2'>
                            <q-badge outline color='primary'><span class='text-uppercase text-grey-3'>{{ feature.name }}</span></q-badge>
                        </div>
                        <div class='col-6'>
                            <TagFields dense v-model='providerConfig.featureConfig[feature.id].tag'></TagFields>
                        </div>
                        <div class='col-3 q-px-md'>
                            <q-range label :min='0' :max='100'
                                :model-value='{ min: providerConfig.featureConfig[feature.id].thresholdMin, max: providerConfig.featureConfig[feature.id].thresholdMax }'
                                @update:model-value='(v) => { providerConfig.featureConfig[feature.id].thresholdMin = v.min; providerConfig.featureConfig[feature.id].thresholdMax = v.max; }'
                                class='t-range' color='grey-8'
                            ></q-range>
                        </div>
                    </div>
                </div>
            </div>
        </div>

        <!-- Additional Tags -->
        <div v-if='selectedProvider'>
            <q-separator class='q-mx-auto' style='max-width: 513px; margin-top: 20px; margin-bottom: 25px' inset color="dark"/>
            <div class='text-subtitle2 text-bold text-primary'>ADDITIONAL TAGS</div>
            <div class='column flex-center q-gutter-sm'>
                <q-toggle v-if='selectedProvider.capabilities.providesBpm'
                    class='justify-between' style='width: 300px;'
                    label='Write BPM' left-label v-model='providerConfig.writeBpm' />
                <q-toggle v-if='selectedProvider.capabilities.providesKey'
                    class='justify-between' style='width: 300px;'
                    label='Write Key' left-label v-model='providerConfig.writeKey' />
                <div v-if='selectedProvider.capabilities.providesGenre' class='column flex-center'>
                    <q-toggle class='justify-between' style='width: 300px;'
                        label='Write Genre (AI analysis)' left-label v-model='providerConfig.writeGenre' />
                    <div v-if='providerConfig.writeGenre' class='row items-center q-gutter-sm q-mb-sm'>
                        <span class='text-grey-6 text-caption'>Top</span>
                        <q-input filled dense type='number' v-model.number='providerConfig.genreCount' style='width: 60px;' />
                        <span class='text-grey-6 text-caption'>genres to</span>
                        <TagFields dense v-model='providerConfig.genreTag' style='width: 200px;'></TagFields>
                    </div>
                </div>
                <div v-if='selectedProvider.capabilities.providesMood' class='column flex-center'>
                    <q-toggle class='justify-between' style='width: 300px;'
                        label='Write Mood (AI analysis)' left-label v-model='providerConfig.writeMood' />
                    <div v-if='providerConfig.writeMood' class='row items-center q-gutter-sm q-mb-sm'>
                        <span class='text-grey-6 text-caption'>Top</span>
                        <q-input filled dense type='number' v-model.number='providerConfig.moodCount' style='width: 60px;' />
                        <span class='text-grey-6 text-caption'>moods to</span>
                        <TagFields dense v-model='providerConfig.moodTag' style='width: 200px;'></TagFields>
                    </div>
                </div>
            </div>
        </div>

        <!-- Separators & Options -->
        <div v-if='selectedProvider'>
            <q-separator class='q-mx-auto' style='max-width: 513px; margin-top: 20px; margin-bottom: 25px' inset color="dark"/>
            <div class='text-subtitle2 text-bold text-primary'>SEPARATORS</div>
            <div class='row q-pb-md q-mt-sm justify-center'>
                <Separators v-model='providerConfig.separators'></Separators>
            </div>
            <q-separator class='q-mx-auto' style='max-width: 513px; margin-top: 8px; margin-bottom: 25px' inset color="dark"/>
            <div class='text-subtitle2 text-bold text-primary' style='margin-bottom: 8px;'>OPTIONS</div>
            <div class='column flex-center'>
                <q-toggle class='justify-between' style='width: 200px;' label='Write OneTagger meta tag' left-label v-model='providerConfig.metaTag'></q-toggle>
                <q-toggle class='justify-between' style='width: 200px;' label='Skip already tagged tracks' left-label v-model='providerConfig.skipTagged'></q-toggle>
                <q-toggle class='justify-between' style='width: 200px;' label='Include subfolders' left-label v-model='providerConfig.includeSubfolders'></q-toggle>
            </div>
        </div>

        <div class='q-my-xl'></div>

        <!-- Start -->
        <q-page-sticky position='bottom-right' :offset='[36, 32]' v-if='selectedProvider'>
            <q-btn fab push icon='mdi-play' color='primary'
                :disable='!providerConfig.path && !playlist.data'
                @click='startProvider'>
                <q-tooltip anchor="top middle" self="bottom middle" :offset="[10, 10]">
                    <span class='text-weight-medium'>START</span>
                </q-tooltip>
            </q-btn>
        </q-page-sticky>
    </div>

    <!-- ========== LEGACY SPOTIFY MODE ========== -->
    <div v-if='mode === "legacy"'>
        <div v-if='!$1t.spotify.value.authorized'>
            <div class='text-subtitle2 text-bold text-primary q-mt-lg'>SETUP</div>
            <SpotifyLogin></SpotifyLogin>
            <div class='q-mt-xl text-subtitle2 text-grey-6' style='line-height: 24px'>
                Spotify audio features via ISRC or exact match.<br>
                <span class='text-weight-bold text-warning'>Note: Spotify now requires Premium for API access.</span>
            </div>
        </div>
        <div v-if='$1t.spotify.value.authorized'>
            <div class='text-subtitle2 text-bold text-primary q-mt-lg'>SELECT INPUT</div>
            <div class='row justify-center' style='max-width: 725px; margin: auto;'>
                <div class='col-1'></div>
                <q-input filled class='col-10' label='Path' v-model='legacyConfig.path'>
                    <template v-slot:append>
                        <q-btn round dense flat icon='mdi-open-in-app' class='text-grey-4' @click='browseLegacy'></q-btn>
                    </template>
                </q-input>
                <div class='col-1'></div>
            </div>
            <q-separator class='q-mx-auto' style='max-width: 513px; margin-top: 16px; margin-bottom: 35px' inset color="dark"/>
            <div class='text-subtitle2 text-bold text-primary custom-margin'>PROMINENT TAG</div>
            <TagFields style='max-width: 550px; margin: auto;' v-model='legacyConfig.mainTag'></TagFields>
            <q-separator class='q-mx-auto' style='max-width: 513px; margin-top: 20px; margin-bottom: 35px' inset color="dark"/>
            <div class='text-subtitle2 text-bold text-primary custom-margin'>PROPERTIES</div>
            <div class='q-px-xl'>
                <div class='row text-subtitle3 text-weight-medium q-mb-md text-grey-6'>
                    <div class='col-1'>Include</div>
                    <div class='col-2'>Audio feature</div>
                    <div class='col-6'>Tag frame</div>
                    <div class='col-3'>Threshold</div>
                </div>
                <div v-for='(_, key, i) in legacyConfig.properties' :key='"P"+i'>
                    <div class='row'>
                        <div class='col-1'><q-checkbox v-model='legacyConfig.properties[key].enabled' class='checkbox'></q-checkbox></div>
                        <div class='col-2'><q-badge outline color='primary'><span class='text-uppercase text-grey-3'>{{key}}</span></q-badge></div>
                        <div class='col-6'><TagFields dense v-model='legacyConfig.properties[key].tag'></TagFields></div>
                        <div class='col-3 q-px-md'>
                            <q-range label :min='0' :max='100' v-model='legacyConfig.properties[key].range' class='t-range' color='grey-8'></q-range>
                        </div>
                    </div>
                </div>
            </div>
            <q-separator class='q-mx-auto' style='max-width: 513px; margin-top: 20px; margin-bottom: 35px' inset color="dark"/>
            <div class='text-subtitle2 text-bold text-primary custom-margin'>OPTIONS</div>
            <div class='column flex-center'>
                <q-toggle class='justify-between' style='width: 200px;' label='Write OneTagger meta tag' left-label v-model='legacyConfig.metaTag'></q-toggle>
                <q-toggle class='justify-between' style='width: 200px;' label='Skip already tagged tracks' left-label v-model='legacyConfig.skipTagged'></q-toggle>
                <q-toggle class='justify-between' style='width: 200px;' label='Include subfolders' left-label v-model='legacyConfig.includeSubfolders'></q-toggle>
            </div>
            <div class='q-my-xl'></div>
            <q-page-sticky position='bottom-right' :offset='[36, 32]'>
                <q-btn fab push icon='mdi-play' color='primary' :disable='!legacyConfig.path' @click='startLegacy'>
                    <q-tooltip anchor="top middle" self="bottom middle" :offset="[10, 10]">
                        <span class='text-weight-medium'>START</span>
                    </q-tooltip>
                </q-btn>
            </q-page-sticky>
        </div>
    </div>

</div>
</div>
</template>

<script lang='ts' setup>
import TagFields from '../components/TagFields.vue';
import PlaylistDropZone from '../components/PlaylistDropZone.vue';
import Separators from '../components/Separators.vue';
import SpotifyLogin from '../components/SpotifyLogin.vue';
import { Playlist } from '../scripts/utils';
import { onMounted, ref, computed, watch } from 'vue';
import { AudioFeaturesConfig, ProviderAFConfig } from '../scripts/settings';
import { get1t } from '../scripts/onetagger';
import { useRouter } from 'vue-router';

const $1t = get1t();
const $router = useRouter();
const playlist = ref<Playlist>({});
const testing = ref(false);
const syncing = ref(false);

const mode = ref($1t.settings.value.audioFeatures?.mode || 'provider');

const providerConfig = ref(
    $1t.settings.value.audioFeatures?.providerConfig
        ? ProviderAFConfig.fromJson($1t.settings.value.audioFeatures.providerConfig)
        : new ProviderAFConfig()
);

const legacyConfig = ref(
    $1t.settings.value.audioFeatures?.config
        ? AudioFeaturesConfig.fromJson($1t.settings.value.audioFeatures.config)
        : new AudioFeaturesConfig()
);

const selectedProvider = computed(() => {
    return $1t.afProviders.value.find((p: any) => p.id === providerConfig.value.providerId);
});

const providerOptions = computed(() => {
    return $1t.afProviders.value.map((p: any) => ({
        label: p.name,
        value: p.id,
        description: p.description,
        icon: p.icon,
    }));
});

const isConnected = computed(() => {
    return $1t.afConnectionStatus.value?.success === true;
});

function onProviderChange() {
    const provider = selectedProvider.value;
    if (provider) {
        providerConfig.value.initFromCapabilities(provider.capabilities.features);
    }
}

function browse() { $1t.browse('af', providerConfig.value.path); }
function browseLegacy() { $1t.browse('af', legacyConfig.value.path); }

function testConnection() {
    testing.value = true;
    $1t.afConnectionStatus.value = null;
    $1t.send('aFTestConnection', {
        providerId: providerConfig.value.providerId,
        config: providerConfig.value.providerConfig,
    });
    setTimeout(() => { testing.value = false; }, 10000);
}

function syncLibrary() {
    syncing.value = true;
    $1t.afSyncResult.value = null;
    $1t.send('aFSync', {
        providerId: providerConfig.value.providerId,
        config: providerConfig.value.providerConfig,
        path: providerConfig.value.path,
        includeSubfolders: providerConfig.value.includeSubfolders,
    });
    setTimeout(() => { syncing.value = false; }, 300000);
}

function resetCache() {
    $1t.afSyncResult.value = null;
    $1t.afConnectionStatus.value = null;
    $1t.send('aFResetCache', {
        providerId: providerConfig.value.providerId,
    });
}

function startProvider() {
    $1t.settings.value.audioFeatures.mode = 'provider';
    $1t.settings.value.audioFeatures.providerConfig = providerConfig.value;
    $1t.saveSettings();

    let p: any = null;
    if (playlist.value && playlist.value.data) p = playlist.value;

    providerConfig.value.type = 'providerAudioFeatures';
    let c = JSON.parse(JSON.stringify(providerConfig.value));

    setTimeout(() => { $1t.send('startTagging', { config: c, playlist: p }); }, 100);
    setTimeout(() => { $router.push('/audiofeatures/status'); }, 10);
}

function startLegacy() {
    $1t.settings.value.audioFeatures.mode = 'legacy';
    $1t.settings.value.audioFeatures.config = legacyConfig.value;
    $1t.saveSettings();

    legacyConfig.value.type = 'audioFeatures';
    let c = JSON.parse(JSON.stringify(legacyConfig.value));

    setTimeout(() => { $1t.send('startTagging', { config: c, playlist: null }); }, 100);
    setTimeout(() => { $router.push('/audiofeatures/status'); }, 10);
}

watch(() => $1t.afConnectionStatus.value, () => { testing.value = false; });
watch(() => $1t.afSyncResult.value, () => { syncing.value = false; });

watch(() => $1t.afProviders.value, (providers) => {
    if (providers.length > 0 && selectedProvider.value) {
        providerConfig.value.initFromCapabilities(selectedProvider.value.capabilities.features);
    }
});

onMounted(() => {
    if (selectedProvider.value) {
        providerConfig.value.initFromCapabilities(selectedProvider.value.capabilities.features);
    }
    $1t.onAudioFeaturesEvent = (json: any) => {
        switch (json.action) {
            case 'browse':
                if (mode.value === 'provider') {
                    providerConfig.value.path = json.path;
                } else {
                    legacyConfig.value.path = json.path;
                }
                break;
        }
    }
});
</script>

<style>
.af-wrapper { width: 100%; display: flex; justify-content: center; }
.af-content { width: 100%; max-width: 1400px; }
.t-range .q-slider__inner.absolute { background: var(--q-primary) !important; }
.custom-margin { margin-top: 35px !important; }
.click-highlight { padding: 4px; border-radius: 2px; background: #262828; margin-bottom: 4px; margin-left: 4px; }
.sync-results { padding: 16px; margin: 8px auto; max-width: 600px; background: rgba(0,210,191,0.05); border-radius: 8px; border: 1px solid rgba(0,210,191,0.15); }
.sync-stat { text-align: center; min-width: 80px; }
.sync-stat-number { font-size: 1.4em; font-weight: bold; color: #e0e0e0; }
.sync-stat-label { font-size: 0.75em; color: #888; margin-top: 2px; }
</style>
