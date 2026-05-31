import Adw from 'gi://Adw';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Gtk from 'gi://Gtk';

Gio._promisify(
    Gio.Subprocess.prototype,
    'communicate_utf8_async',
    'communicate_utf8_finish'
);

const DAEMON_SERVICE = 'chirper-daemon.service';
const SETTINGS_SCHEMA = 'org.gnome.shell.extensions.chirper';
const TOGGLE_RECORDING_KEY = 'toggle-recording';
const PASTE_AFTER_STOP_KEY = 'paste-after-stop';
const CHECK_UPDATES_KEY = 'check-updates';
const COMMON_WHISPER_DOWNLOADS = [
    'base',
    'small.en',
    'small',
    'medium',
    'large-v3-turbo',
    'large-v3-turbo-q5_0',
];
const AI_LOG_RETENTION_OPTIONS = [
    ['0', 'Off', 'Do not keep prompt logs.'],
    ['1', '1 Day', 'Delete prompt logs older than one day.'],
    ['7', '1 Week', 'Delete prompt logs older than one week.'],
    ['30', '30 Days', 'Delete prompt logs older than 30 days.'],
];

export function loadExtensionSettings(extensionPath) {
    const schemaDir = GLib.build_filenamev([extensionPath, 'schemas']);
    const source = Gio.SettingsSchemaSource.new_from_directory(
        schemaDir,
        Gio.SettingsSchemaSource.get_default(),
        false
    );
    const schema = source.lookup(SETTINGS_SCHEMA, false);

    return new Gio.Settings({settings_schema: schema});
}

export function buildPreferencesWindow(window, extensionPath, settings) {
    const builder = new ChirperPreferencesBuilder(window, extensionPath, settings);
    builder.build();
}

function loadJsonFile(path) {
    const file = Gio.File.new_for_path(path);

    try {
        const [, contents] = file.load_contents(null);
        return JSON.parse(new TextDecoder().decode(contents));
    } catch (_error) {
        return {};
    }
}

async function runCommand(argv) {
    const process = Gio.Subprocess.new(
        argv,
        Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE
    );
    const [stdout, stderr] = await process.communicate_utf8_async(null, null);

    if (!process.get_successful())
        throw new Error((stderr ?? '').trim() || `${argv[0]} failed`);

    return stdout;
}

function runDetached(argv) {
    Gio.Subprocess.new(
        argv,
        Gio.SubprocessFlags.STDOUT_SILENCE | Gio.SubprocessFlags.STDERR_SILENCE
    );
}

function formatAccelerator(value) {
    if (!value)
        return 'Unset';

    return value
        .replace(/</g, '')
        .replace(/>/g, '+')
        .replace(/\+$/g, '')
        .replace(/\bspace\b/i, 'Space')
        .replace(/\bctrl\b/i, 'Ctrl')
        .replace(/\balt\b/i, 'Alt')
        .replace(/\bsuper\b/i, 'Super');
}

