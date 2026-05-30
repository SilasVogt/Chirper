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
const COMMON_WHISPER_DOWNLOADS = [
    'base',
    'small.en',
    'small',
    'medium',
    'large-v3-turbo',
    'large-v3-turbo-q5_0',
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
        this._installedRows = [];
        this._downloadRows = [];
        this._ollamaRows = [];
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
        this._refreshModels();
        this._refreshOllama();
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
