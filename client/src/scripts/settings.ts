import { AutotaggerConfig, AutotaggerProfile } from "./autotagger";
import { EnergyTag, QuickTagCustom, QuickTagGenre, QuickTagMood } from "./quicktag";
import { FrameName, Keybind, Separators } from "./utils";

class Settings {
    path?: string;
    autoTaggerConfig: any = {};
    autoTaggerSinglePage: boolean = false;
    autoTaggerProfiles: AutotaggerProfile[] = [];
    autoTaggerProfile: string = 'Default';
    showAutoTaggerProfiles: boolean = false;
    primaryColor: string = '#00D2BF';
    volume: number = 0.05;
    helpButton: boolean = true;
    continuePlayback: boolean = false;
    clientSidePlayer: boolean = false;
    nonNativeBrowser: boolean = false;
    playOnSeek: boolean = false;
    autoPlayNext: boolean = false;
    externalAudioPlayer: boolean = false;
    devtools: boolean = false;

    renamer: RenamerSettings = new RenamerSettings();
    quickTag: QuickTagSettings = new QuickTagSettings();
    audioFeatures: AudioFeaturesSettings = new AudioFeaturesSettings();

    tagEditorDouble: boolean = false;
    tagEditorCustom: string[] = [];
    tagEditorAutosave: boolean =  false;
    tagEditorPlayer: boolean =  false;

    // Read from json
    static fromJson(data: any): Settings {
        let settings: Settings = Object.assign(new Settings(), data);
        settings.renamer = Object.assign(new RenamerSettings(), data.renamer);
        settings.audioFeatures = AudioFeaturesSettings.fromJson(data.audioFeatures);
        settings.quickTag = QuickTagSettings.fromJson(data.quickTag);
        return settings;
    }

    /// Save new AT profile
    saveATProfile(name: string, config: AutotaggerConfig) {
        let i = this.autoTaggerProfiles.findIndex(p => p.name == name);
        if (i == -1) {
            this.autoTaggerProfiles.push({ name, config: JSON.parse(JSON.stringify(config)) });
            return;
        }
        this.autoTaggerProfiles[i] = { name, config: JSON.parse(JSON.stringify(config)) };
    }
}

class RenamerSettings {
    path?: string;
    outDir?: string;
    template?: string;
    copy: boolean = false;
    subfolders: boolean = true;
    overwrite: boolean = false;
}

class AudioFeaturesSettings {
    spotifyClientId?: string;
    spotifyClientSecret?: string;
    config?: AudioFeaturesConfig;
    /// Provider-based config (new system)
    providerConfig?: ProviderAFConfig;
    /// Which mode: 'legacy' (Spotify direct) or 'provider' (new system)
    mode: string = 'provider';

    static fromJson(data: any): AudioFeaturesSettings {
        let a: AudioFeaturesSettings = Object.assign(new AudioFeaturesSettings(), data);
        if (data.config)
            a.config = AudioFeaturesConfig.fromJson(data.config);
        if (data.providerConfig)
            a.providerConfig = ProviderAFConfig.fromJson(data.providerConfig);
        return a;
    }
}

/// Config for the provider-based audio features system
class ProviderAFConfig {
    providerId: string = 'audiomuse';
    path?: string;
    mainTag = FrameName.same('AUDIO_FEATURES');
    separators = new Separators();
    featureConfig: Record<string, ProviderAFFeatureConfig> = {};
    metaTag: boolean = true;
    skipTagged: boolean = false;
    includeSubfolders: boolean = true;
    writeBpm: boolean = true;
    writeKey: boolean = true;
    writeGenre: boolean = true;
    genreCount: number = 3;
    writeMood: boolean = true;
    moodCount: number = 3;
    genreTag = FrameName.same('GENRE');
    moodTag = FrameName.same('AM_MOOD');
    /// Provider-specific config (e.g., AudioMuse URL)
    providerConfig: any = { apiUrl: 'http://localhost:8000', apiToken: null };
    type?: string;

    static fromJson(data: any): ProviderAFConfig {
        let c: ProviderAFConfig = Object.assign(new ProviderAFConfig(), data);
        if (data.mainTag) c.mainTag = FrameName.fromJson(data.mainTag);
        if (data.genreTag) c.genreTag = FrameName.fromJson(data.genreTag);
        if (data.moodTag) c.moodTag = FrameName.fromJson(data.moodTag);
        if (data.featureConfig) {
            for (const [k, v] of Object.entries(data.featureConfig)) {
                c.featureConfig[k] = ProviderAFFeatureConfig.fromJson(v);
            }
        }
        return c;
    }