function formatBytes(bytes) {
    if (!bytes)
        return '';

    const units = ['B', 'KiB', 'MiB', 'GiB'];
    let value = bytes;
    let unit = 0;

    while (value >= 1024 && unit < units.length - 1) {
        value /= 1024;
        unit++;
    }

    return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

function modelSubtitle(model, selected = false) {
    const parts = [];

    if (selected)
        parts.push('Selected');

    const size = formatBytes(model.bytes);
    if (size)
        parts.push(size);

    if (model.path)
        parts.push(model.path);

    return parts.join(' - ');
}

function addButton(row, label, callback, options = {}) {
    const button = new Gtk.Button({
        label,
        valign: Gtk.Align.CENTER,
        sensitive: options.sensitive ?? true,
    });

    if (options.suggested)
        button.add_css_class('suggested-action');

    button.connect('clicked', callback);
    row.add_suffix(button);
    row.activatable_widget = button;
    return button;
}

class ChirperPreferencesBuilder {
    constructor(window, extensionPath, settings) {
        this._window = window;
        this._extensionPath = extensionPath;
        this._settings = settings;
        this._runtime = loadJsonFile(GLib.build_filenamev([extensionPath, 'runtime.json']));
        this._audioRows = [];
        this._languageRows = [];
        this._transcriptionRows = [];
        this._installedRows = [];
        this._downloadRows = [];
        this._aiTierRows = [];
        this._aiLogRows = [];
        this._ollamaRows = [];
        this._refreshingAiFormatting = false;
        this._updateChecking = false;
        this._updateRunning = false;
    }

    build() {
        const page = new Adw.PreferencesPage({
            title: 'Chirper',
            icon_name: 'audio-input-microphone-symbolic',
        });
        this._window.add(page);

        const generalGroup = new Adw.PreferencesGroup({
            title: 'General',
        });
        page.add(generalGroup);

        const pasteRow = new Adw.SwitchRow({
            title: 'Paste After Stop',
            subtitle: 'Paste the copied transcript into the previously focused window.',
            active: this._settings.get_boolean(PASTE_AFTER_STOP_KEY),
        });
        pasteRow.connect('notify::active', row => {
            this._settings.set_boolean(PASTE_AFTER_STOP_KEY, row.get_active());
        });
        generalGroup.add(pasteRow);

        const shortcut = this._settings.get_strv(TOGGLE_RECORDING_KEY)[0];
        generalGroup.add(new Adw.ActionRow({
            title: 'Recording Shortcut',
            subtitle: formatAccelerator(shortcut),
        }));

        const updateCheckRow = new Adw.SwitchRow({
            title: 'Automatic Update Checks',
            subtitle: 'The GNOME extension checks periodically and notifies when the installed source checkout is behind upstream.',
            active: this._settings.get_boolean(CHECK_UPDATES_KEY),
        });
        updateCheckRow.connect('notify::active', row => {
            this._settings.set_boolean(CHECK_UPDATES_KEY, row.get_active());
        });
        generalGroup.add(updateCheckRow);

        const audioGroup = new Adw.PreferencesGroup({
            title: 'Audio Input',
            description: 'Choose the microphone or capture source used for normal recordings.',
        });
        page.add(audioGroup);

        this._currentAudioRow = new Adw.ActionRow({
            title: 'Current Input',
            subtitle: 'Loading',
        });
        audioGroup.add(this._currentAudioRow);

        const refreshAudioRow = new Adw.ActionRow({
            title: 'Refresh Inputs',
            subtitle: 'Reload PipeWire sources from the current session.',
        });
        addButton(refreshAudioRow, 'Refresh', () => this._refreshAudioInputs());
        audioGroup.add(refreshAudioRow);

        this._audioInputsGroup = new Adw.PreferencesGroup({
            title: 'Available Inputs',
        });
        page.add(this._audioInputsGroup);

        const languageGroup = new Adw.PreferencesGroup({
            title: 'Transcription Language',
            description: 'Force the language passed to whisper.cpp. Auto detection can be unreliable for multilingual conversations.',
        });
        page.add(languageGroup);

        this._currentLanguageRow = new Adw.ActionRow({
            title: 'Current Language',
            subtitle: 'Loading',
        });
        languageGroup.add(this._currentLanguageRow);

        const refreshLanguageRow = new Adw.ActionRow({
            title: 'Refresh Languages',
            subtitle: 'Reload common whisper.cpp language options.',
        });
        addButton(refreshLanguageRow, 'Refresh', () => this._refreshLanguages());
        languageGroup.add(refreshLanguageRow);

        this._languagesGroup = new Adw.PreferencesGroup({
            title: 'Languages',
        });
        page.add(this._languagesGroup);

        const transcriptionGroup = new Adw.PreferencesGroup({
            title: 'Transcription Speed',
            description: 'Choose whisper.cpp decoding behavior. Fast mode trades some accuracy and context for lower latency.',
        });
        page.add(transcriptionGroup);

        this._currentTranscriptionRow = new Adw.ActionRow({
            title: 'Current Profile',
            subtitle: 'Loading',
        });
        transcriptionGroup.add(this._currentTranscriptionRow);

        const refreshTranscriptionRow = new Adw.ActionRow({
            title: 'Refresh Transcription Profiles',
            subtitle: 'Reload the configured whisper.cpp transcription profile.',
        });
        addButton(refreshTranscriptionRow, 'Refresh', () => this._refreshTranscriptionProfiles());
        transcriptionGroup.add(refreshTranscriptionRow);

        this._transcriptionGroup = new Adw.PreferencesGroup({
            title: 'Profiles',
        });
        page.add(this._transcriptionGroup);

        const daemonGroup = new Adw.PreferencesGroup({
            title: 'Daemon',
        });
        page.add(daemonGroup);

        this._daemonStatusRow = new Adw.ActionRow({
            title: 'Daemon Status',
            subtitle: 'Use the panel menu to see live recording state.',
        });
        daemonGroup.add(this._daemonStatusRow);

        const restartRow = new Adw.ActionRow({
            title: 'Restart Daemon',
            subtitle: 'Restarts the user systemd service.',
        });
        addButton(restartRow, 'Restart', () => this._restartDaemon());
        daemonGroup.add(restartRow);

        const configPath = GLib.build_filenamev([
            GLib.getenv('XDG_CONFIG_HOME') ?? GLib.build_filenamev([GLib.get_home_dir(), '.config']),
            'chirper',
        ]);
        const configRow = new Adw.ActionRow({
            title: 'Config Folder',
            subtitle: configPath,
        });
        addButton(configRow, 'Open', () => this._openConfigFolder(configPath));
        daemonGroup.add(configRow);

        const updateGroup = new Adw.PreferencesGroup({
            title: 'Updates',
            description: 'Checks the installed Chirper source checkout against its upstream branch.',
        });
        page.add(updateGroup);

        this._updateStatusRow = new Adw.ActionRow({
            title: 'Update Status',
            subtitle: 'Not checked yet',
        });
        updateGroup.add(this._updateStatusRow);

        const updateActionsRow = new Adw.ActionRow({
            title: 'Update Chirper',
            subtitle: 'Pulls, rebuilds, reinstalls the user service and GNOME extension, then restarts the daemon.',
        });
        this._updateButton = addButton(updateActionsRow, 'Update', () => this._runUpdate(), {
            suggested: true,
        });
        this._updateCheckButton = addButton(updateActionsRow, 'Check', () => this._checkUpdates());
        updateGroup.add(updateActionsRow);

        const modelGroup = new Adw.PreferencesGroup({
            title: 'Whisper Models',
            description: 'Select an installed local model or download one through whisper.cpp.',
        });
        page.add(modelGroup);

        this._currentModelRow = new Adw.ActionRow({
            title: 'Current Model',
            subtitle: 'Loading',
        });
        modelGroup.add(this._currentModelRow);

        const refreshModelsRow = new Adw.ActionRow({
            title: 'Refresh Models',
            subtitle: 'Reload installed models from the Chirper config.',
        });
        addButton(refreshModelsRow, 'Refresh', () => this._refreshModels());
        modelGroup.add(refreshModelsRow);

        this._installedGroup = new Adw.PreferencesGroup({
            title: 'Installed Models',
        });
        page.add(this._installedGroup);

        this._downloadGroup = new Adw.PreferencesGroup({
            title: 'Download Models',
        });
        page.add(this._downloadGroup);

        const aiGroup = new Adw.PreferencesGroup({
            title: 'AI Formatting',
            description: 'Use Ollama after transcription to produce the final text that is copied or pasted.',
        });
        page.add(aiGroup);

        this._aiFormattingSwitch = new Adw.SwitchRow({
            title: 'AI Formatting',
            subtitle: 'When off, Chirper uses local rules only.',
        });
        this._aiFormattingSwitch.connect('notify::active', row => {
            if (!this._refreshingAiFormatting)
                this._setAiFormattingEnabled(row.get_active());
        });
        aiGroup.add(this._aiFormattingSwitch);

        this._aiStatusRow = new Adw.ActionRow({
            title: 'Selected AI Model',
            subtitle: 'Loading',
        });
        aiGroup.add(this._aiStatusRow);

        this._aiPreloadSwitch = new Adw.SwitchRow({
            title: 'Preload While Recording',
            subtitle: 'Loads the selected Ollama model when recording starts, then unloads after formatting.',
        });
        this._aiPreloadSwitch.connect('notify::active', row => {
            if (!this._refreshingAiFormatting)
                this._setAiPreload(row.get_active());
        });
        aiGroup.add(this._aiPreloadSwitch);

        this._aiPromptLogRow = new Adw.ActionRow({
            title: 'Prompt Logs',
            subtitle: 'Loading',
        });
        addButton(this._aiPromptLogRow, 'Open', () => this._openPromptLogFolder());
        aiGroup.add(this._aiPromptLogRow);

        const refreshAiRow = new Adw.ActionRow({
            title: 'Refresh AI Formatting',
            subtitle: 'Reload the current AI formatting config.',
        });
        addButton(refreshAiRow, 'Refresh', () => this._refreshAiFormatting());
        aiGroup.add(refreshAiRow);

        this._aiTierGroup = new Adw.PreferencesGroup({
            title: 'AI Hardware Presets',
            description: 'Presets choose the Ollama model Chirper uses for AI formatting.',
        });
        page.add(this._aiTierGroup);

        this._aiLogGroup = new Adw.PreferencesGroup({
            title: 'AI Prompt Log Retention',
        });
        page.add(this._aiLogGroup);

        const ollamaGroup = new Adw.PreferencesGroup({
            title: 'Ollama',
            description: 'Use an installed Ollama model to polish dictated text after local rules run.',
        });
        page.add(ollamaGroup);
        this._ollamaStatusRow = new Adw.ActionRow({
            title: 'Formatter',
            subtitle: 'Loading',
        });
        ollamaGroup.add(this._ollamaStatusRow);

        const rulesRow = new Adw.ActionRow({
            title: 'Rules Only',
            subtitle: 'Fast local punctuation and symbol replacement.',
        });
        addButton(rulesRow, 'Use', () => this._selectFormatter('rules'));
        ollamaGroup.add(rulesRow);

        const noneRow = new Adw.ActionRow({
            title: 'No Formatter',
            subtitle: 'Copy Whisper output without cleanup.',
        });
        addButton(noneRow, 'Use', () => this._selectFormatter('none'));
        ollamaGroup.add(noneRow);

        const codexRow = new Adw.ActionRow({
            title: 'Codex CLI',
            subtitle: 'Use `codex exec` for proofreading after local rules run.',
        });
        addButton(codexRow, 'Use', () => this._selectFormatter('codex'));
        ollamaGroup.add(codexRow);

        const refreshOllamaRow = new Adw.ActionRow({
            title: 'Refresh Ollama Models',
            subtitle: 'Reload installed models from `ollama list`.',
        });
        addButton(refreshOllamaRow, 'Refresh', () => this._refreshOllama());
        ollamaGroup.add(refreshOllamaRow);

        this._ollamaModelsGroup = new Adw.PreferencesGroup({
            title: 'Installed Ollama Models',
        });
        page.add(this._ollamaModelsGroup);

        this._refreshAudioInputs();
        this._refreshLanguages();
        this._refreshTranscriptionProfiles();
        this._refreshModels();
        this._refreshAiFormatting();
        this._refreshOllama();
        this._checkUpdates();
    }

    async _checkUpdates() {
        if (this._updateChecking || this._updateRunning)
            return;

        this._updateChecking = true;
        this._syncUpdateButtons();
        this._updateStatusRow.subtitle = 'Checking';

        try {
            const output = await this._runCli(['update-check', '--json']);
            const data = JSON.parse(output);
            this._lastUpdateStatus = data;
            this._updateStatusRow.subtitle = this._formatUpdateStatus(data);
        } catch (error) {
            this._lastUpdateStatus = null;
            this._updateStatusRow.subtitle = error.message;
        } finally {
            this._updateChecking = false;
            this._syncUpdateButtons();
        }
    }

    async _runUpdate() {
        if (this._updateRunning)
            return;

        this._updateRunning = true;
        this._syncUpdateButtons();
        this._updateStatusRow.subtitle = 'Updating';

        try {
            await this._runCli(['update']);
            this._updateStatusRow.subtitle = 'Update finished. Relog if the GNOME extension UI changed.';
            await this._checkUpdates();
        } catch (error) {
            this._updateStatusRow.subtitle = error.message;
        } finally {
            this._updateRunning = false;
            this._syncUpdateButtons();
        }
    }

    _syncUpdateButtons() {
        const busy = this._updateChecking || this._updateRunning;

        if (this._updateCheckButton)
            this._updateCheckButton.sensitive = !busy;

        if (this._updateButton) {
            const updateAvailable = Boolean(this._lastUpdateStatus?.update_available);
            this._updateButton.sensitive = !busy && updateAvailable;
        }
    }

    _formatUpdateStatus(data) {
        const branch = data.branch ?? 'unknown branch';
        const local = String(data.local_sha ?? '').slice(0, 7);
        const remote = String(data.upstream_sha ?? '').slice(0, 7);

        if (data.update_available)
            return `${data.behind} commit(s) behind ${branch}: ${local} -> ${remote}`;

        if (Number(data.ahead ?? 0) > 0)
            return `${branch} is ${data.ahead} commit(s) ahead of upstream`;

        if (data.dirty)
            return `${branch} is up to date with local changes`;

        return `${branch} is up to date`;
    }

    async _refreshAudioInputs() {
        this._currentAudioRow.subtitle = 'Loading';
        this._clearRows(this._audioInputsGroup, this._audioRows);

        try {
            const output = await this._runCli(['audio-list', '--json']);
            const data = JSON.parse(output);
            const current = data.current ?? {};
            const sources = data.sources ?? [];

            this._currentAudioRow.subtitle = current.label ?? 'Default microphone';

            const defaultRow = new Adw.ActionRow({
                title: 'Default microphone',
                subtitle: current.target ? 'Let PipeWire choose the default source' : 'Selected',
            });
            addButton(defaultRow, current.target ? 'Use' : 'Selected', () => this._selectAudioInput('auto'), {
                sensitive: Boolean(current.target),
            });
            this._audioInputsGroup.add(defaultRow);
            this._audioRows.push(defaultRow);

            for (const source of sources) {
                const row = new Adw.ActionRow({
                    title: source.label,
                    subtitle: source.target,
                });
                addButton(row, source.selected ? 'Selected' : 'Use', () => this._selectAudioInput(source.target), {
                    sensitive: !source.selected,
                });
                this._audioInputsGroup.add(row);
                this._audioRows.push(row);
            }

            if (sources.length === 0)
                this._addInfoRow(this._audioInputsGroup, this._audioRows, 'No PipeWire inputs found');
        } catch (error) {
            this._currentAudioRow.subtitle = 'Audio controls unavailable';
            this._addInfoRow(this._audioInputsGroup, this._audioRows, error.message);
        }
    }

    async _selectAudioInput(target) {
        this._currentAudioRow.subtitle = 'Selecting input';

        try {
            await this._runCli(['audio-use', target]);
            await this._refreshAudioInputs();
        } catch (error) {
            this._currentAudioRow.subtitle = error.message;
        }
    }

    async _refreshLanguages() {
        this._currentLanguageRow.subtitle = 'Loading';
        this._clearRows(this._languagesGroup, this._languageRows);

        try {
            const output = await this._runCli(['language-list', '--json']);
            const data = JSON.parse(output);
            const current = data.current ?? {};
            const languages = data.languages ?? [];

            this._currentLanguageRow.subtitle = current.label ?? current.code ?? 'Auto detect';

            for (const language of languages) {
                const selected = Boolean(language.selected);
                const row = new Adw.ActionRow({
                    title: language.label ?? language.code,
                    subtitle: language.code,
                });
                addButton(row, selected ? 'Selected' : 'Use', () => this._selectLanguage(language.code), {
                    sensitive: !selected,
                    suggested: language.code === 'id',
                });
                this._languagesGroup.add(row);
                this._languageRows.push(row);
            }

            if (languages.length === 0)
                this._addInfoRow(this._languagesGroup, this._languageRows, 'No language options available');
        } catch (error) {
            this._currentLanguageRow.subtitle = 'Language controls unavailable';
            this._addInfoRow(this._languagesGroup, this._languageRows, error.message);
        }
    }

    async _selectLanguage(language) {
        this._currentLanguageRow.subtitle = `Selecting ${language}`;

        try {
            await this._runCli(['language-use', language]);
            await this._refreshLanguages();
        } catch (error) {
            this._currentLanguageRow.subtitle = error.message;
        }
    }

    async _refreshTranscriptionProfiles() {
        this._currentTranscriptionRow.subtitle = 'Loading';
        this._clearRows(this._transcriptionGroup, this._transcriptionRows);

        try {
            const output = await this._runCli(['transcription-list', '--json']);
            const data = JSON.parse(output);
            const current = data.current ?? {};
            const profiles = data.profiles ?? [];

            const currentName = current.label ?? current.profile ?? 'Balanced';
            const currentDescription = current.description?.trim();
            this._currentTranscriptionRow.subtitle = currentDescription
                ? `${currentName} - ${currentDescription}`
                : currentName;

            for (const profile of profiles) {
                const selected = Boolean(profile.selected);
                const row = new Adw.ActionRow({
                    title: profile.label ?? profile.name,
                    subtitle: profile.description ?? profile.name,
                });
                addButton(row, selected ? 'Selected' : 'Use', () => this._selectTranscriptionProfile(profile.name), {
                    sensitive: !selected,
                    suggested: !selected,
                });
                this._transcriptionGroup.add(row);
                this._transcriptionRows.push(row);
            }

            if (profiles.length === 0)
                this._addInfoRow(this._transcriptionGroup, this._transcriptionRows, 'No transcription profiles available');
        } catch (error) {
            this._currentTranscriptionRow.subtitle = 'Transcription controls unavailable';
            this._addInfoRow(this._transcriptionGroup, this._transcriptionRows, error.message);
        }
    }

    async _selectTranscriptionProfile(profile) {
        this._currentTranscriptionRow.subtitle = `Selecting ${profile}`;

        try {
            await this._runCli(['transcription-use', profile]);
            await this._refreshTranscriptionProfiles();
        } catch (error) {
            this._currentTranscriptionRow.subtitle = error.message;
        }
    }

    async _refreshModels() {
        this._currentModelRow.subtitle = 'Loading';
        this._clearRows(this._installedGroup, this._installedRows);
        this._clearRows(this._downloadGroup, this._downloadRows);

        try {
            const output = await this._runCli(['model-list', '--json']);
            const data = JSON.parse(output);
            const current = data.current?.name ?? 'unset';
            const installed = data.installed ?? [];
            const available = data.available ?? [];
            const installedNames = new Set(installed.map(model => model.name));

            this._currentModelRow.subtitle = data.current?.path ?? current;

            if (installed.length === 0) {
                this._addInfoRow(this._installedGroup, this._installedRows, 'No local models found');
            } else {
                for (const model of installed) {
                    const selected = model.name === current;
                    const row = new Adw.ActionRow({
                        title: model.name,
                        subtitle: modelSubtitle(model, selected),
                    });
                    addButton(row, selected ? 'Selected' : 'Use', () => this._selectModel(model.name), {
                        sensitive: !selected,
                    });
                    this._installedGroup.add(row);
                    this._installedRows.push(row);
                }
            }

            let downloadCount = 0;
            for (const name of COMMON_WHISPER_DOWNLOADS) {
                if (!available.some(model => model.name === name) || installedNames.has(name))
                    continue;

                const row = new Adw.ActionRow({
                    title: name,
                    subtitle: 'Download and select this model',
                });
                addButton(row, 'Download', () => this._downloadModel(name), {
                    suggested: name === 'small.en',
                });
                this._downloadGroup.add(row);
                this._downloadRows.push(row);
                downloadCount++;
            }

            if (downloadCount === 0)
                this._addInfoRow(this._downloadGroup, this._downloadRows, 'Common models are already installed');
        } catch (error) {
            this._currentModelRow.subtitle = 'Model controls unavailable';
            this._addInfoRow(this._installedGroup, this._installedRows, error.message);
            this._addInfoRow(this._downloadGroup, this._downloadRows, 'Check that Chirper has been built.');
        }
    }

    async _selectModel(model) {
        this._currentModelRow.subtitle = 'Selecting model';

        try {
            await this._runCli(['model-use', model]);
            await this._refreshModels();
        } catch (error) {
            this._currentModelRow.subtitle = error.message;
        }
    }

    async _downloadModel(model) {
        this._currentModelRow.subtitle = 'Downloading model';

        try {
            await this._runCli(['model-download', model, '--select']);
            await this._refreshModels();
        } catch (error) {
            this._currentModelRow.subtitle = error.message;
        }
    }

    async _refreshAiFormatting() {
        this._aiStatusRow.subtitle = 'Loading';
        this._aiPromptLogRow.subtitle = 'Loading';
        this._clearRows(this._aiTierGroup, this._aiTierRows);
        this._clearRows(this._aiLogGroup, this._aiLogRows);

        try {
            const output = await this._runCli(['ai-format-current', '--json']);
            const data = JSON.parse(output);
            const enabled = Boolean(data.enabled);
            const currentTier = data.hardware_tier ?? 'high';
            const tiers = data.tiers ?? [];
            this._aiCurrentTier = currentTier;

            this._refreshingAiFormatting = true;
            this._aiFormattingSwitch.active = enabled;
            this._aiPreloadSwitch.active = Boolean(data.preload_on_recording);
            this._refreshingAiFormatting = false;

            this._aiStatusRow.subtitle = enabled
                ? `${data.hardware_tier_label ?? currentTier}: ${data.model}`
                : `Off, using ${data.backend ?? 'rules'}`;
            this._aiPromptLogRow.subtitle = `${data.prompt_log_dir ?? 'Prompt log folder'} - keep ${this._formatLogDays(data.log_retention_days)}`;
            this._aiPromptLogPath = data.prompt_log_dir;

            for (const tier of tiers) {
                const selected = tier.name === currentTier && enabled;
                const row = new Adw.ActionRow({
                    title: tier.label ?? tier.name,
                    subtitle: `${tier.description ?? ''} - ${tier.model ?? ''}`,
                });
                addButton(row, selected ? 'Selected' : 'Use', () => this._selectAiTier(tier.name), {
                    sensitive: !selected,
                    suggested: tier.name === 'high',
                });
                this._aiTierGroup.add(row);
                this._aiTierRows.push(row);
            }

            for (const [days, title, subtitle] of AI_LOG_RETENTION_OPTIONS) {
                const selected = String(data.log_retention_days ?? 7) === days;
                const row = new Adw.ActionRow({title, subtitle});
                addButton(row, selected ? 'Selected' : 'Use', () => this._setAiLogRetention(days), {
                    sensitive: !selected,
                });
                this._aiLogGroup.add(row);
                this._aiLogRows.push(row);
            }
        } catch (error) {
            this._refreshingAiFormatting = false;
            this._aiStatusRow.subtitle = 'AI formatting controls unavailable';
            this._aiPromptLogRow.subtitle = error.message;
            this._addInfoRow(this._aiTierGroup, this._aiTierRows, error.message);
            this._addInfoRow(this._aiLogGroup, this._aiLogRows, 'Check that Chirper has been built.');
        }
    }

    async _setAiFormattingEnabled(enabled) {
        this._aiStatusRow.subtitle = enabled ? 'Enabling AI formatting' : 'Disabling AI formatting';

        try {
            await this._runCli(['ai-format-use', enabled ? (this._aiCurrentTier ?? 'high') : 'off']);
            await this._refreshAiFormatting();
            await this._refreshOllama();
        } catch (error) {
            this._aiStatusRow.subtitle = error.message;
            await this._refreshAiFormatting();
        }
    }

    async _selectAiTier(tier) {
        this._aiStatusRow.subtitle = `Selecting ${tier}`;

        try {
            await this._runCli(['ai-format-use', tier]);
            await this._refreshAiFormatting();
            await this._refreshOllama();
        } catch (error) {
            this._aiStatusRow.subtitle = error.message;
        }
    }

    async _setAiLogRetention(days) {
        this._aiPromptLogRow.subtitle = `Setting retention to ${this._formatLogDays(days)}`;

        try {
            await this._runCli(['ai-format-logs', days]);
            await this._refreshAiFormatting();
        } catch (error) {
            this._aiPromptLogRow.subtitle = error.message;
        }
    }

    async _setAiPreload(enabled) {
        this._aiStatusRow.subtitle = enabled ? 'Enabling preload' : 'Disabling preload';

        try {
            await this._runCli(['ai-format-preload', enabled ? 'on' : 'off']);
            await this._refreshAiFormatting();
        } catch (error) {
            this._aiStatusRow.subtitle = error.message;
            await this._refreshAiFormatting();
        }
    }

    _formatLogDays(days) {
        const numeric = Number(days);
        if (!Number.isFinite(numeric) || numeric <= 0)
            return 'off';
        if (numeric === 1)
            return '1 day';
        if (numeric === 7)
            return '1 week';

        return `${numeric} days`;
    }

    _openPromptLogFolder() {
        const path = this._aiPromptLogPath;
        if (path)
            this._openConfigFolder(path);
    }

    async _refreshOllama() {
        this._ollamaStatusRow.subtitle = 'Loading';
        this._clearRows(this._ollamaModelsGroup, this._ollamaRows);

        try {
            const output = await this._runCli(['ollama-list', '--json']);
            const data = JSON.parse(output);
            const formatter = data.formatter ?? 'rules';
            const current = data.current?.model ?? 'unset';
            const models = data.models ?? [];

            this._ollamaStatusRow.subtitle = formatter === 'ollama'
                ? `Ollama: ${current}`
                : formatter;

            if (!data.available) {
                this._addInfoRow(
                    this._ollamaModelsGroup,
                    this._ollamaRows,
                    data.error ?? 'Ollama unavailable'
                );
                return;
            }

            if (models.length === 0) {
                this._addInfoRow(this._ollamaModelsGroup, this._ollamaRows, 'No Ollama models found');
                return;
            }

            for (const model of models) {
                const selected = formatter === 'ollama' && model.selected;
                const row = new Adw.ActionRow({
                    title: model.name,
                    subtitle: selected ? 'Selected for LLM formatting' : 'Installed',
                });
                addButton(row, selected ? 'Selected' : 'Use', () => this._selectOllamaModel(model.name), {
                    sensitive: !selected,
                    suggested: !selected,
                });
                this._ollamaModelsGroup.add(row);
                this._ollamaRows.push(row);
            }
        } catch (error) {
            this._ollamaStatusRow.subtitle = 'Formatter controls unavailable';
            this._addInfoRow(this._ollamaModelsGroup, this._ollamaRows, error.message);
        }
    }

    async _selectFormatter(formatter) {
        this._ollamaStatusRow.subtitle = `Selecting ${formatter}`;

        try {
            await this._runCli(['formatter-use', formatter]);
            await this._refreshOllama();
        } catch (error) {
            this._ollamaStatusRow.subtitle = error.message;
        }
    }

    async _selectOllamaModel(model) {
        this._ollamaStatusRow.subtitle = `Selecting ${model}`;

        try {
            await this._runCli(['ollama-use', model]);
            await this._refreshOllama();
        } catch (error) {
            this._ollamaStatusRow.subtitle = error.message;
        }
    }

    async _restartDaemon() {
        this._daemonStatusRow.subtitle = 'Restarting';

        try {
            await runCommand(['systemctl', '--user', 'restart', DAEMON_SERVICE]);
            this._daemonStatusRow.subtitle = 'Daemon restarted';
        } catch (error) {
            this._daemonStatusRow.subtitle = error.message;
        }
    }

    _openConfigFolder(path) {
        try {
            GLib.mkdir_with_parents(path, 0o755);
            runDetached(['xdg-open', path]);
        } catch (error) {
            this._daemonStatusRow.subtitle = error.message;
        }
    }

    async _runCli(args) {
        const cliPath = this._runtime.cliPath || 'chirper';
        return await runCommand([cliPath, ...args]);
    }

    _clearRows(group, rows) {
        for (const row of rows)
            group.remove(row);

        rows.length = 0;
    }

    _addInfoRow(group, rows, title) {
        const row = new Adw.ActionRow({title});
        row.set_sensitive(false);
        group.add(row);
        rows.push(row);
    }
}