    /// Build feature configs from provider capabilities
    initFromCapabilities(features: any[]) {
        for (const f of features) {
            if (!this.featureConfig[f.id]) {
                this.featureConfig[f.id] = new ProviderAFFeatureConfig(
                    f.defaultThresholdMin, f.defaultThresholdMax, f.defaultTag
                );
            }
        }
    }
}

class ProviderAFFeatureConfig {
    tag: FrameName;
    enabled: boolean;
    thresholdMin: number;
    thresholdMax: number;

    constructor(min: number, max: number, frameName: string) {
        this.tag = FrameName.same(frameName);
        this.thresholdMin = min;
        this.thresholdMax = max;
        this.enabled = true;
    }

    static fromJson(data: any): ProviderAFFeatureConfig {
        let p = new ProviderAFFeatureConfig(
            data.thresholdMin || 0, data.thresholdMax || 100, ''
        );
        p.enabled = data.enabled ?? true;
        if (data.tag) p.tag = FrameName.fromJson(data.tag);
        return p;
    }
}


class QuickTagSettings {
    id3v24: boolean = false;
    recursive: boolean = false;
    autosave: boolean = false;
    sortDescending: boolean = false;
    sortOption: string = 'title';
    trackIndex: number = -1;
    separators: Separators = new Separators();
    noteTag: NoteTagSettings = new NoteTagSettings();
    id3CommLang?: string;
    thinTracks: boolean = false;

    moods: QuickTagMood[] = [
        {mood: 'Happy', color: 'amber'},
        {mood: 'Sad', color: 'indigo'},
        {mood: 'Bright', color: 'green'},
        {mood: 'Dark', color: 'deep-purple'},
        {mood: 'Angry', color: 'red'},
        {mood: 'Chill', color: 'teal'},
        {mood: 'Lovely', color: 'pink'},
        {mood: 'Powerful', color: 'light-blue'},
        {mood: 'Sexy', color: 'purple'}
    ];
    moodTag: FrameName = new FrameName('TMOO', 'MOOD', 'iTunes:MOOD');

    energyTag: EnergyTag = new EnergyTag();
    energyKeys: (Keybind | undefined)[] = [undefined, undefined, undefined, undefined, undefined];

    genres: QuickTagGenre[] = [
        {genre: '2-step', subgenres: []},
        {genre: 'Acid', subgenres: []},
        {genre: 'Breakbeat', subgenres: []},
        {genre: 'Disco', subgenres: []},
        {genre: 'Drum & Bass', subgenres: []},
        {genre: 'Electro', subgenres: ['House', 'Dubstep', 'EDM']},
        {genre: 'Funk', subgenres: []},
        {genre: 'Hardcore', subgenres: []},
        {genre: 'Hiphop', subgenres: []},
        {genre: 'House', subgenres: []},
        {genre: 'Industrial', subgenres: []},
        {genre: 'Jungle', subgenres: []},
        {genre: 'Latin', subgenres: []},
        {genre: 'Minimal', subgenres: []},
        {genre: 'Nu-Disco', subgenres: []},
        {genre: 'Pop', subgenres: []},
        {genre: 'Reggae', subgenres: []},
        {genre: 'Rock', subgenres: []},
        {genre: 'Techno', subgenres: []},
        {genre: 'Trance', subgenres: []},
    ];

    custom: QuickTagCustom[] = [
        {
            name: 'Vibe',
            tag: new FrameName('COMM', 'COMMENT', '©cmt'),
            values: [
                {val: 'Afro'},
                {val: 'Asian'},
                {val: 'Arabic'},
                {val: 'Classic'},
                {val: 'Dirty'},
                {val: 'Etnic'},
                {val: 'Funky'},
                {val: 'Gangsta'},
                {val: 'Glitchy'},
                {val: 'Melodic'},
                {val: 'Sensual'},
                {val: 'Soulful'},
            ],
        }, {
            name: 'Situation',
            tag: new FrameName('COMM', 'COMMENT', '©cmt'),
            values: [
                {val: 'Start'},
                {val: 'Build'},
                {val: 'Peak'},
                {val: 'Sustain'},
                {val: 'Release'},
            ]
        }, {
            name: 'Instruments',
            tag: new FrameName('COMM', 'COMMENT', '©cmt'),
            values: [
                {val: 'Vocals'},
                {val: 'Bass Heavy'},
                {val: 'Congas'},
                {val: 'Guitar'},
                {val: 'Horns'},
                {val: 'Organ'},
                {val: 'Piano'},
                {val: 'Strings'},
                {val: 'Sax'},
            ]
        }
    ]

    // Custom subgenre tag
    subgenreTag?: FrameName;

    // Manually load from JSON to restore classes
    static fromJson(data: any): QuickTagSettings {
        let qt: QuickTagSettings = Object.assign(new QuickTagSettings(), data);
        qt.noteTag = NoteTagSettings.fromJson(data.noteTag);
        qt.moodTag = FrameName.fromJson(data.moodTag);
        qt.energyTag = EnergyTag.fromJson(data.energyTag);
        qt.energyKeys = data.energyKeys.map((d: any) => Keybind.fromJson(d));
        qt.genres = qt.genres.map((g) => {
            if (g.keybind) {
                g.keybind = Keybind.fromJson(g.keybind);
            }
            return g;
        });
        qt.custom = qt.custom.map((c) => {
            c.tag = FrameName.fromJson(c.tag);
            c.values = c.values.map((v) => {
                if (v.keybind) {
                    v.keybind = Keybind.fromJson(v.keybind)
                }
                return v;
            });
            return c;
        });
        qt.subgenreTag = qt.subgenreTag ? FrameName.fromJson(data.subgenreTag) : undefined;
        return qt;
    }
}

class NoteTagSettings {
    tag: FrameName = new FrameName('COMM', 'COMMENT', '©cmt');
    keybind?: Keybind

    static fromJson(data: any): NoteTagSettings {
        let nt = new NoteTagSettings();
        nt.tag = FrameName.fromJson(data.tag);
        nt.keybind = Keybind.fromJson(data.keybind);
        return nt;
    }
}

class AudioFeaturesConfig {
    path?: string;
    metaTag = true;
    skipTagged = false;
    includeSubfolders = true;
    mainTag = FrameName.same('AUDIO_FEATURES');
    separators = new Separators();
    properties: Record<string, AudioFeaturesProperty> = {
        acousticness: new AudioFeaturesProperty(0, 90, '1T_ACOUSTICNESS'),
        danceability: new AudioFeaturesProperty(20, 80, '1T_DANCEABILITY'),
        energy: new AudioFeaturesProperty(20, 90, '1T_ENERGY'),
        instrumentalness: new AudioFeaturesProperty(50, 90, '1T_INSTRUMENTALNESS'),
        liveness: new AudioFeaturesProperty(0, 80, '1T_LIVENESS'), 
        speechiness: new AudioFeaturesProperty(0, 70, '1T_SPEECHINESS'),
        valence: new AudioFeaturesProperty(15, 85, '1T_VALENCE'), 
        popularity: new AudioFeaturesProperty(0, 80, '1T_POPULARITY'), 
    }
    type?: string;

    static fromJson(data: any): AudioFeaturesConfig {
        let c: AudioFeaturesConfig = Object.assign(new AudioFeaturesConfig(), data);
        c.mainTag = FrameName.fromJson(data.mainTag);
        for (const [k, v] of Object.entries(data.properties)) {
            c.properties[k] = AudioFeaturesProperty.fromJson(v);
        }
        return c;
    }
}

class AudioFeaturesProperty {
    tag: FrameName;
    enabled: boolean;
    range: { min: number, max: number }

    constructor(min: number, max: number, frameName: string) {
        this.tag = FrameName.same(frameName);
        this.range = { min, max };
        this.enabled = true;
    }

    static fromJson(data: any): AudioFeaturesProperty {
        let p = new AudioFeaturesProperty(data.range.min, data.range.max, '');
        p.enabled = data.enabled;
        p.tag = FrameName.fromJson(data.tag);
        return p;
    }
}

export { Settings, QuickTagSettings, AudioFeaturesConfig, AudioFeaturesProperty, ProviderAFConfig, ProviderAFFeatureConfig };